/// Reasoner trait — pluggable LLM backend for the agent reflection loop.
///
/// Implementors receive the goal and formatted context, and return a synthesis.
/// The default in production is `HttpReasoner` when `URCHIN_REASONER_URL` is set;
/// otherwise `EchoReasoner` is used (deterministic, no network, always safe for tests).

use anyhow::Result;

/// A reasoning backend. Receives raw goal + context, returns a synthesis.
///
/// Implementations must be `Send + Sync` so the `Agent` can be used across threads.
pub trait Reasoner: Send + Sync {
    fn reason(&self, goal: &str, context: &str) -> Result<String>;
}

// ─── EchoReasoner ─────────────────────────────────────────────────────────────

/// Deterministic reasoner — echoes goal + context length. Used in tests and
/// when no LLM endpoint is configured.
pub struct EchoReasoner;

impl Reasoner for EchoReasoner {
    fn reason(&self, goal: &str, context: &str) -> Result<String> {
        Ok(format!(
            "Echo | goal: {goal} | context_len: {len}",
            len = context.len()
        ))
    }
}

// ─── HttpReasoner ─────────────────────────────────────────────────────────────

/// HTTP-based LLM reasoner. Posts to an Ollama-compatible `/api/generate` endpoint.
///
/// Configuration (read once at construction via `from_env()`):
/// - `URCHIN_REASONER_URL`   — e.g. `http://localhost:11434/api/generate`
/// - `URCHIN_REASONER_MODEL` — e.g. `llama3` (default: `"llama3"`)
///
/// Falls back to `EchoReasoner` if `URCHIN_REASONER_URL` is not set.
pub struct HttpReasoner {
    url:   String,
    model: String,
}

impl HttpReasoner {
    pub fn new(url: impl Into<String>, model: impl Into<String>) -> Self {
        Self { url: url.into(), model: model.into() }
    }

    /// Construct from environment variables. Returns `None` if `URCHIN_REASONER_URL`
    /// is not set — callers should fall back to `EchoReasoner`.
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("URCHIN_REASONER_URL").ok()?;
        let model = std::env::var("URCHIN_REASONER_MODEL")
            .unwrap_or_else(|_| "llama3".to_string());
        Some(Self::new(url, model))
    }

    fn build_prompt(goal: &str, context: &str) -> String {
        format!(
            "You are Urchin Agent, a context analysis system.\n\
             Analyse the following developer context and answer the goal concisely.\n\
             Focus on patterns, recent work, and actionable insights.\n\n\
             Goal: {goal}\n\n\
             Context (recent journal events):\n{context}\n\n\
             Reflection:"
        )
    }
}

impl Reasoner for HttpReasoner {
    fn reason(&self, goal: &str, context: &str) -> Result<String> {
        let prompt = Self::build_prompt(goal, context);

        let body = serde_json::json!({
            "model":  self.model,
            "prompt": prompt,
            "stream": false
        });

        let resp: serde_json::Value = ureq::post(&self.url)
            .set("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| anyhow::anyhow!("LLM request failed: {e}"))?
            .into_json()
            .map_err(|e| anyhow::anyhow!("LLM response parse failed: {e}"))?;

        resp["response"]
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("LLM returned empty response field"))
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_reasoner_includes_goal() {
        let r = EchoReasoner;
        let out = r.reason("understand recent errors", "some context").unwrap();
        assert!(out.contains("understand recent errors"));
    }

    #[test]
    fn echo_reasoner_includes_context_len() {
        let r = EchoReasoner;
        let ctx = "hello world";
        let out = r.reason("goal", ctx).unwrap();
        assert!(out.contains(&ctx.len().to_string()));
    }

    #[test]
    fn http_reasoner_from_env_returns_none_without_url() {
        // Remove env var if accidentally set in CI
        unsafe { std::env::remove_var("URCHIN_REASONER_URL") };
        assert!(HttpReasoner::from_env().is_none());
    }

    #[test]
    fn http_reasoner_build_prompt_contains_goal_and_context() {
        let prompt = HttpReasoner::build_prompt("fix auth bug", "recent: npm error");
        assert!(prompt.contains("fix auth bug"));
        assert!(prompt.contains("recent: npm error"));
        assert!(prompt.contains("Reflection:"));
    }
}
