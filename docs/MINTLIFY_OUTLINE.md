# Mintlify Docs Outline (docs.orinadus.com)

This file tracks the planned documentation structure for docs.orinadus.com.
Rule: a page only gets written when the feature it documents passes tests in the repo.
No docs for unbuilt features.

---

## Navigation Structure

```
Getting Started
  Quick Start
  Installation
  Configuration

Collectors
  Overview
  Shell
  Git
  Claude
  Copilot
  Gemini
  Codex
  OpenCode
  Local Model (Drop File)
  Writing a Custom Collector

MCP Reference
  Overview and Setup
  IDE Setup (Cursor / Zed / VS Code)
  Tool: urchin_status
  Tool: urchin_ingest
  Tool: urchin_recent_activity
  Tool: urchin_project_context
  Tool: urchin_search
  Tool: urchin_workspace_context
  Tool: urchin_remember
  Tool: urchin_ephemeral
  Tool: urchin_agent_reflect
  Tool: urchin_semantic_search

API Reference
  POST /ingest
  GET /health
  Authentication

CLI Reference
  urchin doctor
  urchin serve
  urchin mcp
  urchin ingest
  urchin collect
  urchin recent
  urchin query
  urchin vault project
  urchin agent reflect
  urchin sync / pull
  urchin rebuild-index
  urchin config

Configuration
  config.toml Reference
  Environment Variables
  XDG Paths

Architecture
  Crate Map
  Process Topology
  Event Schema
  Journal Format (JSONL)
  SQLite Projection Index
  Checkpoint System

Sovereignty
  Ephemeral Mode
  Burn Button (MCP + CLI)
  .urchinignore (planned — Phase 5)
  Export (planned — Phase 5)

Roadmap
  Phase Status Overview
```

---

## Page Status

| Page | Status | Source |
|---|---|---|
| Quick Start | 🔲 needs writing | `README.md` quick start section |
| Installation | 🔲 needs writing | cargo build + binary download from releases |
| Configuration | 🔲 needs writing | `docs/API_REFERENCE.md`, config table in README |
| Collectors — Overview | 🔲 needs writing | `crates/urchin-collectors/src/lib.rs` + README |
| Collectors — Shell | 🔲 needs writing | `crates/urchin-collectors/src/shell.rs` |
| Collectors — Git | 🔲 needs writing | `crates/urchin-collectors/src/git.rs` |
| Collectors — Claude | 🔲 needs writing | `crates/urchin-collectors/src/claude.rs` |
| Collectors — Copilot | 🔲 needs writing | `crates/urchin-collectors/src/copilot.rs` |
| Collectors — Gemini | 🔲 needs writing | `crates/urchin-collectors/src/gemini.rs` |
| Collectors — Codex | 🔲 needs writing | `crates/urchin-collectors/src/codex.rs` |
| Collectors — OpenCode | 🔲 needs writing | `crates/urchin-collectors/src/opencode.rs` |
| Collectors — Local Model | 🔲 needs writing | `crates/urchin-collectors/src/local_model.rs` + README drop file section |
| MCP Overview + IDE Setup | 🔲 needs writing | README IDE Setup section |
| MCP — all 10 tools | 🔲 needs writing | `crates/urchin-mcp/src/tools.rs` schemas |
| API Reference | 🔲 needs writing | `docs/API_REFERENCE.md` (port directly) |
| CLI Reference | 🔲 needs writing | `crates/urchin-cli/src/main.rs` clap defs — includes `rebuild-index` |
| Architecture — SQLite Index | 🔲 needs writing | `crates/urchin-core/src/index.rs` — WAL, schema, rebuild |
| Architecture — other | 🔲 needs writing | `docs/ARCHITECTURE.md` (port directly) |
| Event Schema | 🔲 needs writing | `crates/urchin-core/src/event.rs` |
| Sovereignty — Ephemeral | 🔲 needs writing | `crates/urchin-core/src/ephemeral.rs` + `SOVEREIGNTY.md` |
| Roadmap | 🔲 needs writing | `ROADMAP.md` (port with phase status markers) |

---

## Writing Rule

Every page in the MCP Reference and Collector sections must include:
- What it does (one sentence)
- Required args + optional args (typed)
- Example request
- Example response
- Error cases

Every Collector page must include:
- Source path it reads from
- What it captures and how it normalizes to `Event`
- Checkpoint mechanism (how it avoids re-ingesting)
- `is_available()` condition (when is it a no-op)

---

## Docs Update Workflow

When a feature ships:
1. Close the tracking issue or PR that delivered the feature
2. Write or update the corresponding Mintlify page before the next release
3. Tag the GitHub Release only after docs are updated

No release ships without its docs page.
