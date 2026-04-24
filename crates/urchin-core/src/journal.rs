/// Append-only journal. Events are written once, never mutated.
/// The journal file at ~/.local/share/urchin/journal/events.jsonl is the source of truth.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use anyhow::Result;
use crate::event::Event;

pub struct Journal {
    path: PathBuf,
}

pub struct JournalStats {
    pub event_count: usize,
    pub file_size_bytes: u64,
    pub last_event: Option<Event>,
}

impl Journal {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("urchin")
            .join("journal")
            .join("events.jsonl")
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn append(&self, event: &Event) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(event)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<Event>> {
        if !self.path.exists() {
            return Ok(vec![]);
        }
        let file = std::fs::File::open(&self.path)?;
        let reader = BufReader::new(file);
        let events = reader
            .lines()
            .filter_map(|l| l.ok())
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(&l).ok())
            .collect();
        Ok(events)
    }

    /// Fast stats without fully deserializing every event.
    pub fn stats(&self) -> Result<JournalStats> {
        if !self.path.exists() {
            return Ok(JournalStats {
                event_count: 0,
                file_size_bytes: 0,
                last_event: None,
            });
        }

        let file_size_bytes = std::fs::metadata(&self.path)?.len();
        let file = std::fs::File::open(&self.path)?;
        let reader = BufReader::new(file);

        let mut event_count = 0usize;
        let mut last_line = String::new();

        for line in reader.lines() {
            let line = line?;
            if !line.trim().is_empty() {
                event_count += 1;
                last_line = line;
            }
        }

        let last_event = if !last_line.is_empty() {
            serde_json::from_str(&last_line).ok()
        } else {
            None
        };

        Ok(JournalStats {
            event_count,
            file_size_bytes,
            last_event,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::EventKind;
    use tempfile::NamedTempFile;

    #[test]
    fn append_and_read_roundtrip() {
        let tmp = NamedTempFile::new().unwrap();
        let journal = Journal::new(tmp.path().to_path_buf());

        let e1 = Event::new("cli", EventKind::Conversation, "first");
        let e2 = Event::new("cli", EventKind::Agent, "second");

        journal.append(&e1).unwrap();
        journal.append(&e2).unwrap();

        let events = journal.read_all().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].content, "first");
        assert_eq!(events[1].content, "second");
    }

    #[test]
    fn stats_on_empty_journal() {
        let tmp = NamedTempFile::new().unwrap();
        // Write empty file
        std::fs::write(tmp.path(), "").unwrap();
        let journal = Journal::new(tmp.path().to_path_buf());
        let stats = journal.stats().unwrap();
        assert_eq!(stats.event_count, 0);
        assert!(stats.last_event.is_none());
    }

    #[test]
    fn stats_counts_correctly() {
        let tmp = NamedTempFile::new().unwrap();
        let journal = Journal::new(tmp.path().to_path_buf());

        for i in 0..5 {
            let e = Event::new("cli", EventKind::Conversation, format!("event {}", i));
            journal.append(&e).unwrap();
        }

        let stats = journal.stats().unwrap();
        assert_eq!(stats.event_count, 5);
        assert!(stats.last_event.is_some());
        assert_eq!(stats.last_event.unwrap().content, "event 4");
    }

    #[test]
    fn read_all_on_missing_file_returns_empty() {
        let journal = Journal::new(PathBuf::from("/nonexistent/path/events.jsonl"));
        let events = journal.read_all().unwrap();
        assert!(events.is_empty());
    }
}
