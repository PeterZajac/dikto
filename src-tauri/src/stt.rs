use serde::Deserialize;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SttError {
    #[error("network: {0}")]
    Network(String),
    #[error("groq api {status}: {message}")]
    Api {
        status: u16,
        message: String,
        retry_after: Option<Duration>,
    },
    #[error("parse: {0}")]
    Parse(String),
}

impl SttError {
    pub fn is_rate_limit(&self) -> bool {
        matches!(self, SttError::Api { status: 429, .. })
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            SttError::Api { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// Transient failures worth another attempt: rate limits, server-side
    /// errors and dropped connections. A 400/401 means the request or the key
    /// is wrong — retrying just burns the user's quota.
    fn is_transient(&self) -> bool {
        match self {
            SttError::Network(_) => true,
            SttError::Api { status, .. } => *status == 429 || *status >= 500,
            SttError::Parse(_) => false,
        }
    }
}

#[derive(Deserialize)]
struct GroqResponse {
    text: String,
    #[serde(default)]
    language: Option<String>,
}

/// Shape of Groq's error payload — used to show the human-readable reason
/// instead of a wall of raw JSON.
#[derive(Deserialize)]
struct GroqErrorBody {
    error: GroqErrorDetail,
}

#[derive(Deserialize)]
struct GroqErrorDetail {
    #[serde(default)]
    message: String,
}

const MAX_ERROR_CHARS: usize = 300;

/// Pulls `error.message` out of Groq's JSON, falling back to the raw body.
/// Either way the result is trimmed to something a bubble can display.
fn error_message(body: &str) -> String {
    let text = serde_json::from_str::<GroqErrorBody>(body)
        .map(|b| b.error.message)
        .ok()
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| body.trim().to_string());
    if text.chars().count() > MAX_ERROR_CHARS {
        let cut: String = text.chars().take(MAX_ERROR_CHARS).collect();
        format!("{cut}…")
    } else {
        text
    }
}

/// Groq states the wait in a `retry-after` header, and repeats it in prose
/// ("Please try again in 6.432s"). Read the header first, then the prose —
/// obeying the server beats guessing with exponential backoff.
fn parse_retry_after(header: Option<&str>, message: &str) -> Option<Duration> {
    if let Some(secs) = header.and_then(|h| h.trim().parse::<f64>().ok()) {
        if secs.is_finite() && secs >= 0.0 {
            return Some(Duration::from_secs_f64(secs.min(300.0)));
        }
    }
    let rest = message.split("try again in").nth(1)?.trim_start();
    let digits: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let secs = digits.parse::<f64>().ok()?;
    // `digits` is ASCII, so its byte length is a valid split point in `rest`.
    let unit = rest[digits.len()..].trim_start();
    let secs = if unit.starts_with('m') && !unit.starts_with("ms") {
        secs * 60.0
    } else if unit.starts_with("ms") {
        secs / 1000.0
    } else {
        secs
    };
    (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs.min(300.0)))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    /// Ceiling on time spent retrying. The user is staring at a bubble — past
    /// this we stop and hand them a failed row they can retry from history.
    pub max_total: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            base_delay: Duration::from_secs(1),
            max_total: Duration::from_secs(20),
        }
    }
}

