use anyhow::{Context, Result};
use urchin_core::event::Event;

/// HTTP client for the local Urchin daemon.
pub struct UrchinClient {
    base_url: String,
    http: reqwest::Client,
}

impl UrchinClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::new(),
        }
    }

    /// Connect to the default local daemon on port 18799.
    pub fn local() -> Self {
        Self::new("http://127.0.0.1:18799")
    }

    /// POST an event to the daemon's /ingest endpoint.
    /// Returns the recorded event id on success.
    pub async fn ingest(&self, event: &Event) -> Result<String> {
        let url = format!("{}/ingest", self.base_url);
        let resp = self.http
            .post(&url)
            .json(event)
            .send()
            .await
            .context("failed to reach Urchin daemon")?;

        let status = resp.status();
        let body: serde_json::Value = resp.json().await
            .context("non-JSON response from daemon")?;

        if !status.is_success() {
            anyhow::bail!("daemon returned {}: {:?}", status, body);
        }

        Ok(body["id"].as_str().unwrap_or("ok").to_string())
    }

    /// Return a fluent builder pre-wired to this client.
    pub fn builder(&self) -> crate::builder::EventBuilder<'_> {
        crate::builder::EventBuilder::new(self)
    }
}
