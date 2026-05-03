# Changelog

All notable changes to Urchin are documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
Versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.3.0] — 2025-07-05

### Added

**MCP hardening for daily Cursor/Zed use**
- Committed `.cursor/mcp.json` — drop this in any repo and Cursor picks up the 9-tool server automatically
- README IDE Setup section: config blocks for Cursor, Zed, VS Code
- MCP server test renamed to `tools_list_returns_nine_tools` to stay in sync

**`urchin-agent` skeleton crate** (`crates/urchin-agent/`)
- `Agent` struct + `AgentConfig` builder (`with_hours`, `with_limit`)
- `context::load()` — time-window + count filter over journal events
- `context::format_context()` — renders structured context block
- `reflect::synthesise()` — deterministic text pass (Phase 2 ReAct; Phase 4 slot reserved for LLM backend)
- `reflect::to_event()` — wraps reflection as `EventKind::Agent` journal event
- `run()` full loop: load → reflect → append back to journal
- 11 tests

**`urchin agent reflect` CLI subcommand**
- `urchin agent reflect "<goal>" --hours <f> --limit <n>`
- Dispatches through `agent_cmd()` in `urchin-cli`

**`urchin_agent_reflect` as 9th MCP tool**
- Exposed via MCP stdio: `{"goal": "...", "hours": 24, "limit": 30}`
- Writes the reflection back as an `Agent` event in the journal
- 17 MCP tests total (was 16)

### Test counts

| Crate              | Tests |
|--------------------|-------|
| `urchin-core`      | 7     |
| `urchin-intake`    | 2     |
| `urchin-mcp`       | 17    |
| `urchin-collectors`| 52    |
| `urchin-vault`     | 3     |
| `urchin-agent`     | 11    |
| **Total**          | **92**|

---

## [0.2.0] — 2026-07-04

### Added

**Collector trait + registry**
- Object-safe `Collector` trait: `name()`, `collect()`, `is_available()`
- `CollectorRegistry::with_defaults(repo_roots)` — wires all 8 collectors, skips unavailable
- `run_all()` returns per-collector results with name + count/error
- Adding a new collector = one `impl Collector` struct; no changes to daemon or dispatch

**Codex collector**
- Source: `~/.codex/state_5.sqlite` (`threads` table)
- Captures `first_user_message` as user intent (falls back to `title`)
- Skips archived sessions and slash-command titles (`/clear`, etc.)
- Checkpoint: JSON `{ last_ts_ms }` watermark

**OpenCode collector**
- Source: `~/.local/share/opencode/opencode.db` (`message` JOIN `session`)
- Filters for `role=user` messages only
- Extracts text from three content formats: `parts[].text`, `content` string, `content[].text` blocks
- Checkpoint: JSON `{ last_ts_ms }` watermark

**Local model collector**
- Source: `~/.local/share/urchin/local-model.jsonl` (opt-in drop file)
- Any local inference harness (Ollama, LM Studio, llama.cpp) can append JSONL records
- Fields: `prompt` (required), `model` (optional tag), `ts` (RFC3339, optional), `workspace` (optional)
- Checkpoint: byte-offset (same mechanism as shell collector)
- `is_available()` returns false when drop file is absent — zero noise

**CLI subcommands**
- `urchin collect codex` — run Codex collector
- `urchin collect opencode` — run OpenCode collector
- `urchin collect local-model` — run local model collector
- All three included in `urchin collect all` via registry

**Documentation**
- `ROADMAP.md` — architectural contract encoding Context OS phases 0–6
- `SOVEREIGNTY.md` — four sovereignty mandates as binding spec
- `.urchinignore.example` — sensible defaults (secrets, .env, gpg, pass)
- `README.md` updated: 74 tests, 8 collectors, new commands, local model format

### Changed

- Version bumped `0.1.0` → `0.2.0` across all 7 crates
- `urchin-collectors/Cargo.toml` gained `rusqlite = { version = "0.31", features = ["bundled"] }` — no system libsqlite3 required

### Test count

| Crate | Tests |
|---|---|
| urchin-core | 7 |
| urchin-intake | 2 |
| urchin-mcp | 10 |
| urchin-collectors | 52 |
| urchin-vault | 3 |
| **Total** | **74** |

---

## [0.1.0-alpha] — 2026-05-03

### Added

- `urchin-core`: `Event`, `Journal`, `Identity`, `Config`, `EventKind`, `Actor`
- `urchin-intake`: `POST /ingest`, `GET /health`, binds to `127.0.0.1:18799`
- `urchin-mcp`: MCP over stdio (JSON-RPC 2.0), 5 tools
  - `urchin_status`, `urchin_ingest`, `urchin_recent_activity`, `urchin_project_context`, `urchin_search`
- `urchin-collectors`: shell, git, claude, copilot, gemini (5 live collectors)
- `urchin-vault`: vault projection into `~/brain/daily/YYYY-MM-DD.md` inside marker guards
- `urchin-sdk`: HTTP client for daemon + cloud hub
- `urchin-cli`: single binary `urchin` with doctor, ingest, serve, mcp, collect, recent, query, vault, sync
- Daemon mode (`urchin serve`): tokio runtime, collector tick loop
- Cloud sync: `urchin sync` → `orinadus.com/api/urchin-sync`
- Systemd user service
