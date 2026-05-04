/// Semantic search over the journal.
///
/// Two backends, selected at construction time:
///
/// 1. `TokenCosine` (default, always available) — tokenises query and candidate,
///    computes overlap-based cosine similarity. O(n) per event, zero network.
///
/// 2. `OllamaEmbed` — calls the Ollama `/api/embed` endpoint for real vector
///    embeddings, then ranks by cosine similarity. Activated when
///    `URCHIN_EMBEDDER_URL` is set in the environment; falls back to
///    `TokenCosine` on any network/parse failure.

use std::collections::HashSet;

use anyhow::Result;
use chrono::{Duration, Utc};
use urchin_core::event::Event;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single search result paired with its relevance score.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub event: Event,
    /// Relevance score in (0.0, 1.0]. Higher is more relevant.
    pub score: f32,
}

// ---------------------------------------------------------------------------
// Backend trait
// ---------------------------------------------------------------------------

pub trait EmbedBackend: Send + Sync {
    /// Compute a relevance score in [0, 1] between `query` and `candidate`.
    fn score(&self, query: &str, candidate: &str) -> f32;
    fn name(&self) -> &'static str;
}

// ---------------------------------------------------------------------------
// TokenCosine — no network, no deps
// ---------------------------------------------------------------------------

/// Tokenise-and-overlap cosine similarity.
///
/// score = |shared_tokens| / sqrt(|query_tokens| * |candidate_tokens|)
///
/// This is the cosine similarity of the binary term-presence vectors.
/// Effective for keyword and topic-level matching.
pub struct TokenCosine;

impl EmbedBackend for TokenCosine {
    fn score(&self, query: &str, candidate: &str) -> f32 {
        let qt = token_set(query);
        let ct = token_set(candidate);
        if qt.is_empty() || ct.is_empty() {
            return 0.0;
        }
        let shared = qt.intersection(&ct).count() as f32;
        shared / (qt.len() as f32).sqrt() / (ct.len() as f32).sqrt()
    }

    fn name(&self) -> &'static str {
        "token-cosine"
    }
}

fn token_set(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2)
        .map(|t| t.to_lowercase())
        .collect()
}

// ---------------------------------------------------------------------------
// OllamaEmbed — real vector embeddings via local Ollama server
// ---------------------------------------------------------------------------

/// Embedding backend backed by an Ollama-compatible `/api/embed` endpoint.
///
/// Env vars:
/// - `URCHIN_EMBEDDER_URL`   — required to activate (e.g. `http://localhost:11434`)
/// - `URCHIN_EMBEDDER_MODEL` — optional, defaults to `mxbai-embed-large`
pub struct OllamaEmbed {
    url:   String,
    model: String,
}

impl OllamaEmbed {
    /// Returns `Some` only when `URCHIN_EMBEDDER_URL` is set.
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("URCHIN_EMBEDDER_URL").ok()?;
        let model = std::env::var("URCHIN_EMBEDDER_MODEL")
            .unwrap_or_else(|_| "mxbai-embed-large".to_string());
        Some(Self { url, model })
    }

    fn embed(&self, text: &str) -> Option<Vec<f32>> {
        let payload = serde_json::json!({
            "model": self.model,
            "input": text
        });
        let resp = ureq::post(&format!("{}/api/embed", self.url))
            .send_json(&payload)
            .ok()?;
        let body: serde_json::Value = resp.into_json().ok()?;
        let embeddings = body.get("embeddings")?.as_array()?;
        let first = embeddings.first()?.as_array()?;
        Some(
            first
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect(),
        )
    }
}

impl EmbedBackend for OllamaEmbed {
    fn score(&self, query: &str, candidate: &str) -> f32 {
        match (self.embed(query), self.embed(candidate)) {
            (Some(q), Some(c)) => cosine_sim(&q, &c),
            // On any failure, fall back to token-cosine so the tool still works.
            _ => TokenCosine.score(query, candidate),
        }
    }

    fn name(&self) -> &'static str {
        "ollama-embed"
    }
}

fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

// ---------------------------------------------------------------------------
// SemanticSearch — public facade
// ---------------------------------------------------------------------------

pub struct SemanticSearch {
    backend: Box<dyn EmbedBackend>,
}

impl SemanticSearch {
    /// Construct with the best available backend:
    /// - `OllamaEmbed` if `URCHIN_EMBEDDER_URL` is set
    /// - `TokenCosine` otherwise
    pub fn new() -> Self {
        let backend: Box<dyn EmbedBackend> = match OllamaEmbed::from_env() {
            Some(b) => {
                tracing::debug!("semantic search: using OllamaEmbed backend");
                Box::new(b)
            }
            None => {
                tracing::debug!("semantic search: using TokenCosine backend");
                Box::new(TokenCosine)
            }
        };
        Self { backend }
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    /// Search `events` for relevance to `query`.
    ///
    /// Events older than `hours` are excluded before scoring.
    /// Results are sorted descending by score and capped at `limit`.
    pub fn search(
        &self,
        query: &str,
        events: &[Event],
        hours: f64,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let cutoff = Utc::now() - Duration::seconds((hours * 3600.0) as i64);

        let mut hits: Vec<SearchHit> = events
            .iter()
            .filter(|e| e.timestamp >= cutoff)
            .filter_map(|e| {
                let text = build_text(e);
                let score = self.backend.score(query, &text);
                if score > 0.0 {
                    Some(SearchHit {
                        event: e.clone(),
                        score,
                    })
                } else {
                    None
                }
            })
            .collect();

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit);
        Ok(hits)
    }
}

