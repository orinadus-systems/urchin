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
    /// Run a one-shot cloud sync pass and exit
    Sync,
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
    /// Show recent journal events
    Recent {
        /// Number of events to show (default: 20)
        #[arg(short, long, default_value = "20")]
        n: usize,
        /// Filter by source (e.g. claude, shell, copilot)
        #[arg(short, long)]
        source: Option<String>,
        /// Look back this many hours (default: 168 = 1 week)
        #[arg(long, default_value = "168")]
        hours: f64,
    },
    /// Search journal events by keyword
    Query {
        /// Substring to search for (case-insensitive)
        text: String,
        /// Filter by source
        #[arg(short, long)]
        source: Option<String>,
        /// Max results (default: 20)
        #[arg(short, long, default_value = "20")]
        limit: usize,
        /// Look back this many hours (default: 168 = 1 week)
        #[arg(long, default_value = "168")]
        hours: f64,
    },
    /// Vault operations
    Vault {
        #[command(subcommand)]
        action: VaultAction,
    },
    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
enum VaultAction {
    /// Project today's journal events into ~/brain/daily/YYYY-MM-DD.md
    Project {
        /// Date to project (default: today, format: YYYY-MM-DD)
        #[arg(short, long)]
        date: Option<String>,
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
    /// Ingest prompts from ~/.copilot/command-history-state.json
    Copilot,
    /// Ingest prompts from ~/.gemini/history
    Gemini,
    /// Ingest sessions from ~/.codex/state_5.sqlite
    Codex,
    /// Ingest sessions from ~/.local/share/opencode/opencode.db
    Opencode,
    /// Ingest records from ~/.local/share/urchin/local-model.jsonl
    LocalModel,
    /// Run every collector that has a default path (shell, git via URCHIN_REPO_ROOTS, claude, copilot, gemini)
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
        Commands::Recent { n, source, hours }  => recent(n, source, hours),
        Commands::Query  { text, source, limit, hours } => query(text, source, limit, hours),
        Commands::Vault  { action } => vault_cmd(action),
        Commands::Config { action } => config_cmd(action),
        Commands::Sync => sync().await,
    }
}

