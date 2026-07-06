use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SttError {
    #[error("network: {0}")]
    Network(String),
    #[error("groq api {status}: {body}")]
    Api { status: u16, body: String },
}

#[derive(Deserialize)]
struct GroqResponse {
    text: String,
    #[serde(default)]
    language: Option<String>,
}

pub struct SttClient {
    base_url: String,
    api_key: String,
    http: reqwest::Client,
}

pub const GROQ_MODEL: &str = "whisper-large-v3-turbo";

impl SttClient {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(5))
                .build()
                .expect("reqwest client"),
        }
    }

    pub async fn transcribe(
        &self,
        wav: Vec<u8>,
        language: Option<&str>,
    ) -> Result<Transcript, SttError> {
        let part = reqwest::multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| SttError::Network(e.to_string()))?;
        let mut form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("model", GROQ_MODEL)
            .text("response_format", "verbose_json");
        if let Some(lang) = language {
            form = form.text("language", lang.to_string());
        }
        let resp = self
            .http
            .post(format!("{}/openai/v1/audio/transcriptions", self.base_url))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| SttError::Network(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(SttError::Api {
                status: status.as_u16(),
                body: resp.text().await.unwrap_or_default(),
            });
        }
        let g: GroqResponse = resp
            .json()
            .await
            .map_err(|e| SttError::Network(e.to_string()))?;
        Ok(Transcript {
            text: g.text.trim().to_string(),
            language: g.language,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn transcribes_and_reads_language() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/openai/v1/audio/transcriptions"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": "  ahoj svet  ",
                "language": "sk"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = SttClient::new(server.uri(), "test-key".into());
        let t = client.transcribe(vec![1, 2, 3], None).await.unwrap();
        assert_eq!(t.text, "ahoj svet");
        assert_eq!(t.language.as_deref(), Some("sk"));
    }

    #[tokio::test]
    async fn api_error_surfaces_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let client = SttClient::new(server.uri(), "k".into());
        match client.transcribe(vec![0], Some("sk")).await {
            Err(SttError::Api { status: 429, body }) => assert_eq!(body, "rate limited"),
            other => panic!("expected 429, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn network_error_when_server_unreachable() {
        let client = SttClient::new("http://127.0.0.1:1".into(), "k".into());
        assert!(matches!(
            client.transcribe(vec![0], None).await,
            Err(SttError::Network(_))
        ));
    }
}
