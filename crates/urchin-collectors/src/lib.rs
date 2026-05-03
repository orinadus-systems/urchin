/// urchin-collectors: one module per source.
/// Each collector reads from a tool's native output and produces Events.
/// Collectors are passive — they read, they never write to source tools.
///
/// `run_all` is the single entry-point used by both the CLI `collect all` command
/// and the daemon tick loop. Adding a new collector means: implement the module,
/// register one arm here.

use std::path::PathBuf;
use std::sync::Arc;

use urchin_core::{identity::Identity, journal::Journal};

pub mod state;

pub mod claude;
pub mod copilot;
pub mod gemini;
pub mod shell;
pub mod git;
pub mod agent_bridge;

/// One result entry from `run_all`.
pub struct CollectorResult {
    pub name:  &'static str,
    pub count: anyhow::Result<usize>,
}

/// Run every collector that has a default path wired up.
/// `repo_roots` drives the git collector; if empty it falls back to `URCHIN_REPO_ROOTS`.
pub fn run_all(
    journal: &Arc<Journal>,
    identity: &Arc<Identity>,
    repo_roots: &[PathBuf],
) -> Vec<CollectorResult> {
    let mut results = Vec::new();

    results.push(CollectorResult {
        name:  "shell",
        count: shell::collect(journal, identity, &shell::ShellOpts::defaults()),
    });

    for repo in repo_roots {
        results.push(CollectorResult {
            name:  "git",
            count: git::collect_repo(journal, identity, &git::GitOpts::defaults_for(repo.clone())),
        });
    }

    results.push(CollectorResult {
        name:  "claude",
        count: claude::collect(journal, identity, &claude::ClaudeOpts::defaults()),
    });

    results.push(CollectorResult {
        name:  "copilot",
        count: copilot::collect(journal, identity, &copilot::CopilotOpts::defaults()),
    });

    results.push(CollectorResult {
        name:  "gemini",
        count: gemini::collect(journal, identity, &gemini::GeminiOpts::defaults()),
    });

    results
}
