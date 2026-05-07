//! Cross-process ephemeral mode flag, backed by a lock file on disk.
//!
//! When active, journal writes are suppressed across all processes (MCP, intake).
//! MCP calls `activate()`/`deactivate()` via the urchin_ephemeral tool.
//! urchin-intake checks `is_active()` before writing each ingested event.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct EphemeralMode {
    flag_path: PathBuf,
}

impl EphemeralMode {
    pub fn new(data_dir: &Path) -> Self {
        Self { flag_path: data_dir.join("ephemeral.lock") }
    }

    /// Returns true if the flag file exists — ephemeral mode is on.
    pub fn is_active(&self) -> bool {
        self.flag_path.exists()
    }

    /// Create the flag file. Subsequent calls to `is_active()` return true.
    pub fn activate(&self) -> std::io::Result<()> {
        if let Some(p) = self.flag_path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(&self.flag_path, "1")
    }

    /// Remove the flag file. Subsequent calls to `is_active()` return false.
    /// No-op if the file does not exist.
    pub fn deactivate(&self) -> std::io::Result<()> {
        if self.flag_path.exists() {
            std::fs::remove_file(&self.flag_path)?;
        }
        Ok(())
    }
}

impl Default for EphemeralMode {
    fn default() -> Self {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("urchin");
        Self::new(&data_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn mode(dir: &TempDir) -> EphemeralMode {
        EphemeralMode::new(dir.path())
    }

    #[test]
    fn inactive_by_default() {
        let dir = TempDir::new().unwrap();
        assert!(!mode(&dir).is_active());
    }

    #[test]
    fn activate_deactivate_roundtrip() {
        let dir = TempDir::new().unwrap();
        let m = mode(&dir);
        m.activate().unwrap();
        assert!(m.is_active());
        m.deactivate().unwrap();
        assert!(!m.is_active());
    }

    #[test]
    fn deactivate_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let m = mode(&dir);
        m.deactivate().unwrap(); // no-op on missing file — must not error
        assert!(!m.is_active());
    }
}
