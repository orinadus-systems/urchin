use anyhow::Result;
use urchin_core::event::{Event, EventKind};
use crate::client::UrchinClient;

/// Fluent builder for Urchin events.
/// Obtain one via `UrchinClient::builder()`, set context fields, then call a terminal kind method.
///
/// ```ignore
/// let id = client.builder()
///     .source("claude")
///     .workspace("/home/me/dev/urchin")
///     .decision("ship path-b before path-c")
///     .await?;
/// ```
pub struct EventBuilder<'a> {
    client:    &'a UrchinClient,
    source:    String,
    brain:     Option<String>,
    workspace: Option<String>,
    session:   Option<String>,
    title:     Option<String>,
    tags:      Vec<String>,
}

impl<'a> EventBuilder<'a> {
    pub fn new(client: &'a UrchinClient) -> Self {
        Self {
            client,
            source:    "sdk".to_string(),
            brain:     None,
            workspace: None,
            session:   None,
            title:     None,
            tags:      vec![],
        }
    }

    pub fn source(mut self, s: impl Into<String>) -> Self {
        self.source = s.into();
        self
    }

    pub fn brain(mut self, b: impl Into<String>) -> Self {
        self.brain = Some(b.into());
        self
    }

    pub fn workspace(mut self, w: impl Into<String>) -> Self {
        self.workspace = Some(w.into());
        self
    }

    pub fn session(mut self, s: impl Into<String>) -> Self {
        self.session = Some(s.into());
        self
    }

    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = Some(t.into());
        self
    }

    pub fn tags(mut self, t: Vec<String>) -> Self {
        self.tags = t;
        self
    }

    // Terminal methods — one per EventKind.

    pub async fn conversation(self, content: impl Into<String>) -> Result<String> {
        self.send(EventKind::Conversation, content.into()).await
    }

    pub async fn agent(self, content: impl Into<String>) -> Result<String> {
        self.send(EventKind::Agent, content.into()).await
    }

    pub async fn command(self, content: impl Into<String>) -> Result<String> {
        self.send(EventKind::Command, content.into()).await
    }

    pub async fn commit(self, content: impl Into<String>) -> Result<String> {
        self.send(EventKind::Commit, content.into()).await
    }

    pub async fn file(self, content: impl Into<String>) -> Result<String> {
        self.send(EventKind::File, content.into()).await
    }

    pub async fn decision(self, content: impl Into<String>) -> Result<String> {
        self.send(EventKind::Decision, content.into()).await
    }

    async fn send(self, kind: EventKind, content: String) -> Result<String> {
        anyhow::ensure!(!content.trim().is_empty(), "event content must not be empty");

        let client = self.client;   // &'a UrchinClient is Copy
        let mut event = Event::new(self.source, kind, content);
        event.brain     = self.brain;
        event.workspace = self.workspace;
        event.session   = self.session;
        event.title     = self.title;
        event.tags      = self.tags;

        client.ingest(&event).await
    }
}
