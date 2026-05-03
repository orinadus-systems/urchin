# Urchin dev loop

## Repo layout
```
crates/
  urchin-core/       — zero I/O, shared types and event model
  urchin-intake/     — axum HTTP POST /ingest, GET /health  (port 18799)
  urchin-cli/        — single binary: urchin
  urchin-mcp/        — MCP over stdio, 5 tools
  urchin-collectors/ — claude/shell/git wired; copilot/gemini stubbed
  urchin-vault/      — Obsidian projection (stub)
```

## Build

```bash
cd ~/dev/orinadus/substrate/urchin-rust
cargo build              # dev build
cargo build --release    # release build
```

## Running the daemon

The daemon is managed by systemd user service:

```bash
systemctl --user start urchin      # start
systemctl --user stop urchin       # stop
systemctl --user restart urchin    # restart (picks up new binary automatically)
journalctl --user -u urchin -f     # follow logs
```

After `cargo build`, run `systemctl --user restart urchin` to pick up the new binary.

## MCP

All three clients (Claude, VS Code, Copilot CLI) are configured to use:
```
/home/samhc/dev/orinadus/substrate/urchin-rust/target/debug/urchin mcp
```

MCP runs as a child process launched on-demand by each client. Rebuilding the binary is enough — clients restart it on their next call.

## Tests

```bash
cargo test                         # run all tests
cargo test -p urchin-core          # single crate
cargo test -- --nocapture          # show stdout
```

## Health check

```bash
urchin doctor
curl -s http://127.0.0.1:18799/health
```

## Config

`~/.config/urchin/config.toml` — runtime config. Cloud sync is disabled; local-only mode.

## Collector runs (manual)

```bash
urchin collect claude    # ingest new claude history entries
urchin collect shell     # ingest shell history delta
urchin collect git       # ingest recent git activity
```

Collectors are also wired into the daemon's auto-collection cycle when `serve` is running.