async fn serve() -> Result<()> {
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::watch;
    use tokio::task::spawn_blocking;
    use tokio::time::{interval, Duration};
    use urchin_collectors::CollectorRegistry;
    use urchin_core::{config::Config, identity::Identity, journal::Journal};

    let cfg      = Config::load();
    let identity = Arc::new(Identity::resolve());
    let jp       = cfg.journal_path.clone();

    // Cloud shuttle config — cloned before cfg is borrowed by intake server
    let cloud_url          = cfg.cloud_url.clone();
    let cloud_token        = cfg.cloud_token.clone();
    let shuttle_offset_path = shuttle_offset_path(&cfg);

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
                let journal = Arc::new(Journal::new(jp));
                let repos: Vec<PathBuf> = std::env::var("URCHIN_REPO_ROOTS")
                    .unwrap_or_default()
                    .split(':')
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .collect();

                CollectorRegistry::with_defaults(&repos).run_all(&journal, &id)
                    .into_iter()
                    .map(|r| match r.count {
                        Ok(n) => {
                            if n > 0 { tracing::info!("[DAEMON] {}: {} events", r.name, n); }
                            n
                        }
                        Err(e) => { tracing::warn!("[DAEMON] {}: {}", r.name, e); 0 }
                    })
                    .sum()
            }).await.unwrap_or(0);

            println!("[DAEMON] Tick complete: {} total events ingested.", total);

            // ── Cloud Shuttle ────────────────────────────────────────────────
            if let Some(ref url) = cloud_url {
                match run_shuttle(
                    url.as_str(),
                    cloud_token.as_deref(),
                    &tick_jp,
                    &shuttle_offset_path,
                ).await {
                    Ok((pushed, total)) if total > 0 => {
                        println!("[DAEMON] Shuttled {}/{} events to Cloud Hub.", pushed, total);
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("[DAEMON] shuttle: {}", e),
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
    use std::sync::Arc;
    use urchin_collectors::{
        claude as claude_col, codex as codex_col, copilot as copilot_col,
        gemini as gemini_col, git as git_col, local_model as lm_col,
        opencode as opencode_col, shell as shell_col, CollectorRegistry,
    };
    use urchin_core::{config::Config, identity::Identity, journal::Journal};

    let cfg      = Config::load();
    let identity = Arc::new(Identity::resolve());
    let journal  = Arc::new(Journal::new(cfg.journal_path.clone()));

    match which {
        CollectKind::Shell => {
            let n = shell_col::collect(&journal, &identity, &shell_col::ShellOpts::defaults())?;
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
            let n = claude_col::collect(&journal, &identity, &claude_col::ClaudeOpts::defaults())?;
            println!("claude: {} new events", n);
        }
        CollectKind::Copilot => {
            let n = copilot_col::collect(&journal, &identity, &copilot_col::CopilotOpts::defaults())?;
            println!("copilot: {} new events", n);
        }
        CollectKind::Gemini => {
            let n = gemini_col::collect(&journal, &identity, &gemini_col::GeminiOpts::defaults())?;
            println!("gemini: {} new events", n);
        }
        CollectKind::Codex => {
            let n = codex_col::collect(&journal, &identity, &codex_col::CodexOpts::defaults())?;
            println!("codex: {} new events", n);
        }
        CollectKind::Opencode => {
            let n = opencode_col::collect(&journal, &identity, &opencode_col::OpenCodeOpts::defaults())?;
            println!("opencode: {} new events", n);
        }
        CollectKind::LocalModel => {
            let n = lm_col::collect(&journal, &identity, &lm_col::LocalModelOpts::defaults())?;
            println!("local-model: {} new events", n);
        }
        CollectKind::All => {
            let repos = resolve_repos(vec![]);
            for r in CollectorRegistry::with_defaults(&repos).run_all(&journal, &identity) {
                match r.count {
                    Ok(n)  => println!("{}: {} new events", r.name, n),
                    Err(e) => eprintln!("{} skipped: {}", r.name, e),
                }
            }
        }
    }

    Ok(())
}

/// Drive a single shuttle pass: read unsynced events from journal, POST to cloud hub.
/// Returns (pushed, total). Advances the offset checkpoint only when pushed == total.
async fn run_shuttle(
    url: &str,
    token: Option<&str>,
    journal_path: &std::path::Path,
    offset_path: &std::path::Path,
) -> Result<(usize, usize)> {
    use urchin_core::journal::Journal;

    let jp = journal_path.to_path_buf();
    let op = offset_path.to_path_buf();

    let (events, new_offset) = tokio::task::spawn_blocking(move || {
        let offset = read_offset(&op);
        Journal::new(jp).read_from_byte_offset(offset)
    }).await??;

    if events.is_empty() {
        return Ok((0, 0));
    }

    let total = events.len();
    let mut client = urchin_sdk::UrchinClient::new(url);
    if let Some(t) = token {
        client = client.with_token(t);
    }

    let mut pushed = 0usize;
    for event in &events {
        match client.ingest(event).await {
            Ok(_) => pushed += 1,
            Err(e) => {
                if let Some(http) = e.downcast_ref::<urchin_sdk::HttpError>() {
                    eprintln!("[ERROR] Sync Rejected: {} - {}", http.status, http.body);
                } else {
                    eprintln!("[ERROR] {}", e);
                }
                break;
            }
        }
    }

    // Only advance the checkpoint when the full batch succeeded.
    // On failure the same batch will be retried on the next tick.
    if pushed == total {
        let op = offset_path.to_path_buf();
        tokio::task::spawn_blocking(move || save_offset(&op, new_offset)).await?;
    }

    Ok((pushed, total))
}

fn shuttle_offset_path(cfg: &urchin_core::config::Config) -> std::path::PathBuf {
    cfg.cache_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("cloud-sync.offset")
}

async fn sync() -> Result<()> {
    use urchin_core::config::Config;

    let cfg = Config::load();

    let Some(ref cloud_url) = cfg.cloud_url else {
        eprintln!("No cloud_url configured. Run: urchin config set cloud_url <url>");
        return Ok(());
    };

    let offset_path = shuttle_offset_path(&cfg);

    let (pushed, total) = run_shuttle(
        cloud_url.as_str(),
        cfg.cloud_token.as_deref(),
        &cfg.journal_path,
        &offset_path,
    ).await?;

    if total == 0 {
        println!("Already up to date. No events to sync.");
    } else if pushed == total {
        println!("Synced {} events.", pushed);
    } else {
        println!("Partial: {}/{} events pushed. Checkpoint not advanced — will retry on next sync.", pushed, total);
        std::process::exit(1);
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

fn recent(n: usize, source: Option<String>, hours: f64) -> Result<()> {
    use urchin_core::{config::Config, journal::Journal, query};

    let cfg     = Config::load();
    let journal = Journal::new(cfg.journal_path);
    let events  = journal.read_all()?;
    let hits    = query::recent(&events, hours, source.as_deref(), n);

    if hits.is_empty() {
        println!("(no events in window)");
    } else {
        for e in hits {
            let ts = e.timestamp.format("%Y-%m-%dT%H:%M:%SZ");
            println!("{}  {}  {}", ts, e.source, truncate_line(&e.content, 100));
        }
    }
    Ok(())
}

fn query(text: String, source: Option<String>, limit: usize, hours: f64) -> Result<()> {
    use urchin_core::{config::Config, journal::Journal, query};

    let cfg     = Config::load();
    let journal = Journal::new(cfg.journal_path);
    let events  = journal.read_all()?;
    let mut hits = query::search_content(&events, &text, hours, limit);

    // Post-filter by source if given.
    if let Some(ref src) = source {
        hits.retain(|e| e.source == *src);
    }

    if hits.is_empty() {
        println!("(no matches)");
    } else {
        println!("{} match(es) for {:?}:", hits.len(), text);
        for e in hits {
            let ts = e.timestamp.format("%Y-%m-%dT%H:%M:%SZ");
            println!("{}  {}  {}", ts, e.source, truncate_line(&e.content, 100));
        }
    }
    Ok(())
}

fn vault_cmd(action: VaultAction) -> Result<()> {
    use chrono::NaiveDate;
    use urchin_core::{config::Config, journal::Journal};
    use urchin_vault::projection;

    let cfg     = Config::load();
    let journal = Journal::new(cfg.journal_path);

    match action {
        VaultAction::Project { date } => {
            let d = match date {
                Some(s) => NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                    .map_err(|_| anyhow::anyhow!("invalid date format, expected YYYY-MM-DD"))?,
                None    => chrono::Local::now().naive_local().date(),
            };
            projection::project_daily(&journal, &cfg.vault_root, d)?;
            println!("projected {}", d.format("%Y-%m-%d"));
        }
    }
    Ok(())
}

fn truncate_line(s: &str, max: usize) -> String {
    let first = s.lines().next().unwrap_or("").trim();
    if first.len() > max {
        format!("{}…", &first[..max])
    } else {
        first.to_string()
    }
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
