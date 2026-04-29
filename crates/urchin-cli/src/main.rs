use clap::{Parser, Subcommand};
use anyhow::Result;

#[derive(Parser)]
#[command(name = "urchin", about = "Local-first memory sync substrate for AI tools")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the background daemon (intake + sync)
    Serve,
    /// Start the MCP server over stdio
    Mcp,
    /// Show status and health
    Doctor,
    /// Ingest an event from the command line
    Ingest {
        #[arg(short, long)]
        content: String,
        #[arg(short, long)]
        source: Option<String>,
        #[arg(short, long)]
        workspace: Option<String>,
        #[arg(short, long)]
        title: Option<String>,
        #[arg(short = 'T', long, value_delimiter = ',')]
        tags: Vec<String>,
        /// Event kind: conversation | agent | command | commit | file (default: conversation)
        #[arg(short, long, default_value = "conversation")]
        kind: String,
    },
    /// Run a collector once and append new events to the journal
    Collect {
        #[command(subcommand)]
        which: CollectKind,
    },
    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Set a configuration key (e.g. cloud_url, cloud_token)
    Set {
        key: String,
        value: String,
    },
    /// Print current configuration
    Show,
}

#[derive(Subcommand)]
enum CollectKind {
    /// Tail ~/.bash_history for new commands
    Shell,
    /// Ingest commits from one or more git repos.
    /// Repos can be passed via --repo (repeatable) or via URCHIN_REPO_ROOTS (colon-separated).
    Git {
        #[arg(short, long)]
        repo: Vec<String>,
    },
    /// Ingest prompts from ~/.claude/history.jsonl
    Claude,
    /// Run every collector that has a default path (shell, git via URCHIN_REPO_ROOTS, claude)
    All,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            std::env::var("URCHIN_LOG").unwrap_or_else(|_| "urchin=info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve  => serve().await,
        Commands::Mcp    => mcp().await,
        Commands::Doctor => doctor().await,
        Commands::Ingest { content, source, workspace, title, tags, kind } => {
            ingest(content, source, workspace, title, tags, kind)
        }
        Commands::Collect { which } => collect(which),
        Commands::Config { action } => config_cmd(action),
    }
}

