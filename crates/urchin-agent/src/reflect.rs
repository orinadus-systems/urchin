/// Reflection synthesis: deterministic pass over context.
///
/// Phase 2 skeleton: produces a structured text block summarising the context
/// and echoing the goal. In Phase 4 this module will accept an optional LLM
/// backend (via a `Reasoner` trait) and use it instead when available.

use chrono::Utc;
use urchin_core::event::{Event, EventKind};
use urchin_core::identity::Identity;

use crate::context::format_context;

/// Produce a structured reflection over `context` given a `goal`.
/// The result is always a non-empty UTF-8 string.
pub fn synthesise(goal: &str, context: &[&Event]) -> String {
    let block = format_context(context);
    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    format!(
        "=== Urchin Agent Reflection ===\n\
         Goal: {goal}\n\
         Timestamp: {ts}\n\
         Context events: {count}\n\
         ---\n\
         {block}\n\
         ---\n\
         (Phase 2: deterministic pass — LLM routing wired in Phase 4)",
        count = context.len(),
    )
}

/// Wrap a reflection string into a journal `Event` ready to append.
pub fn to_event(reflection: &str, goal: &str, source: &str, _identity: &Identity) -> Event {
    let goal_tag = format!("goal:{}", &goal[..goal.len().min(64)]);
    let mut ev = Event::new(source, EventKind::Agent, reflection);
    ev.tags = vec!["agent-reflect".to_string(), goal_tag];
    ev
}

#[cfg(test)]
mod tests {
    use super::*;
    use urchin_core::event::{Event, EventKind};

    fn dummy_event(content: &str) -> Event {
        Event::new("shell", EventKind::Command, content)
    }

    #[test]
    fn synthesise_echoes_goal() {
        let goal = "understand recent git activity";
        let out = synthesise(goal, &[]);
        assert!(out.contains(goal));
    }

    #[test]
    fn synthesise_includes_context_count() {
        let e = dummy_event("cargo test passed");
        let out = synthesise("check builds", &[&e]);
        assert!(out.contains("Context events: 1"));
    }

    #[test]
    fn to_event_has_agent_reflect_tag() {
        let id = Identity::resolve();
        let ev = to_event("some reflection", "my goal", "urchin-agent", &id);
        assert!(ev.tags.contains(&"agent-reflect".to_string()));
        assert_eq!(ev.source, "urchin-agent");
    }

    #[test]
    fn to_event_goal_tag_truncated() {
        let id = Identity::resolve();
        let long_goal = "a".repeat(100);
        let ev = to_event("reflection", &long_goal, "urchin-agent", &id);
        let goal_tag = ev.tags.iter().find(|t| t.starts_with("goal:")).unwrap();
        assert!(goal_tag.len() <= "goal:".len() + 64);
    }
}
