/// Context loading: select events from the journal that fit within the
/// agent's context window (hours + max count).

use chrono::{Duration, Utc};
use urchin_core::event::Event;

/// Pull events from the last `hours` worth of history, capped at `limit`.
/// Returns events in ascending chronological order (oldest first).
pub fn load(events: &[Event], hours: f64, limit: usize) -> Vec<&Event> {
    let cutoff = Utc::now() - Duration::seconds((hours * 3600.0) as i64);
    let mut ctx: Vec<&Event> = events
        .iter()
        .filter(|e| e.timestamp >= cutoff)
        .collect();

    if ctx.len() > limit {
        let drop = ctx.len() - limit;
        ctx.drain(0..drop);
    }
    ctx
}

/// Format the context window into a block the synthesiser can reason over.
pub fn format_context(events: &[&Event]) -> String {
    if events.is_empty() {
        return "No recent events in the journal.".to_string();
    }
    let mut lines = Vec::with_capacity(events.len());
    for ev in events {
        let ts = ev.timestamp.format("%Y-%m-%dT%H:%M:%SZ");
        lines.push(format!("[{}] {} — {}", ts, ev.source, ev.content));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use urchin_core::event::{Event, EventKind};

    fn make_event(source: &str, content: &str, offset_secs: i64) -> Event {
        let mut ev = Event::new(source, EventKind::Command, content);
        ev.timestamp = Utc::now() + Duration::seconds(offset_secs);
        ev
    }

    #[test]
    fn empty_events_returns_empty() {
        let ctx = load(&[], 24.0, 10);
        assert!(ctx.is_empty());
    }

    #[test]
    fn old_events_are_excluded() {
        let old = make_event("shell", "ancient command", -48 * 3600);
        let recent = make_event("shell", "recent command", -1);
        let events = vec![old, recent];
        let ctx = load(&events, 24.0, 10);
        assert_eq!(ctx.len(), 1);
        assert_eq!(ctx[0].content, "recent command");
    }

    #[test]
    fn limit_trims_oldest() {
        let e1 = make_event("shell", "first", -100);
        let e2 = make_event("shell", "second", -50);
        let e3 = make_event("shell", "third", -10);
        let events = vec![e1, e2, e3];
        let ctx = load(&events, 24.0, 2);
        assert_eq!(ctx.len(), 2);
        assert_eq!(ctx[0].content, "second");
        assert_eq!(ctx[1].content, "third");
    }

    #[test]
    fn format_is_readable() {
        let e = make_event("git", "feat: initial commit", -60);
        let evs = vec![&e];
        let block = format_context(&evs);
        assert!(block.contains("git"));
        assert!(block.contains("feat: initial commit"));
    }
}
