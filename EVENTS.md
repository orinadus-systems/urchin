# Events

The `Event` is the canonical unit. Everything that flows through Urchin is an Event.

## Schema

```json
{
  "id":        "uuid-v4",
  "timestamp": "2024-01-15T10:00:00Z",
  "source":    "claude",
  "kind":      "conversation",
  "content":   "the payload text",
  "workspace": "/path/to/project",
  "session":   "session-id",
  "title":     "short description",
  "tags":      ["tag-a", "tag-b"],
  "actor":     { "account": "alice", "device": "laptop", "workspace": "/path" },
  "meta":      { "amount": 4.50, "currency": "USD", "merchant": "Blue Bottle" }
}
```

All fields except `id`, `timestamp`, `source`, `kind`, and `content` are optional and omitted from JSON when not set.

## EventKind

| Kind | Serialized as | Produced by |
|---|---|---|
| `Conversation` | `"conversation"` | claude, copilot, gemini, codex, opencode, local-model |
| `Agent` | `"agent"` | claude (tool-use sessions) |
| `Command` | `"command"` | shell |
| `Commit` | `"commit"` | git |
| `File` | `"file"` | file watchers (planned) |
| `Decision` | `"decision"` | MCP `urchin_remember` tool |
| `Purchase` | `"purchase"` | bank-csv |
| `Location` | `"location"` | google-takeout |
| `HealthMetric` | `"health_metric"` | apple-health |
| `CalendarEvent` | `"calendar_event"` | calendar |
| `SearchQuery` | `"search_query"` | google-takeout |
| `WatchHistory` | `"watch_history"` | google-takeout |
| `Other(s)` | `"other:<s>"` | fallback for unrecognized kinds |

## EventMeta

Structured optional fields for personal data kinds. Always omitted when not set.

| Field | Type | Used by |
|---|---|---|
| `amount` | `f64` | Purchase: transaction amount |
| `currency` | `string` | Purchase: e.g. `"USD"` |
| `merchant` | `string` | Purchase: payee name |
| `category` | `string` | Purchase, HealthMetric: category or metric type |
| `lat` | `f64` | Location: latitude in decimal degrees |
| `lng` | `f64` | Location: longitude in decimal degrees |
| `value` | `f64` | HealthMetric: numeric reading (steps, bpm, etc.) |
| `unit` | `string` | HealthMetric: unit string, e.g. `"count"`, `"bpm"` |
| `duration_secs` | `u64` | CalendarEvent, HealthMetric: duration in seconds |
| `attendees` | `u32` | CalendarEvent: number of attendees |

## Invariants

- The journal is append-only. Events are never mutated or deleted.
- Unknown fields in JSON are silently ignored on read. Old journal files stay readable after schema additions.
- `content` and `source` must be non-empty. The intake rejects events with blank values.
- Events are serialized as newline-delimited JSON (JSONL). One event per line.
- `id` is UUID v4 generated at creation time. Consumers can use it as a stable deduplication key.
