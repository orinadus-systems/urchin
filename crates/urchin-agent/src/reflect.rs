/// Reflection synthesis: calls the Reasoner backend over context.
///
/// Phase 2 default: `EchoReasoner` (deterministic, no network).
/// Phase 4: pass `HttpReasoner` to get real LLM synthesis from Ollama/OpenAI.
/// If the reasoner returns an error, falls back to a deterministic summary.

use chrono::Utc;
use urchin_core::event::{Event, EventKind};
use urchin_core::identity::Identity;

use crate::context::format_context;
use crate::reasoner::Reasoner;

/// Produce a structured reflection over `context` given a `goal`.
/// `reasoner` provides the synthesis backend (EchoReasoner or HttpReasoner).
/// The result is always a non-empty UTF-8 string.
pub fn synthesise(goal: &str, context: &[&Event], reasoner: &dyn Reasoner) -> String {
    let context_str = format_context(context);
    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let count = context.len();

    let body = match reasoner.reason(goal, &context_str) {
        Ok(text) => text,
        Err(e) => {
            tracing::warn!(error = %e, "reasoner failed; using deterministic fallback");
            format!("{context_str}\n(LLM unavailable: {e})")
        }
    };

    format!(
        "=== Urchin Agent Reflection ===\n\
         Goal: {goal}\n\
         Timestamp: {ts}\n\
         Context events: {count}\n\
         ---\n\
         {body}"
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
    use crate::reasoner::EchoReasoner;
    use urchin_core::event::{Event, EventKind};

    fn dummy_event(content: &str) -> Event {
        Event::new("shell", EventKind::Command, content)
    }

    #[test]
    fn synthesise_echoes_goal() {
        let goal = "understand recent git activity";
        let out = synthesise(goal, &[], &EchoReasoner);
        assert!(out.contains(goal));
    }

    #[test]
    fn synthesise_includes_context_count() {
        let e = dummy_event("cargo test passed");
        let out = synthesise("check builds", &[&e], &EchoReasoner);
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