async fn serve() -> Result<()> {
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::watch;
    use tokio::task::spawn_blocking;
    use tokio::time::{interval, Duration};
    use urchin_collectors::{claude as claude_col, git as git_col, shell as shell_col};
    use urchin_core::{config::Config, identity::Identity, journal::Journal};

    let cfg      = Config::load();
    let identity = Arc::new(Identity::resolve());
    let jp       = cfg.journal_path.clone();

    // Cloud shuttle config — cloned before cfg is borrowed by intake server
    let cloud_url   = cfg.cloud_url.clone();
    let cloud_token = cfg.cloud_token.clone();
    let shuttle_offset_path = cfg.cache_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("cloud-sync.offset");

    // ── Shutdown plumbing ────────────────────────────────────────────────────
    let (stop_tx, stop_rx) = watch::channel(false);

    let tx_ctrlc = stop_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("[DAEMON] Ctrl+C received, shutting down.");
        let _ = tx_ctrlc.send(true);
    });

    // ── Background collector + shuttle tick loop ─────────────────────────────
    let tick_id = Arc::clone(&identity);
    let tick_jp = jp.clone();
    let mut tick_stop = stop_rx.clone();

    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(60));
        ticker.tick().await; // skip the immediate first fire

        loop {
            tokio::select! {
                _ = tick_stop.changed() => {
                    if *tick_stop.borrow() { break; }
                }
                _ = ticker.tick() => {}
            }

            println!("[DAEMON] Tick started: collecting across all channels...");

            let id = Arc::clone(&tick_id);
            let jp = tick_jp.clone();

            let total = spawn_blocking(move || -> usize {
                let journal = Journal::new(jp);
                let mut n = 0usize;

                let opts = shell_col::ShellOpts::defaults();
                match shell_col::collect(&journal, &id, &opts) {
                    Ok(k)  => n += k,
                    Err(e) => tracing::warn!("[DAEMON] shell: {}", e),
                }

                if let Ok(roots) = std::env::var("URCHIN_REPO_ROOTS") {
                    for root in roots.split(':').filter(|s| !s.is_empty()) {
                        let opts = git_col::GitOpts::defaults_for(
                            PathBuf::from(root)
                        );
                        match git_col::collect_repo(&journal, &id, &opts) {
                            Ok(k)  => n += k,
                            Err(e) => tracing::warn!("[DAEMON] git: {}", e),
                        }
                    }
                }

                let opts = claude_col::ClaudeOpts::defaults();
                match claude_col::collect(&journal, &id, &opts) {
                    Ok(k)  => n += k,
                    Err(e) => tracing::warn!("[DAEMON] claude: {}", e),
                }

                n
            }).await.unwrap_or(0);

            println!("[DAEMON] Tick complete: {} total events ingested.", total);

            // ── Cloud Shuttle ────────────────────────────────────────────────
            if let Some(ref url) = cloud_url {
                let jp2 = tick_jp.clone();
                let offset_path = shuttle_offset_path.clone();

                let read_result = spawn_blocking(move || {
                    let offset = read_offset(&offset_path);
                    let journal = Journal::new(jp2);
                    journal.read_from_byte_offset(offset)
                        .map(|(events, new_off)| (events, new_off, offset_path))
                }).await;

                match read_result {
                    Ok(Ok((events, new_offset, offset_path))) if !events.is_empty() => {
                        let mut client = urchin_sdk::UrchinClient::new(url.as_str());
                        if let Some(ref t) = cloud_token {
                            client = client.with_token(t.as_str());
                        }
                        let mut shuttled = 0usize;
                        for event in &events {
                            match client.ingest(event).await {
                                Ok(_)  => shuttled += 1,
                                Err(e) => {
                                    tracing::warn!("[DAEMON] shuttle ingest failed: {}", e);
                                    break;
                                }
                            }
                        }
                        if shuttled == events.len() {
                            save_offset(&offset_path, new_offset);
                        }
                        println!("[DAEMON] Shuttled {} events to Cloud Hub.", shuttled);
                    }
                    Ok(Err(e)) => tracing::warn!("[DAEMON] shuttle read: {}", e),
                    _ => {}
                }
            }
        }

        tracing::info!("[DAEMON] Collector loop stopped.");
    });

    // ── Intake server with graceful shutdown ─────────────────────────────────
    let mut intake_stop = stop_rx;
    urchin_intake::server::serve_with_shutdown(
        &cfg,
        async move { intake_stop.changed().await.ok(); },
    ).await
}

fn read_offset(path: &std::path::Path) -> u64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn save_offset(path: &std::path::Path, offset: u64) {
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(path, offset.to_string());
}

async fn mcp() -> Result<()> {
    let cfg = urchin_core::config::Config::load();
    urchin_mcp::server::run(cfg).await
}

async fn doctor() -> Result<()> {
    use urchin_core::{config::Config, identity::Identity, journal::Journal};

    let cfg = Config::load();
    let identity = Identity::resolve();
    let journal = Journal::new(cfg.journal_path.clone());
    let stats = journal.stats()?;

    println!("urchin doctor");
    println!();

    println!("  identity:");
    println!("    account:  {}", identity.account);
    println!("    device:   {}", identity.device);
    println!();

    println!("  config:");
    let config_path = Config::config_path();
    let config_source = if config_path.exists() {
        config_path.display().to_string()
    } else {
        format!("{} (not found, using defaults)", config_path.display())
    };
    println!("    config:   {}", config_source);
    println!("    vault:    {}", cfg.vault_root.display());
    println!("    intake:   {}", cfg.intake_port);
    println!();

    println!("  journal:");
    if stats.event_count == 0 && !journal.exists() {
        println!("    path:     {}", cfg.journal_path.display());
        println!("    status:   not found");
    } else {
        println!("    path:     {}", cfg.journal_path.display());
        println!("    events:   {}", stats.event_count);
        println!("    size:     {} KB", stats.file_size_bytes / 1024);
        if let Some(last) = stats.last_event {
            println!("    last:     {} ({})", last.timestamp.format("%Y-%m-%dT%H:%M:%SZ"), last.source);
        }
    }

    Ok(())
}

