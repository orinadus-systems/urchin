/// Shared helpers for collectors that need to remember where they left off.
/// Checkpoint files live under `XDG_STATE_HOME/urchin/` (or `~/.local/state/urchin/`).

use std::path::PathBuf;

pub fn state_dir() -> PathBuf {
    if let Ok(p) = std::env::var("XDG_STATE_HOME") {
        return PathBuf::from(p).join("urchin");
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".local")
        .join("state")
        .join("urchin")
}
