//! urchin-collectors: passive readers for every tool's native output.
//!
//! # Architecture
//!
//! Each collector implements the [`Collector`] trait. The [`CollectorRegistry`]
//! holds all registered collectors and runs them in sequence.
//!
//! Adding a new collector:
//! 1. Create `src/<name>.rs` with `pub fn collect(journal, identity, opts) -> Result<usize>`
//! 2. Define a struct that implements `Collector`
//! 3. Add one `registry.register(MyCollector::new())` line in `with_defaults()`
//!
//! Collectors never write back to source tools.

use std::path::PathBuf;
use std::sync::Arc;

use urchin_core::{identity::Identity, journal::Journal};

pub mod state;

pub mod agent_bridge;
pub mod apple_health;
pub mod bank_csv;
pub mod calendar;
pub mod claude;
pub mod codex;
pub mod copilot;
pub mod gemini;
pub mod git;
pub mod google_takeout;
pub mod local_model;
pub mod opencode;
pub mod shell;

// ─── Trait ───────────────────────────────────────────────────────────────────

/// A passive reader that ingests new events from a single source tool.
///
/// Implementations must be `Send + Sync` so the registry can be used across
/// threads (the daemon runs collectors inside `spawn_blocking`).
pub trait Collector: Send + Sync {
    /// Short human-readable name used in log output and CLI.
    fn name(&self) -> &'static str;

    /// Collect new events and append them to the journal.
    /// Returns the number of events ingested this run.
    fn collect(&self, journal: &Journal, identity: &Identity) -> anyhow::Result<usize>;

    /// Whether the source this collector reads from is present on this machine.
    /// Return `false` to skip silently. Default: `true`.
    fn is_available(&self) -> bool {
        true
    }
}

// ─── Result ──────────────────────────────────────────────────────────────────

/// Output from one collector run.
pub struct CollectorResult {
    pub name: &'static str,
    pub count: anyhow::Result<usize>,
}

// ─── Registry ────────────────────────────────────────────────────────────────

/// Holds all registered collectors and drives them in order.
pub struct CollectorRegistry {
    collectors: Vec<Box<dyn Collector>>,
}

impl CollectorRegistry {
    /// Empty registry.
    pub fn new() -> Self {
        Self {
            collectors: Vec::new(),
        }
    }

    /// Registry pre-loaded with every built-in collector.
    ///
    /// `repo_roots` is forwarded to the git collector. If empty the git
    /// collector reads from `URCHIN_REPO_ROOTS`.
    pub fn with_defaults(repo_roots: &[PathBuf]) -> Self {
        let mut r = Self::new();
        r.register(ShellCollector::new());
        r.register(GitCollector::new(repo_roots));
        r.register(ClaudeCollector::new());
        r.register(CopilotCollector::new());
        r.register(GeminiCollector::new());
        r.register(CodexCollector::new());
        r.register(OpenCodeCollector::new());
        r.register(LocalModelCollector::new());
        r.register(GoogleTakeoutCollector::new());
        r.register(AppleHealthCollector::new());
        r.register(BankCsvCollector::new());
        r.register(CalendarCollector::new());
        r
    }

    /// Add a collector to the registry.
    pub fn register(&mut self, c: impl Collector + 'static) {
        self.collectors.push(Box::new(c));
    }

    /// Run every available collector in registration order.
    ///
    /// Collectors that return `is_available() == false` are silently skipped.
    pub fn run_all(
        &self,
        journal: &Arc<Journal>,
        identity: &Arc<Identity>,
    ) -> Vec<CollectorResult> {
        self.collectors
            .iter()
            .filter(|c| c.is_available())
            .map(|c| CollectorResult {
                name: c.name(),
                count: c.collect(journal.as_ref(), identity.as_ref()),
            })
            .collect()
    }
}

impl Default for CollectorRegistry {
    fn default() -> Self {
        Self::with_defaults(&[])
    }
}

// ─── Built-in collector structs ───────────────────────────────────────────────

struct ShellCollector {
    opts: shell::ShellOpts,
}
impl ShellCollector {
    fn new() -> Self {
        Self {
            opts: shell::ShellOpts::defaults(),
        }
    }
}
impl Collector for ShellCollector {
    fn name(&self) -> &'static str {
        "shell"
    }
    fn collect(&self, journal: &Journal, identity: &Identity) -> anyhow::Result<usize> {
        shell::collect(journal, identity, &self.opts)
    }
    fn is_available(&self) -> bool {
        self.opts.history_path.exists()
    }
}

struct GitCollector {
    repo_roots: Vec<PathBuf>,
}
impl GitCollector {
    fn new(roots: &[PathBuf]) -> Self {
        let repo_roots = if roots.is_empty() {
            std::env::var("URCHIN_REPO_ROOTS")
                .unwrap_or_default()
                .split(':')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect()
        } else {
            roots.to_vec()
        };
        Self { repo_roots }
    }
}
impl Collector for GitCollector {
    fn name(&self) -> &'static str {
        "git"
    }
    fn collect(&self, journal: &Journal, identity: &Identity) -> anyhow::Result<usize> {
        let mut total = 0usize;
        for repo in &self.repo_roots {
            total +=
                git::collect_repo(journal, identity, &git::GitOpts::defaults_for(repo.clone()))?;
        }
        Ok(total)
    }
    fn is_available(&self) -> bool {
        !self.repo_roots.is_empty()
    }
}

