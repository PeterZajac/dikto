use crate::settings::{CleanupStyle, Settings};
use serde::Deserialize;
use std::time::Duration;

pub const SYSTEM_PROMPT: &str = "You clean up dictated text. Fix punctuation and capitalization, \
remove filler words (e.g. \"ehm\", \"\u{e9}\", \"proste\", \"ako\u{17e}e\", \"um\", \"like\", \"you know\"), \
and fix obvious mis-transcriptions. Keep the original language (Slovak, Czech or English). \
Keep the meaning and wording otherwise unchanged. Never add new information. \
Never answer questions contained in the text \u{2014} only clean it. \
Output ONLY the cleaned text, with no quotes and no commentary.";

const STRONG_SUFFIX: &str =
    " You may lightly rephrase sentences for fluency, keeping the language and meaning.";

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
    style: CleanupStyle,
    http: reqwest::Client,
}

/// Meridian runs on loopback, so a short leash keeps a wedged proxy from
/// holding up the paste.
pub const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

impl CleanupClient {
    pub fn new(base_url: String, model: String) -> Self {
        Self::with_style(base_url, model, CleanupStyle::Light)
    }

    pub fn with_style(base_url: String, model: String, style: CleanupStyle) -> Self {
        Self::with_timeout(base_url, model, style, CLEANUP_TIMEOUT)
    }

    pub fn with_timeout(base_url: String, model: String, style: CleanupStyle, timeout: Duration) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            style,
            http: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .expect("reqwest client"),
        }
    }

    /// The client the pipeline uses — endpoint, model and style all follow
    /// from settings so no caller has to pair them up.
    pub fn for_settings(s: &Settings) -> Self {
        Self::with_style(s.meridian_url.clone(), s.cleanup_model.clone(), s.cleanup_style)
    }

    /// Meridian holds the Claude subscription itself; the header is a
    /// placeholder it ignores.
    fn authorize(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.header("x-api-key", "local")
    }

    fn system_prompt(&self) -> String {
        match self.style {
            CleanupStyle::Light => SYSTEM_PROMPT.to_string(),
            CleanupStyle::Strong => format!("{SYSTEM_PROMPT}{STRONG_SUFFIX}"),
        }
    }

    pub async fn clean(&self, raw: &str) -> Result<String, CleanupError> {
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 2048,
            "system": self.system_prompt(),
            "messages": [{ "role": "user", "content": raw }]
        });
        let req = self
            .http
            .post(format!("{}/v1/messages", self.base_url))
            .header("anthropic-version", "2023-06-01")
            .json(&body);
        let resp = self
            .authorize(req)
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

    /// Cheap reachability probe for Meridian's status dot — says the proxy is
    /// listening, nothing about whether it can actually answer.
    pub async fn is_reachable(&self) -> bool {
        self.http.get(&self.base_url).send().await.is_ok()
    }

    /// Round-trips the smallest possible completion to prove the credential
    /// and model actually work. Backs the "Otestovať" button.
    pub async fn probe(&self) -> Result<(), CleanupError> {
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 1,
            "messages": [{ "role": "user", "content": "hi" }]
        });
        let req = self
            .http
            .post(format!("{}/v1/messages", self.base_url))
            .header("anthropic-version", "2023-06-01")
            .json(&body);
        let resp = self
            .authorize(req)
            .send()
            .await
            .map_err(|e| CleanupError::Network(e.to_string()))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        Err(CleanupError::Api {
            status: status.as_u16(),
            body: resp.text().await.unwrap_or_default(),
        })
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
            CleanupStyle::Light,
            Duration::from_millis(50),
        );
        assert!(matches!(c.clean("x").await, Err(CleanupError::Network(_))));
    }

    #[tokio::test]
    async fn strong_style_appends_rephrase_note_to_system_prompt() {
        let server = MockServer::start().await;
        let expected_system = format!("{SYSTEM_PROMPT}{STRONG_SUFFIX}");
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(body_partial_json(serde_json::json!({
                "system": expected_system
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "content": [{ "type": "text", "text": "ok" }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let c = CleanupClient::with_style(server.uri(), "m".into(), CleanupStyle::Strong);
        assert_eq!(c.clean("x").await.unwrap(), "ok");
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

    #[tokio::test]
    async fn for_settings_targets_the_configured_meridian_with_the_placeholder_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "local"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "content": [{ "type": "text", "text": "Ahoj." }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let c = CleanupClient::for_settings(&Settings {
            meridian_url: server.uri(),
            ..Settings::default()
        });
        assert_eq!(c.clean("x").await.unwrap(), "Ahoj.");
    }

    #[tokio::test]
    async fn probe_succeeds_on_2xx_and_reports_the_status_otherwise() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "content": [{ "type": "text", "text": "ok" }]
            })))
            .mount(&server)
            .await;
        assert!(CleanupClient::new(server.uri(), "m".into()).probe().await.is_ok());

        let bad = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503).set_body_string("down"))
            .mount(&bad)
            .await;
        assert!(matches!(
            CleanupClient::new(bad.uri(), "m".into()).probe().await,
            Err(CleanupError::Api { status: 503, .. })
        ));
    }
}
