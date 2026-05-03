use chrono::{Duration, Utc};
use crate::event::Event;

pub fn recent<'a>(events: &'a [Event], hours: f64, source: Option<&str>, limit: usize) -> Vec<&'a Event> {
    let cutoff = Utc::now() - Duration::milliseconds((hours * 3_600_000.0) as i64);
    let mut filtered: Vec<&Event> = events
        .iter()
        .filter(|e| e.timestamp >= cutoff)
        .filter(|e| source.map(|s| e.source == s).unwrap_or(true))
        .collect();
    filtered.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    filtered.truncate(limit);
    filtered
}

pub fn search_content<'a>(events: &'a [Event], query: &str, hours: f64, limit: usize) -> Vec<&'a Event> {
    let q = query.to_lowercase();
    let cutoff = Utc::now() - Duration::milliseconds((hours * 3_600_000.0) as i64);
    let mut filtered: Vec<&Event> = events
        .iter()
        .filter(|e| e.timestamp >= cutoff)
        .filter(|e| e.content.to_lowercase().contains(&q))
        .collect();
    filtered.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    filtered.truncate(limit);
    filtered
}

pub fn project_context<'a>(events: &'a [Event], project: &str, hours: f64, limit: usize) -> Vec<&'a Event> {
    let p = project.to_lowercase();
    let cutoff = Utc::now() - Duration::milliseconds((hours * 3_600_000.0) as i64);
    let mut filtered: Vec<&Event> = events
        .iter()
        .filter(|e| e.timestamp >= cutoff)
        .filter(|e| {
            e.content.to_lowercase().contains(&p)
                || e.tags.iter().any(|t| t.to_lowercase().contains(&p))
                || e.workspace.as_deref().map(|w| w.to_lowercase().contains(&p)).unwrap_or(false)
        })
        .collect();
    filtered.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    filtered.truncate(limit);
    filtered
}

/// Events whose workspace starts with `path` (case-insensitive prefix match).
pub fn workspace_context<'a>(events: &'a [Event], path: &str, hours: f64, limit: usize) -> Vec<&'a Event> {
    let p = path.to_lowercase();
    let cutoff = Utc::now() - Duration::milliseconds((hours * 3_600_000.0) as i64);
    let mut filtered: Vec<&Event> = events
        .iter()
        .filter(|e| e.timestamp >= cutoff)
        .filter(|e| {
            e.workspace
                .as_deref()
                .map(|w| w.to_lowercase().starts_with(&p) || w.to_lowercase().contains(&p))
                .unwrap_or(false)
        })
        .collect();
    filtered.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    filtered.truncate(limit);
    filtered
}

pub fn format_events(events: &[&Event]) -> String {
    if events.is_empty() {
        return "(no matching events)".to_string();
    }
    events
        .iter()
        .map(|e| {
            let ts = e.timestamp.format("%Y-%m-%dT%H:%M:%SZ");
            let brain = e.brain.as_deref().unwrap_or("-");
            format!("[{}] {} ({}) — {}", ts, e.source, brain, truncate(&e.content, 120))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate(s: &str, n: usize) -> String {
    let collapsed: String = s.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();
    if collapsed.chars().count() <= n {
        collapsed
    } else {
        let mut out: String = collapsed.chars().take(n).collect();
        out.push('…');
        out
    }
}
