# Connectors

Connectors are the read path. Each one reads a single data source, normalizes events, and appends them to the journal. Connectors never write back to source tools.

## The Collector trait

```rust
pub trait Collector: Send + Sync {
    fn name(&self) -> &'static str;
    fn collect(&self, journal: &Journal, identity: &Identity) -> anyhow::Result<usize>;
    fn is_available(&self) -> bool { true }
}
```

- `name()`: short slug used in logs and CLI output.
- `collect()`: read new events, append to journal, return count.
- `is_available()`: return `false` to skip silently when the source is absent. The daemon calls this before every run.

## Checkpoint pattern

Every connector tracks where it left off so it never re-emits events it has already seen. The pattern depends on the source format:

**Byte offset** (append-only files like `~/.bash_history`):
```rust
// Read checkpoint: last byte position
let start = read_checkpoint(&opts.checkpoint_path);
file.seek(SeekFrom::Start(start))?;
// ... read and emit events ...
// Write checkpoint: current file size
write_checkpoint(&opts.checkpoint_path, file_size)?;
```

**Content hash set** (JSON files without stable ordering, like Copilot history):
```rust
let mut seen: HashSet<String> = load_checkpoint(&opts.checkpoint_path);
for entry in entries {
    let key = hash(entry);
    if seen.contains(&key) { continue; }
    // ... emit event ...
    seen.insert(key);
}
save_checkpoint(&opts.checkpoint_path, &seen)?;
```

**Timestamp** (chronological sources like Apple Health XML):
```rust
let last_ts = read_checkpoint(&opts.checkpoint_path); // ISO 8601 string
for record in records {
    if record.timestamp <= last_ts { continue; }
    // ... emit event ...
    if record.timestamp > newest_ts { newest_ts = record.timestamp.clone(); }
}
write_checkpoint(&opts.checkpoint_path, &newest_ts)?;
```

**Per-file row count** (CSVs that may grow):
```rust
let rows_seen = checkpoint.get(filename).copied().unwrap_or(0);
for (i, row) in records.enumerate() {
    if i < rows_seen { continue; }
    // ... emit event ...
    count += 1;
}
checkpoint.insert(filename, rows_seen + count);
```

Checkpoint files live in `~/.local/state/urchin/` (XDG state dir). The `state_dir()` helper in `crates/urchin-collectors/src/state.rs` resolves this path.

## Built-in connectors

| Name | Source | Kind | Checkpoint |
|---|---|---|---|
| shell | `~/.bash_history` | Command | byte offset |
| git | any git repo | Commit | per-repo SHA |
| claude | `~/.claude/projects/**/*.jsonl` | Conversation / Agent | byte offset |
| copilot | `~/.copilot/command-history-state.json` | Conversation | content-hash set |
| gemini | `~/.gemini/tmp/*/chats/*.jsonl` | Conversation | partial-offset JSON |
| codex | `~/.codex/state_5.sqlite` | Conversation | watermark timestamp |
| opencode | `~/.local/share/opencode/opencode.db` | Conversation | watermark timestamp |
| local-model | `~/.local/share/urchin/local-model.jsonl` | Conversation | byte offset |
| google-takeout | `~/.local/share/urchin/imports/google-takeout/` | Location / SearchQuery / WatchHistory | timestamp + seen sets |
| apple-health | `~/.local/share/urchin/imports/apple-health/export.xml` | HealthMetric | timestamp |
| bank-csv | `~/.local/share/urchin/imports/bank/*.csv` | Purchase | per-file row count |
| calendar | `~/.local/share/urchin/imports/calendar/*.ics` | CalendarEvent | seen UID set |

## Adding a connector

1. Create `crates/urchin-collectors/src/<name>.rs` with a public `collect(journal, identity, opts)` function.
2. Define an `Opts` struct with a `defaults()` constructor that resolves paths from `dirs::home_dir()` or `state_dir()`.
3. Add the module to `lib.rs`:
   ```rust
   pub mod my_connector;
   ```
4. Create a struct implementing `Collector` and register it in `with_defaults()`:
   ```rust
   struct MyCollector { opts: my_connector::MyOpts }
   impl MyCollector { fn new() -> Self { Self { opts: my_connector::MyOpts::defaults() } } }
   impl Collector for MyCollector {
       fn name(&self) -> &'static str { "my-connector" }
       fn collect(&self, journal: &Journal, identity: &Identity) -> anyhow::Result<usize> {
           my_connector::collect(journal, identity, &self.opts)
       }
       fn is_available(&self) -> bool { self.opts.source_path.exists() }
   }
   // In with_defaults():
   r.register(MyCollector::new());
   ```
5. Add a CLI variant in `urchin-cli/src/main.rs` under `CollectKind`.
6. Write tests using `tempfile::TempDir` with a fixture data file.

## Import-based connectors

Google Takeout, Apple Health, bank CSV, and calendar connectors read from drop directories under `~/.local/share/urchin/imports/`. The user places export files there; the daemon picks them up on the next collection pass.

The daemon watches for new files using the `notify` crate (already in workspace deps), so in practice the collection is automatic once a file is dropped.