fn ingest(
    content: String,
    source: Option<String>,
    workspace: Option<String>,
    title: Option<String>,
    tags: Vec<String>,
    kind: String,
) -> Result<()> {
    use urchin_core::{
        config::Config,
        event::{Actor, Event, EventKind},
        identity::Identity,
        journal::Journal,
    };

    let cfg = Config::load();
    let journal = Journal::new(cfg.journal_path);
    let identity = Identity::resolve();

    let event_kind = match kind.as_str() {
        "agent"        => EventKind::Agent,
        "command"      => EventKind::Command,
        "commit"       => EventKind::Commit,
        "file"         => EventKind::File,
        "conversation" => EventKind::Conversation,
        other          => EventKind::Other(other.to_string()),
    };

    let mut event = Event::new(
        source.unwrap_or_else(|| "cli".into()),
        event_kind,
        content,
    );
    event.workspace = workspace;
    event.title = title;
    event.tags = tags;
    event.actor = Some(Actor {
        account: Some(identity.account),
        device: Some(identity.device),
        workspace: event.workspace.clone(),
    });

    journal.append(&event)?;
    println!("ingested: {}", event.id);
    Ok(())
}

fn collect(which: CollectKind) -> Result<()> {
    use urchin_collectors::{claude as claude_col, git as git_col, shell as shell_col};
    use urchin_core::{config::Config, identity::Identity, journal::Journal};

    let cfg = Config::load();
    let identity = Identity::resolve();
    let journal = Journal::new(cfg.journal_path.clone());

    match which {
        CollectKind::Shell => {
            let opts = shell_col::ShellOpts::defaults();
            let n = shell_col::collect(&journal, &identity, &opts)?;
            println!("shell: {} new events", n);
        }
        CollectKind::Git { repo } => {
            let repos = resolve_repos(repo);
            if repos.is_empty() {
                eprintln!("no repos given. Pass --repo <path> or set URCHIN_REPO_ROOTS.");
                return Ok(());
            }
            let mut total = 0;
            for r in &repos {
                let opts = git_col::GitOpts::defaults_for(r.clone());
                match git_col::collect_repo(&journal, &identity, &opts) {
                    Ok(n) => {
                        println!("git {}: {} new commits", r.display(), n);
                        total += n;
                    }
                    Err(e) => eprintln!("git {} skipped: {}", r.display(), e),
                }
            }
            println!("git total: {}", total);
        }
        CollectKind::Claude => {
            let opts = claude_col::ClaudeOpts::defaults();
            let n = claude_col::collect(&journal, &identity, &opts)?;
            println!("Collected {} new events from Claude CLI history.", n);
        }
        CollectKind::All => {
            let opts = shell_col::ShellOpts::defaults();
            match shell_col::collect(&journal, &identity, &opts) {
                Ok(n)  => println!("shell: {} new events", n),
                Err(e) => eprintln!("shell skipped: {}", e),
            }
            for r in &resolve_repos(vec![]) {
                let opts = git_col::GitOpts::defaults_for(r.clone());
                match git_col::collect_repo(&journal, &identity, &opts) {
                    Ok(n)  => println!("git {}: {} new commits", r.display(), n),
                    Err(e) => eprintln!("git {} skipped: {}", r.display(), e),
                }
            }
            let opts = claude_col::ClaudeOpts::defaults();
            match claude_col::collect(&journal, &identity, &opts) {
                Ok(n)  => println!("claude: {} new events", n),
                Err(e) => eprintln!("claude skipped: {}", e),
            }
        }
    }

    Ok(())
}

fn config_cmd(action: ConfigAction) -> Result<()> {
    use urchin_core::config::Config;
    match action {
        ConfigAction::Set { key, value } => {
            Config::set_field(&key, &value)?;
            println!("set {} = {}", key, value);
        }
        ConfigAction::Show => {
            let cfg = Config::load();
            println!("vault_root:    {}", cfg.vault_root.display());
            println!("journal_path:  {}", cfg.journal_path.display());
            println!("cache_path:    {}", cfg.cache_path.display());
            println!("intake_port:   {}", cfg.intake_port);
            println!("remote_host:   {}", cfg.remote_host.as_deref().unwrap_or("-"));
            println!("cloud_url:     {}", cfg.cloud_url.as_deref().unwrap_or("-"));
            println!("cloud_token:   {}", cfg.cloud_token.as_deref().map(|_| "<set>").unwrap_or("-"));
        }
    }
    Ok(())
}

fn resolve_repos(from_args: Vec<String>) -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut out: Vec<PathBuf> = from_args.into_iter().map(PathBuf::from).collect();
    if out.is_empty() {
        if let Ok(env) = std::env::var("URCHIN_REPO_ROOTS") {
            out.extend(env.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
        }
    }
    out
}