impl Default for SemanticSearch {
    fn default() -> Self {
        Self::new()
    }
}

/// Concatenate all searchable fields into a single text blob for scoring.
fn build_text(e: &Event) -> String {
    let mut parts: Vec<&str> = vec![e.content.as_str(), e.source.as_str()];
    if let Some(t) = &e.title {
        parts.push(t.as_str());
    }
    if let Some(w) = &e.workspace {
        parts.push(w.as_str());
    }
    for tag in &e.tags {
        parts.push(tag.as_str());
    }
    parts.join(" ")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use urchin_core::event::{Event, EventKind};

    fn event(source: &str, content: &str) -> Event {
        let mut e = Event::new(source, EventKind::Command, content);
        e.timestamp = Utc::now(); // fresh — within any hour window
        e
    }

    fn event_old(content: &str) -> Event {
        let mut e = Event::new("shell", EventKind::Command, content);
        e.timestamp = Utc::now() - chrono::Duration::hours(200);
        e
    }

    // ---- TokenCosine unit tests ----

    #[test]
    fn token_cosine_identical_texts_score_one() {
        let b = TokenCosine;
        let s = b.score("auth flow debugging", "auth flow debugging");
        assert!((s - 1.0).abs() < 1e-4, "expected ~1.0, got {s}");
    }

    #[test]
    fn token_cosine_disjoint_texts_score_zero() {
        let b = TokenCosine;
        let s = b.score("authentication jwt token", "solar panel energy");
        assert_eq!(s, 0.0);
    }

    #[test]
    fn token_cosine_partial_overlap_between_zero_and_one() {
        let b = TokenCosine;
        let s = b.score("auth flow", "auth module refactor");
        assert!(s > 0.0 && s < 1.0, "expected (0,1), got {s}");
    }

    #[test]
    fn token_cosine_empty_query_returns_zero() {
        let b = TokenCosine;
        assert_eq!(b.score("", "some content"), 0.0);
    }

    // ---- SemanticSearch integration tests ----

    #[test]
    fn search_finds_relevant_events() {
        let search = SemanticSearch::new();
        let events = vec![
            event("shell", "debugged the authentication flow"),
            event("git",   "feat: add solar panel metrics"),
            event("claude","reviewed the auth token logic"),
        ];
        let hits = search.search("authentication auth", &events, 24.0, 10).unwrap();
        assert!(!hits.is_empty(), "expected at least one hit");
        // auth-related events should score higher than solar panel
        let top = &hits[0].event.content;
        assert!(
            top.contains("auth"),
            "top result should be auth-related, got: {top}"
        );
    }

    #[test]
    fn search_excludes_old_events() {
        let search = SemanticSearch::new();
        let events = vec![
            event_old("authentication flow old event"),
            event("shell", "authentication flow recent event"),
        ];
        let hits = search.search("authentication", &events, 24.0, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].event.content.contains("recent"));
    }

    #[test]
    fn search_respects_limit() {
        let search = SemanticSearch::new();
        let events: Vec<Event> = (0..20)
            .map(|i| event("shell", &format!("auth event number {i}")))
            .collect();
        let hits = search.search("auth event", &events, 24.0, 5).unwrap();
        assert_eq!(hits.len(), 5);
    }

    #[test]
    fn search_empty_journal_returns_empty() {
        let search = SemanticSearch::new();
        let hits = search.search("anything", &[], 24.0, 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn search_scores_include_title_and_workspace() {
        let search = SemanticSearch::new();
        let mut e = event("copilot", "generic content");
        e.title     = Some("urchin substrate refactor".to_string());
        e.workspace = Some("/dev/orinadus/substrate/urchin-rust".to_string());
        let hits = search.search("urchin substrate", &[e], 24.0, 10).unwrap();
        assert!(!hits.is_empty(), "title/workspace should be scored");
    }

    #[test]
    fn results_are_sorted_descending_by_score() {
        let search = SemanticSearch::new();
        let events = vec![
            event("a", "auth flow authentication token jwt refresh"),
            event("b", "auth"),
            event("c", "auth authentication flow"),
        ];
        let hits = search.search("auth authentication flow token", &events, 24.0, 10).unwrap();
        let scores: Vec<f32> = hits.iter().map(|h| h.score).collect();
        for w in scores.windows(2) {
            assert!(w[0] >= w[1], "expected descending scores, got {w:?}");
        }
    }

    #[test]
    fn token_set_filters_short_tokens() {
        let ts = token_set("a bb ccc dddd");
        // "a" (len 1) should be excluded; "bb" is length 2 (included)
        assert!(!ts.contains("a"));
        assert!(ts.contains("bb"));
        assert!(ts.contains("ccc"));
    }

    #[test]
    fn cosine_sim_unit_vectors() {
        let a = vec![1.0_f32, 0.0, 0.0];
        let b = vec![1.0_f32, 0.0, 0.0];
        assert!((cosine_sim(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_sim_orthogonal_vectors() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![0.0_f32, 1.0];
        assert_eq!(cosine_sim(&a, &b), 0.0);
    }

    #[test]
    fn cosine_sim_mismatched_lengths_returns_zero() {
        let a = vec![1.0_f32, 2.0];
        let b = vec![1.0_f32];
        assert_eq!(cosine_sim(&a, &b), 0.0);
    }
}