impl RetryPolicy {
    /// For the live preview: expendable, so one shot and move on.
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            base_delay: Duration::ZERO,
            max_total: Duration::ZERO,
        }
    }

    /// Server's `Retry-After` if it gave one, else exponential on `base_delay`.
    fn delay_for(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        retry_after.unwrap_or_else(|| self.base_delay * 2u32.pow(attempt.saturating_sub(1).min(5)))
    }
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
        self.transcribe_with(wav, language, RetryPolicy::default(), |_, _| {})
            .await
    }

    /// Transcribes, retrying transient failures per `policy`. `on_retry` is
    /// called with the upcoming attempt number and the wait before it, so the
    /// caller can tell the user we're waiting out a rate limit rather than
    /// having failed.
    pub async fn transcribe_with<F: Fn(u32, Duration)>(
        &self,
        wav: Vec<u8>,
        language: Option<&str>,
        policy: RetryPolicy,
        on_retry: F,
    ) -> Result<Transcript, SttError> {
        let started = Instant::now();
        let mut attempt = 1;
        loop {
            let err = match self.attempt(wav.clone(), language).await {
                Ok(t) => return Ok(t),
                Err(e) => e,
            };
            if attempt >= policy.max_attempts || !err.is_transient() {
                return Err(err);
            }
            let delay = policy.delay_for(attempt, err.retry_after());
            if started.elapsed() + delay > policy.max_total {
                return Err(err);
            }
            attempt += 1;
            on_retry(attempt, delay);
            tokio::time::sleep(delay).await;
        }
    }

    async fn attempt(
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
            let header = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let body = resp.text().await.unwrap_or_default();
            let message = error_message(&body);
            let retry_after = parse_retry_after(header.as_deref(), &message);
            return Err(SttError::Api {
                status: status.as_u16(),
                message,
                retry_after,
            });
        }
        let g: GroqResponse = resp
            .json()
            .await
            .map_err(|e| SttError::Parse(e.to_string()))?;
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

    fn fast_policy() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 4,
            base_delay: Duration::from_millis(5),
            max_total: Duration::from_secs(5),
        }
    }

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
    async fn api_error_surfaces_status_and_readable_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let client = SttClient::new(server.uri(), "k".into());
        match client.transcribe(vec![0], Some("sk")).await {
            Err(SttError::Api { status: 400, message, .. }) => assert_eq!(message, "rate limited"),
            other => panic!("expected 400, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn network_error_when_server_unreachable() {
        let client = SttClient::new("http://127.0.0.1:1".into(), "k".into());
        let err = client
            .transcribe_with(vec![0], None, RetryPolicy::none(), |_, _| {})
            .await
            .unwrap_err();
        assert!(matches!(err, SttError::Network(_)));
    }

    #[tokio::test]
    async fn invalid_json_on_2xx_surfaces_as_parse_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let client = SttClient::new(server.uri(), "k".into());
        assert!(matches!(
            client.transcribe(vec![0], None).await,
            Err(SttError::Parse(_))
        ));
    }

    // ---- retry ----

    #[tokio::test]
    async fn a_429_is_retried_and_the_next_attempt_wins() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).set_body_string("slow down"))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": "dlha veta ktoru nechcem stratit"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = SttClient::new(server.uri(), "k".into());
        let t = client
            .transcribe_with(vec![0], None, fast_policy(), |_, _| {})
            .await
            .unwrap();
        assert_eq!(t.text, "dlha veta ktoru nechcem stratit");
    }

    #[tokio::test]
    async fn a_500_is_retried_too() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503).set_body_string("upstream down"))
            .up_to_n_times(2)
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "text": "ok" })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = SttClient::new(server.uri(), "k".into());
        assert_eq!(
            client
                .transcribe_with(vec![0], None, fast_policy(), |_, _| {})
                .await
                .unwrap()
                .text,
            "ok"
        );
    }

    #[tokio::test]
    async fn a_401_is_not_retried() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string("bad key"))
            .expect(1)
            .mount(&server)
            .await;

        let client = SttClient::new(server.uri(), "k".into());
        let err = client
            .transcribe_with(vec![0], None, fast_policy(), |_, _| {})
            .await
            .unwrap_err();
        assert!(matches!(err, SttError::Api { status: 401, .. }));
    }

    #[tokio::test]
    async fn retries_are_announced_to_the_caller() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).set_body_string("nope"))
            .mount(&server)
            .await;

        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        let client = SttClient::new(server.uri(), "k".into());
        let err = client
            .transcribe_with(vec![0], None, fast_policy(), move |attempt, delay| {
                sink.lock().unwrap().push((attempt, delay));
            })
            .await
            .unwrap_err();

        assert!(err.is_rate_limit());
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3, "4 attempts means 3 announced waits");
        assert_eq!(seen[0].0, 2);
        assert_eq!(seen[2].0, 4);
    }

    #[tokio::test]
    async fn policy_none_makes_a_single_attempt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).set_body_string("nope"))
            .expect(1)
            .mount(&server)
            .await;

        let client = SttClient::new(server.uri(), "k".into());
        assert!(client
            .transcribe_with(vec![0], None, RetryPolicy::none(), |_, _| {})
            .await
            .is_err());
    }

    #[tokio::test]
    async fn retry_after_header_is_carried_on_the_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "7")
                    .set_body_string("slow down"),
            )
            .mount(&server)
            .await;

        let client = SttClient::new(server.uri(), "k".into());
        let err = client
            .transcribe_with(vec![0], None, RetryPolicy::none(), |_, _| {})
            .await
            .unwrap_err();
        assert_eq!(err.retry_after(), Some(Duration::from_secs(7)));
    }

    #[tokio::test]
    async fn groq_json_error_is_unwrapped_to_its_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).set_body_json(serde_json::json!({
                "error": {
                    "message": "Rate limit reached for model `whisper-large-v3-turbo`. Please try again in 6.432s.",
                    "type": "rate_limit_exceeded"
                }
            })))
            .mount(&server)
            .await;

        let client = SttClient::new(server.uri(), "k".into());
        let err = client
            .transcribe_with(vec![0], None, RetryPolicy::none(), |_, _| {})
            .await
            .unwrap_err();
        let SttError::Api { message, retry_after, .. } = &err else {
            panic!("expected api error, got {err:?}");
        };
        assert!(message.starts_with("Rate limit reached for model"));
        assert!(!message.contains('{'), "raw JSON must not reach the user");
        // No retry-after header, so the wait comes from the prose.
        assert_eq!(*retry_after, Some(Duration::from_secs_f64(6.432)));
    }

    // ---- pure helpers ----

    #[test]
    fn error_message_falls_back_to_the_raw_body() {
        assert_eq!(error_message("plain text boom"), "plain text boom");
        assert_eq!(error_message("{\"error\":{\"message\":\"\"}}"), "{\"error\":{\"message\":\"\"}}");
    }

    #[test]
    fn error_message_is_truncated() {
        let long = "x".repeat(MAX_ERROR_CHARS + 50);
        let out = error_message(&long);
        assert_eq!(out.chars().count(), MAX_ERROR_CHARS + 1);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn retry_after_prefers_the_header() {
        assert_eq!(
            parse_retry_after(Some("3"), "Please try again in 60s"),
            Some(Duration::from_secs(3))
        );
    }

    #[test]
    fn retry_after_reads_minutes_and_millis_from_prose() {
        assert_eq!(
            parse_retry_after(None, "Please try again in 2m30s"),
            Some(Duration::from_secs(120))
        );
        assert_eq!(
            parse_retry_after(None, "Please try again in 500ms"),
            Some(Duration::from_secs_f64(0.5))
        );
    }

    #[test]
    fn retry_after_is_none_when_nothing_says_so() {
        assert_eq!(parse_retry_after(None, "something else broke"), None);
        assert_eq!(parse_retry_after(Some("not-a-number"), "boom"), None);
    }

    #[test]
    fn retry_after_is_clamped() {
        assert_eq!(
            parse_retry_after(Some("99999"), ""),
            Some(Duration::from_secs(300))
        );
    }

    #[test]
    fn delay_backs_off_exponentially_without_a_server_hint() {
        let p = RetryPolicy { base_delay: Duration::from_secs(1), ..RetryPolicy::default() };
        assert_eq!(p.delay_for(1, None), Duration::from_secs(1));
        assert_eq!(p.delay_for(2, None), Duration::from_secs(2));
        assert_eq!(p.delay_for(3, None), Duration::from_secs(4));
        assert_eq!(p.delay_for(2, Some(Duration::from_secs(9))), Duration::from_secs(9));
    }
}