struct ClaudeCollector {
    opts: claude::ClaudeOpts,
}
impl ClaudeCollector {
    fn new() -> Self {
        Self {
            opts: claude::ClaudeOpts::defaults(),
        }
    }
}
impl Collector for ClaudeCollector {
    fn name(&self) -> &'static str {
        "claude"
    }
    fn collect(&self, journal: &Journal, identity: &Identity) -> anyhow::Result<usize> {
        claude::collect(journal, identity, &self.opts)
    }
    fn is_available(&self) -> bool {
        self.opts.history_path.exists()
    }
}

struct CopilotCollector {
    opts: copilot::CopilotOpts,
}
impl CopilotCollector {
    fn new() -> Self {
        Self {
            opts: copilot::CopilotOpts::defaults(),
        }
    }
}
impl Collector for CopilotCollector {
    fn name(&self) -> &'static str {
        "copilot"
    }
    fn collect(&self, journal: &Journal, identity: &Identity) -> anyhow::Result<usize> {
        copilot::collect(journal, identity, &self.opts)
    }
    fn is_available(&self) -> bool {
        self.opts.history_path.exists()
    }
}

struct GeminiCollector {
    opts: gemini::GeminiOpts,
}
impl GeminiCollector {
    fn new() -> Self {
        Self {
            opts: gemini::GeminiOpts::defaults(),
        }
    }
}
impl Collector for GeminiCollector {
    fn name(&self) -> &'static str {
        "gemini"
    }
    fn collect(&self, journal: &Journal, identity: &Identity) -> anyhow::Result<usize> {
        gemini::collect(journal, identity, &self.opts)
    }
    fn is_available(&self) -> bool {
        self.opts.chats_dir.exists()
    }
}

struct CodexCollector {
    opts: codex::CodexOpts,
}
impl CodexCollector {
    fn new() -> Self {
        Self {
            opts: codex::CodexOpts::defaults(),
        }
    }
}
impl Collector for CodexCollector {
    fn name(&self) -> &'static str {
        "codex"
    }
    fn collect(&self, journal: &Journal, identity: &Identity) -> anyhow::Result<usize> {
        codex::collect(journal, identity, &self.opts)
    }
    fn is_available(&self) -> bool {
        self.opts.db_path.exists()
    }
}

struct OpenCodeCollector {
    opts: opencode::OpenCodeOpts,
}
impl OpenCodeCollector {
    fn new() -> Self {
        Self {
            opts: opencode::OpenCodeOpts::defaults(),
        }
    }
}
impl Collector for OpenCodeCollector {
    fn name(&self) -> &'static str {
        "opencode"
    }
    fn collect(&self, journal: &Journal, identity: &Identity) -> anyhow::Result<usize> {
        opencode::collect(journal, identity, &self.opts)
    }
    fn is_available(&self) -> bool {
        self.opts.db_path.exists()
    }
}

struct LocalModelCollector {
    opts: local_model::LocalModelOpts,
}
impl LocalModelCollector {
    fn new() -> Self {
        Self {
            opts: local_model::LocalModelOpts::defaults(),
        }
    }
}
impl Collector for LocalModelCollector {
    fn name(&self) -> &'static str {
        "local-model"
    }
    fn collect(&self, journal: &Journal, identity: &Identity) -> anyhow::Result<usize> {
        local_model::collect(journal, identity, &self.opts)
    }
    fn is_available(&self) -> bool {
        self.opts.drop_file.exists()
    }
}

struct GoogleTakeoutCollector {
    opts: google_takeout::GoogleTakeoutOpts,
}
impl GoogleTakeoutCollector {
    fn new() -> Self {
        Self {
            opts: google_takeout::GoogleTakeoutOpts::defaults(),
        }
    }
}
impl Collector for GoogleTakeoutCollector {
    fn name(&self) -> &'static str {
        "google-takeout"
    }
    fn collect(&self, journal: &Journal, identity: &Identity) -> anyhow::Result<usize> {
        google_takeout::collect(journal, identity, &self.opts)
    }
    fn is_available(&self) -> bool {
        self.opts.import_dir.exists()
    }
}

struct AppleHealthCollector {
    opts: apple_health::AppleHealthOpts,
}
impl AppleHealthCollector {
    fn new() -> Self {
        Self {
            opts: apple_health::AppleHealthOpts::defaults(),
        }
    }
}
impl Collector for AppleHealthCollector {
    fn name(&self) -> &'static str {
        "apple-health"
    }
    fn collect(&self, journal: &Journal, identity: &Identity) -> anyhow::Result<usize> {
        apple_health::collect(journal, identity, &self.opts)
    }
    fn is_available(&self) -> bool {
        self.opts.export_path.exists()
    }
}

struct BankCsvCollector {
    opts: bank_csv::BankCsvOpts,
}
impl BankCsvCollector {
    fn new() -> Self {
        Self {
            opts: bank_csv::BankCsvOpts::defaults(),
        }
    }
}
impl Collector for BankCsvCollector {
    fn name(&self) -> &'static str {
        "bank-csv"
    }
    fn collect(&self, journal: &Journal, identity: &Identity) -> anyhow::Result<usize> {
        bank_csv::collect(journal, identity, &self.opts)
    }
    fn is_available(&self) -> bool {
        self.opts.import_dir.exists()
    }
}

struct CalendarCollector {
    opts: calendar::CalendarOpts,
}
impl CalendarCollector {
    fn new() -> Self {
        Self {
            opts: calendar::CalendarOpts::defaults(),
        }
    }
}
impl Collector for CalendarCollector {
    fn name(&self) -> &'static str {
        "calendar"
    }
    fn collect(&self, journal: &Journal, identity: &Identity) -> anyhow::Result<usize> {
        calendar::collect(journal, identity, &self.opts)
    }
    fn is_available(&self) -> bool {
        self.opts.import_dir.exists()
    }
}
