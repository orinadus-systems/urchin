/// Append-only journal. Events are written once, never mutated.
/// The journal file at ~/.local/share/urchin/journal/events.jsonl is the source of truth.

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
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

    /// Read the last `n` events without loading the whole file.
    pub fn read_tail(&self, n: usize) -> Result<Vec<Event>> {
        if n == 0 || !self.path.exists() {
            return Ok(vec![]);
        }
        let mut file = std::fs::File::open(&self.path)?;
        let file_len = file.seek(SeekFrom::End(0))?;
        if file_len == 0 {
            return Ok(vec![]);
        }
        const CHUNK: u64 = 65_536;
        let mut newlines: usize = 0;
        let mut start: u64 = 0;
        let mut pos: u64 = file_len;
        'scan: loop {
            let read_from = pos.saturating_sub(CHUNK);
            let read_len = (pos - read_from) as usize;
            file.seek(SeekFrom::Start(read_from))?;
            let mut buf = vec![0u8; read_len];
            file.read_exact(&mut buf)?;
            for i in (0..read_len).rev() {
                if buf[i] == b'\n' {
                    newlines += 1;
                    if newlines > n {
                        start = read_from + i as u64 + 1;
                        break 'scan;
                    }
                }
            }
            if read_from == 0 {
                // start remains 0 (read from beginning of file)
                break;
            }
            pos = read_from;
        }
        let (events, _) = self.read_from_byte_offset(start)?;
        let skip = events.len().saturating_sub(n);
        Ok(events.into_iter().skip(skip).collect())
    }

    /// Read events starting from a byte offset in the file.
    /// Returns (events, new_offset) where new_offset is the file position after reading.
    /// Caller should persist new_offset so the next call skips already-read events.
    pub fn read_from_byte_offset(&self, offset: u64) -> Result<(Vec<Event>, u64)> {
        if !self.path.exists() {
            return Ok((vec![], 0));
        }
        let mut file = std::fs::File::open(&self.path)?;
        let file_len = file.seek(SeekFrom::End(0))?;
        if offset >= file_len {
            return Ok((vec![], file_len));
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        let events = buf
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        Ok((events, file_len))
    }

    /// Fast stats: raw byte scan for line count, tail seek for last event.
    pub fn stats(&self) -> Result<JournalStats> {
        if !self.path.exists() {
            return Ok(JournalStats { event_count: 0, file_size_bytes: 0, last_event: None });
        }
        let file_size_bytes = std::fs::metadata(&self.path)?.len();
        if file_size_bytes == 0 {
            return Ok(JournalStats { event_count: 0, file_size_bytes: 0, last_event: None });
        }
        let mut file = std::fs::File::open(&self.path)?;
        // Count newlines via raw byte scan — no per-line alloc, no JSON parse.
        let mut event_count: usize = 0;
        let mut chunk = vec![0u8; 65_536];
        loop {
            let n = file.read(&mut chunk)?;
            if n == 0 { break; }
            for &b in &chunk[..n] {
                if b == b'\n' { event_count += 1; }
            }
        }
        // Parse only the last ~4KB to extract the most recent event.
        let tail_start = file_size_bytes.saturating_sub(4096);
        file.seek(SeekFrom::Start(tail_start))?;
        let mut tail = String::new();
        file.read_to_string(&mut tail)?;
        let last_event = tail
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .and_then(|l| serde_json::from_str(l).ok());
        Ok(JournalStats { event_count, file_size_bytes, last_event })
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

    #[test]
    fn read_tail_returns_last_n() {
        let tmp = NamedTempFile::new().unwrap();
        let journal = Journal::new(tmp.path().to_path_buf());
        for i in 0..10 {
            journal.append(&Event::new("cli", EventKind::Conversation, format!("event {}", i))).unwrap();
        }
        let tail = journal.read_tail(3).unwrap();
        assert_eq!(tail.len(), 3);
        assert_eq!(tail[0].content, "event 7");
        assert_eq!(tail[1].content, "event 8");
        assert_eq!(tail[2].content, "event 9");
    }

    #[test]
    fn read_tail_fewer_events_than_n() {
        let tmp = NamedTempFile::new().unwrap();
        let journal = Journal::new(tmp.path().to_path_buf());
        for i in 0..3 {
            journal.append(&Event::new("cli", EventKind::Conversation, format!("e{}", i))).unwrap();
        }
        let tail = journal.read_tail(10).unwrap();
        assert_eq!(tail.len(), 3);
    }
}
