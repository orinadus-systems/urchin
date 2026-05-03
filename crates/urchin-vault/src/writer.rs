// urchin-vault: writer — atomic in-place block marker updates.
// Writes via tempfile + rename so partial writes never corrupt a vault file.
// Preserves all human content outside the sentinel markers.

use std::path::Path;
use anyhow::{Context, Result};

/// Upsert a machine-owned block in a vault file.
///
/// - If the file doesn't exist, creates it with the block wrapped by `open`/`close`.
/// - If the file exists but the markers are absent, appends the block.
/// - If the markers are present, replaces only the text between them.
/// - Writes atomically via tempfile in the same directory + rename.
pub fn upsert_block(path: &Path, open: &str, close: &str, content: &str) -> Result<()> {
    let dir = path.parent().context("vault file has no parent dir")?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("create_dir_all {}", dir.display()))?;

    let existing = if path.exists() {
        std::fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?
    } else {
        String::new()
    };

    let updated = splice_block(&existing, open, close, content);

    // Write to a sibling temp file then rename — atomic on same filesystem.
    let tmp = path.with_extension("urchin.tmp");
    std::fs::write(&tmp, &updated)
        .with_context(|| format!("write tmp {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;

    Ok(())
}

/// Pure function: splice `content` into `source` between `open` and `close`.
fn splice_block(source: &str, open: &str, close: &str, content: &str) -> String {
    if let (Some(o), Some(c)) = (source.find(open), source.find(close)) {
        if o < c {
            let after_open = o + open.len();
            let before = &source[..after_open];
            let after  = &source[c..];
            return format!("{}\n{}\n{}", before, content, after);
        }
    }

    // Markers absent — append (with leading newline if file is non-empty).
    if source.is_empty() {
        format!("{}\n{}\n{}\n", open, content, close)
    } else {
        let trimmed = source.trim_end_matches('\n');
        format!("{}\n\n{}\n{}\n{}\n", trimmed, open, content, close)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_when_absent() {
        let result = splice_block("# Existing\n", "<!-- O -->", "<!-- /O -->", "new");
        assert!(result.contains("<!-- O -->\nnew\n<!-- /O -->"));
        assert!(result.contains("# Existing\n"));
    }

    #[test]
    fn replace_existing_block() {
        let src = "before\n<!-- O -->\nold\n<!-- /O -->\nafter\n";
        let result = splice_block(src, "<!-- O -->", "<!-- /O -->", "new");
        assert!(result.contains("\nnew\n"));
        assert!(!result.contains("old"));
        assert!(result.contains("before\n"));
        assert!(result.contains("\nafter\n"));
    }

    #[test]
    fn creates_from_empty() {
        let result = splice_block("", "<!-- O -->", "<!-- /O -->", "body");
        assert_eq!(result, "<!-- O -->\nbody\n<!-- /O -->\n");
    }
}
