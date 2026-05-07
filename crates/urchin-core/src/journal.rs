//! Append-only journal. Events are written once, never mutated.
//! The journal file at ~/.local/share/urchin/journal/events.jsonl is the source of truth.

use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use anyhow::Result;
use crate::event::Event;

enum JournalOp {
    Append(String),
    Flush(std::sync::mpsc::SyncSender<()>),
}

pub struct Journal {
    path: PathBuf,
    tx: tokio::sync::mpsc::UnboundedSender<JournalOp>,
}

pub struct JournalStats {
    pub event_count: usize,
    pub file_size_bytes: u64,
    pub last_event: Option<Event>,
}

fn open_writer(path: &PathBuf) -> BufWriter<std::fs::File> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("journal: failed to open file for writing");
    BufWriter::new(f)
}

impl Journal {
    pub fn new(path: PathBuf) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<JournalOp>();
        let writer_path = path.clone();
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("journal: failed to build writer runtime")
                .block_on(async move {
                    let mut file: Option<BufWriter<std::fs::File>> = None;
                    while let Some(op) = rx.recv().await {
                        match op {
                            JournalOp::Append(line) => {
                                let f = file.get_or_insert_with(|| open_writer(&writer_path));
                                let _ = writeln!(f, "{}", line);
                            }
                            JournalOp::Flush(ack) => {
                                if let Some(f) = &mut file {
                                    let _ = f.flush();
                                }
                                let _ = ack.send(());
                            }
                        }
                    }
                });
        });
        Self { path, tx }
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

    /// Queue an event for writing. Returns immediately; the writer task flushes asynchronously.
    /// Call flush() to guarantee the event is on disk before reading back.
    pub fn append(&self, event: &Event) -> Result<()> {
        let line = serde_json::to_string(event)?;
        self.tx
            .send(JournalOp::Append(line))
            .map_err(|_| anyhow::anyhow!("journal writer has stopped"))
    }

    /// Block until all queued events are flushed to the OS buffer.
    pub fn flush(&self) -> Result<()> {
        let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel(0);
        self.tx
            .send(JournalOp::Flush(ack_tx))
            .map_err(|_| anyhow::anyhow!("journal writer has stopped"))?;
        ack_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("journal writer exited before flush ack"))
    }

    pub fn read_all(&self) -> Result<Vec<Event>> {
        if !self.path.exists() {
            return Ok(vec![]);
        }
        let file = std::fs::File::open(&self.path)?;
        let reader = BufReader::new(file);
        let events = reader
            .lines()
            .map_while(Result::ok)
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
                break;
            }
            pos = read_from;
        }
        let (events, _) = self.read_from_byte_offset(start)?;
        let skip = events.len().saturating_sub(n);
        Ok(events.into_iter().skip(skip).collect())
    }

    /// Read a window of events at newest-first positions [offset, offset+limit).
    /// Bounded read: only the last `offset + limit` events are scanned from EOF.
    pub fn read_window(&self, offset: usize, limit: usize) -> Result<Vec<Event>> {
        if limit == 0 {
            return Ok(vec![]);
        }
        let mut events = self.read_tail(offset + limit)?;
        events.reverse();
        Ok(events.into_iter().skip(offset).take(limit).collect())
    }

    /// Read events starting from a byte offset in the file.
    /// Returns (events, new_offset) where new_offset is the file position after reading.
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
        let mut event_count: usize = 0;
        let mut chunk = vec![0u8; 65_536];
        loop {
            let n = file.read(&mut chunk)?;
            if n == 0 { break; }
            for &b in &chunk[..n] {
                if b == b'\n' { event_count += 1; }
            }
        }
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
        journal.flush().unwrap();

        let events = journal.read_all().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].content, "first");
        assert_eq!(events[1].content, "second");
    }

    #[test]
    fn stats_on_empty_journal() {
        let tmp = NamedTempFile::new().unwrap();
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
        journal.flush().unwrap();

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
        journal.flush().unwrap();
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
        journal.flush().unwrap();
        let tail = journal.read_tail(10).unwrap();
        assert_eq!(tail.len(), 3);
    }

    #[test]
    fn read_window_first_page_is_newest_first() {
        let tmp = NamedTempFile::new().unwrap();
        let journal = Journal::new(tmp.path().to_path_buf());
        for i in 0..10 {
            journal.append(&Event::new("cli", EventKind::Conversation, format!("e{}", i))).unwrap();
        }
        journal.flush().unwrap();
        let window = journal.read_window(0, 3).unwrap();
        assert_eq!(window.len(), 3);
        assert_eq!(window[0].content, "e9");
        assert_eq!(window[1].content, "e8");
        assert_eq!(window[2].content, "e7");
    }

    #[test]
    fn read_window_offset_skips_newest() {
        let tmp = NamedTempFile::new().unwrap();
        let journal = Journal::new(tmp.path().to_path_buf());
        for i in 0..10 {
            journal.append(&Event::new("cli", EventKind::Conversation, format!("e{}", i))).unwrap();
        }
        journal.flush().unwrap();
        let window = journal.read_window(3, 3).unwrap();
        assert_eq!(window.len(), 3);
        assert_eq!(window[0].content, "e6");
        assert_eq!(window[1].content, "e5");
        assert_eq!(window[2].content, "e4");
    }

    #[test]
    fn read_window_past_end_returns_empty() {
        let tmp = NamedTempFile::new().unwrap();
        let journal = Journal::new(tmp.path().to_path_buf());
        for i in 0..3 {
            journal.append(&Event::new("cli", EventKind::Conversation, format!("e{}", i))).unwrap();
        }
        journal.flush().unwrap();
        let window = journal.read_window(10, 5).unwrap();
        assert!(window.is_empty());
    }

    #[test]
    fn concurrent_writers_all_land() {
        use std::sync::Arc;
        let tmp = NamedTempFile::new().unwrap();
        let journal = Arc::new(Journal::new(tmp.path().to_path_buf()));
        let handles: Vec<_> = (0..10).map(|i| {
            let j = journal.clone();
            std::thread::spawn(move || {
                for k in 0..1_000 {
                    j.append(&Event::new(
                        "test",
                        EventKind::Command,
                        format!("thread {} event {}", i, k),
                    ))
                    .unwrap();
                }
            })
        }).collect();
        for h in handles { h.join().unwrap(); }
        journal.flush().unwrap();
        let events = journal.read_all().unwrap();
        assert_eq!(events.len(), 10_000);
    }
}
