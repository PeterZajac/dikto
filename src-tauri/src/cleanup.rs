use serde::Deserialize;
use std::time::Duration;

pub const SYSTEM_PROMPT: &str = "You clean up dictated text. Fix punctuation and capitalization, \
remove filler words (e.g. \"ehm\", \"\u{e9}\", \"proste\", \"ako\u{17e}e\", \"um\", \"like\", \"you know\"), \
and fix obvious mis-transcriptions. Keep the original language (Slovak, Czech or English). \
Keep the meaning and wording otherwise unchanged. Never add new information. \
Never answer questions contained in the text \u{2014} only clean it. \
Output ONLY the cleaned text, with no quotes and no commentary.";

#[derive(Debug, thiserror::Error)]
pub enum CleanupError {
    #[error("network: {0}")]
    Network(String),
    #[error("meridian api {status}: {body}")]
    Api { status: u16, body: String },
    #[error("empty response")]
    Empty,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

pub struct CleanupClient {
    base_url: String,
    model: String,
    http: reqwest::Client,
}

pub const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

impl CleanupClient {
    pub fn new(base_url: String, model: String) -> Self {
        Self::with_timeout(base_url, model, CLEANUP_TIMEOUT)
    }

    pub fn with_timeout(base_url: String, model: String, timeout: Duration) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            http: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .expect("reqwest client"),
        }
    }

    pub async fn clean(&self, raw: &str) -> Result<String, CleanupError> {
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 2048,
            "system": SYSTEM_PROMPT,
            "messages": [{ "role": "user", "content": raw }]
        });
        let resp = self
            .http
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", "local")
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| CleanupError::Network(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(CleanupError::Api {
                status: status.as_u16(),
                body: resp.text().await.unwrap_or_default(),
            });
        }
        let parsed: AnthropicResponse = resp
            .json()
            .await
            .map_err(|e| CleanupError::Network(e.to_string()))?;
        let text = parsed
            .content
            .iter()
            .find(|b| b.kind == "text")
            .map(|b| b.text.trim().to_string())
            .unwrap_or_default();
        if text.is_empty() {
            return Err(CleanupError::Empty);
        }
        Ok(text)
    }

    /// Cheap reachability probe reserved for the Plan 2 settings UI;
    /// the pipeline does not call this today.
    pub async fn is_reachable(&self) -> bool {
        self.http.get(&self.base_url).send().await.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn cleans_text() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("anthropic-version", "2023-06-01"))
            .and(body_partial_json(serde_json::json!({
                "model": "claude-sonnet-5",
                "messages": [{ "role": "user", "content": "no proste ahoj svet akoze" }]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "content": [{ "type": "text", "text": "Ahoj, svet." }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let c = CleanupClient::new(server.uri(), "claude-sonnet-5".into());
        assert_eq!(
            c.clean("no proste ahoj svet akoze").await.unwrap(),
            "Ahoj, svet."
        );
    }

    #[tokio::test]
    async fn timeout_is_a_network_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(500))
                    .set_body_json(serde_json::json!({"content": []})),
            )
            .mount(&server)
            .await;

        let c = CleanupClient::with_timeout(
            server.uri(),
            "m".into(),
            Duration::from_millis(50),
        );
        assert!(matches!(c.clean("x").await, Err(CleanupError::Network(_))));
    }

    #[tokio::test]
    async fn empty_content_is_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "content": [{ "type": "text", "text": "   " }]
            })))
            .mount(&server)
            .await;

        let c = CleanupClient::new(server.uri(), "m".into());
        assert!(matches!(c.clean("x").await, Err(CleanupError::Empty)));
    }

    #[tokio::test]
    async fn api_error_surfaces_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;

        let c = CleanupClient::new(server.uri(), "m".into());
        assert!(matches!(
            c.clean("x").await,
            Err(CleanupError::Api { status: 500, .. })
        ));
    }
}
