//! urchin-agent: ReAct scaffold for the Urchin substrate.
//!
//! An agent loads recent context from the journal, runs a reasoning
//! pass over it via a pluggable `Reasoner` backend, and writes its output
//! back as an `EventKind::Agent` event. Nothing leaves the machine;
//! the journal is the execution log.
//!
//! Reasoner selection at runtime:
//! - `URCHIN_REASONER_URL` set → `HttpReasoner` (Ollama-compat endpoint)
//! - not set → `EchoReasoner` (deterministic, no network, default for tests)

pub mod context;
pub mod reasoner;
pub mod reflect;
pub mod semantic;

use anyhow::Result;
use urchin_core::{config::Config, identity::Identity, journal::Journal};

use reasoner::{EchoReasoner, HttpReasoner, Reasoner};

/// Configuration for a single agent run.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// The goal or instruction the agent is asked to reason about.
    pub goal: String,
    /// How far back (hours) to pull context events.
    pub context_hours: f64,
    /// Max events to include in the context window.
    pub context_limit: usize,
    /// Source tag to attach to emitted events. Defaults to "urchin-agent".
    pub source: String,
}

impl AgentConfig {
    pub fn new(goal: impl Into<String>) -> Self {
        Self {
            goal: goal.into(),
            context_hours: 24.0,
            context_limit: 30,
            source: "urchin-agent".to_string(),
        }
    }

    pub fn with_hours(mut self, h: f64) -> Self {
        self.context_hours = h;
        self
    }

    pub fn with_limit(mut self, n: usize) -> Self {
        self.context_limit = n;
        self
    }
}

/// A single agent run: load context → reason → write.
pub struct Agent {
    journal:  Journal,
    identity: Identity,
    reasoner: Box<dyn Reasoner>,
}

impl Agent {
    pub fn new(cfg: Config) -> Self {
        let reasoner: Box<dyn Reasoner> = match HttpReasoner::from_env() {
            Some(r) => {
                tracing::info!("using HttpReasoner");
                Box::new(r)
            }
            None => Box::new(EchoReasoner),
        };
        Self {
            journal:  Journal::new(cfg.journal_path.clone()),
            identity: Identity::resolve(),
            reasoner,
        }
    }

    /// Run the reflect loop:
    /// 1. Load recent events from the journal as context.
    /// 2. Call the `Reasoner` backend to produce a reflection.
    /// 3. Write the reflection back as an `Agent` event.
    ///
    /// Returns the reflection text.
    pub fn run(&self, agent_cfg: &AgentConfig) -> Result<String> {
        let events = self.journal.read_all()?;
        let ctx = context::load(&events, agent_cfg.context_hours, agent_cfg.context_limit);

        tracing::debug!(
            goal = %agent_cfg.goal,
            context_events = ctx.len(),
            "agent run starting"
        );

        let reflection = reflect::synthesise(&agent_cfg.goal, &ctx, self.reasoner.as_ref());

        let event = reflect::to_event(
            &reflection,
            &agent_cfg.goal,
            &agent_cfg.source,
            &self.identity,
        );
        self.journal.append(&event)?;
        self.journal.flush()?;

        tracing::info!(
            source = %agent_cfg.source,
            chars = reflection.len(),
            "agent reflection written"
        );

        Ok(reflection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn test_agent() -> (Agent, NamedTempFile) {
        let tmp = NamedTempFile::new().unwrap();
        let mut cfg = Config::default();
        cfg.journal_path = tmp.path().to_path_buf();
        (Agent::new(cfg), tmp)
    }

    #[test]
    fn run_on_empty_journal_produces_reflection_event() {
        let (agent, _tmp) = test_agent();
        let cfg = AgentConfig::new("What did we build recently?");
        let out = agent.run(&cfg).unwrap();
        assert!(!out.is_empty());

        let events = agent.journal.read_all().unwrap();
        assert_eq!(events.len(), 1);
        let ev = &events[0];
        assert_eq!(ev.source, "urchin-agent");
        assert!(ev.tags.contains(&"agent-reflect".to_string()));
    }

    #[test]
    fn reflection_includes_goal_echo() {
        let (agent, _tmp) = test_agent();
        let goal = "Summarise recent terminal activity";
        let cfg = AgentConfig::new(goal);
        let out = agent.run(&cfg).unwrap();
        assert!(out.contains(goal));
    }

    #[test]
    fn context_window_is_respected() {
        let (agent, _tmp) = test_agent();
        let cfg = AgentConfig::new("first pass").with_limit(5);
        agent.run(&cfg).unwrap();

        let cfg2 = AgentConfig::new("second pass").with_limit(5);
        let out2 = agent.run(&cfg2).unwrap();
        assert!(!out2.is_empty());
        let events = agent.journal.read_all().unwrap();
        assert_eq!(events.len(), 2);
    }
}
