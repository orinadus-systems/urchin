# Privacy

## What Urchin stores

Urchin writes events to a local JSONL file at `~/.local/share/urchin/journal.jsonl`. Each event contains:

- A timestamp
- The source tool name (e.g. `claude`, `shell`, `bank-csv`)
- A kind label (e.g. `conversation`, `command`, `purchase`)
- The content string from the source
- Optional structured metadata (amounts, coordinates, health values)

No raw file contents, screenshots, or full document text are captured. Connectors read semantic signals: prompts, commands, commit messages, transaction amounts, step counts.

## What Urchin does not transmit

By default, nothing leaves your machine:

- The journal is a local file. No background upload.
- The HTTP intake binds to `127.0.0.1:18799` only. Not reachable from other machines.
- Cloud sync (`urchin sync`) requires explicit configuration of `URCHIN_SYNC_TOKEN` and is a manual command, not a background process.

## Ephemeral mode

When ephemeral mode is active, no events are written to the journal. The daemon accepts ingest requests but drops them silently.

Start and stop:
```
POST 127.0.0.1:18799/ingest  {"action":"start"}  (via urchin_ephemeral MCP tool)
```

Or from the CLI:
```bash
urchin ingest --content "" --kind other  # not yet: native ephemeral CLI coming
```

Ephemeral state is a file flag at `~/.local/share/urchin/ephemeral.lock`. Remove the file or call `end` to resume writing.

## Deleting data

The journal is a plain text file. Delete it to remove all stored events:

```bash
rm ~/.local/share/urchin/journal.jsonl
rm ~/.local/share/urchin/index.db
```

Checkpoint files (in `~/.local/state/urchin/`) track read positions per connector. Deleting them causes connectors to re-read from the start of each source on the next run.

## Import files

Personal data import files (Apple Health, Google Takeout, bank CSVs, calendar ICS files) are read from `~/.local/share/urchin/imports/` and never modified. You can delete them after import.

## The `.urchinignore` protocol

You can define collection boundaries at `~/.urchinignore`:

```
# Never collect from these paths
ignore: .env*
ignore: *.pem
ignore: ~/.ssh/

# Blind the shell collector to specific commands
ignore_command: pass
ignore_command: gpg
```

The spec is in `SOVEREIGNTY.md`. Runtime enforcement is planned.
