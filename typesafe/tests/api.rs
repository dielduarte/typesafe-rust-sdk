//! Behaviour against a mock server: wire format, headers, retries, timeouts, and errors.

use std::time::Duration;

use serde_json::{Value, json};
use typesafe::{Answer, ApiErrorKind, Client, Error, Question, RetryPolicy};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const SYSTEM_ONE_RESPONSE: &str = r#"{
    "model": "jev-1",
    "answers": {"billing": {"type": "noul", "noul": 0.75}},
    "usage": {"input_tokens": 3, "output_tokens": 1}
}"#;

fn ok_json(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.as_bytes().to_vec(), "application/json")
}

/// Fast retries so tests don't wait on real backoff.
fn fast_retries(max_retries: u32) -> RetryPolicy {
    RetryPolicy {
        max_retries,
        backoff_initial: Duration::from_millis(1),
        backoff_max: Duration::from_millis(1),
        ..RetryPolicy::default()
    }
}

fn client(server: &MockServer) -> Client {
    Client::builder().api_key("sk_test_123456789").base_url(server.uri()).retry_policy(fast_retries(2)).build().unwrap()
}

fn billing() -> [(&'static str, Question); 1] {
    [("billing", Question::noul("Is this about billing?"))]
}

fn header_of<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request.headers.get(name).and_then(|v| v.to_str().ok())
}

#[tokio::test]
async fn sends_the_request_and_parses_answers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ok_json(SYSTEM_ONE_RESPONSE))
        .mount(&server)
        .await;

    let response = client(&server).system_one(json!({"subject": "Charged twice"}), billing()).await.unwrap();
    assert_eq!(response.model, "jev-1");
    assert_eq!(response.answers.get("billing").and_then(Answer::as_noul).unwrap().noul, 0.75);
    assert_eq!(response.usage.input_tokens, 3);

    let requests = server.received_requests().await.unwrap();
    let request = &requests[0];
    assert_eq!(header_of(request, "authorization"), Some("Bearer sk_test_123456789"));
    assert_eq!(header_of(request, "content-type"), Some("application/json"));
    assert_eq!(header_of(request, "accept"), Some("application/json"));
    assert!(header_of(request, "user-agent").unwrap().starts_with("typesafe-sdk-rust/"));
    assert!(header_of(request, "x-typesafe-runtime").unwrap().starts_with("rust ("));
    assert_eq!(header_of(request, "x-typesafe-retry-count"), None);
    assert_eq!(
        request.body_json::<Value>().unwrap(),
        json!({
            "state": {"subject": "Charged twice"},
            "questions": {"billing": {"type": "noul", "instructions": "Is this about billing?"}},
            "model": "jev-latest",
        })
    );
}

#[tokio::test]
async fn model_comes_from_the_call_then_the_client() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ok_json(SYSTEM_ONE_RESPONSE)).mount(&server).await;
    let client = Client::builder().api_key("k").base_url(server.uri()).default_model("jev-2").build().unwrap();

    client.system_one("text", billing()).await.unwrap();
    client.system_one("text", billing()).model("jev-3").await.unwrap();

    let models: Vec<Value> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.body_json::<Value>().unwrap()["model"].clone())
        .collect();
    assert_eq!(models, [json!("jev-2"), json!("jev-3")]);
}

#[tokio::test]
async fn rejects_invalid_questions_without_sending() {
    let server = MockServer::start().await;
    let client = client(&server);

    let empty = client.system_one("x", Vec::<(String, Question)>::new()).await.unwrap_err();
    assert!(matches!(empty, Error::InvalidRequest(ref m) if m == "At least one question is required."), "{empty}");
    let short = client.system_one("x", [("s", Question::score("?", ["only"]))]).await.unwrap_err();
    assert!(matches!(short, Error::InvalidRequest(_)), "{short}");

    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn sdk_headers_win_over_default_and_per_call_headers() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ok_json(SYSTEM_ONE_RESPONSE)).mount(&server).await;
    let client = Client::builder()
        .api_key("real-key")
        .base_url(server.uri())
        .default_header("X-Team", "default")
        .default_header("x-only-default", "kept")
        .default_header("Authorization", "Bearer default-override")
        .build()
        .unwrap();

    client
        .system_one("x", billing())
        .header("x-team", "per-call")
        .header("AUTHORIZATION", "Bearer call-override")
        .header("X-TypeSafe-Retry-Count", "7")
        .await
        .unwrap();

    let request = &server.received_requests().await.unwrap()[0];
    assert_eq!(request.headers.get_all("x-team").iter().collect::<Vec<_>>(), ["per-call"]);
    assert_eq!(header_of(request, "x-only-default"), Some("kept"));
    assert_eq!(request.headers.get_all("authorization").iter().collect::<Vec<_>>(), ["Bearer real-key"]);
    assert_eq!(header_of(request, "x-typesafe-retry-count"), None);
}

#[tokio::test]
async fn lists_models_with_a_get_and_no_content_type() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ok_json(r#"{"models": [{"name": "jev-1", "description": "d", "release_date": "2026-09-01"}]}"#))
        .mount(&server)
        .await;

    let models = client(&server).models().list().header("content-type", "text/plain").await.unwrap();
    assert_eq!(models[0].name, "jev-1");
    assert_eq!(header_of(&server.received_requests().await.unwrap()[0], "content-type"), None);
}

#[tokio::test]
async fn an_unexpected_success_body_is_a_decode_error_with_the_request_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ok_json(r#"{"items": []}"#).insert_header("x-typesafe-request-id", "req_42"))
        .mount(&server)
        .await;

    let err = client(&server).models().list().await.unwrap_err();
    assert!(matches!(&err, Error::Decode(e) if e.body == r#"{"items": []}"#), "{err:?}");
    assert_eq!(err.request_id(), Some("req_42"));
}

#[tokio::test]
async fn with_response_exposes_status_headers_and_request_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ok_json(SYSTEM_ONE_RESPONSE).insert_header("x-typesafe-request-id", "req_1"))
        .mount(&server)
        .await;

    let response = client(&server).system_one("x", billing()).with_response().await.unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.request_id.as_deref(), Some("req_1"));
    assert_eq!(response.data.model, "jev-1");
}

#[tokio::test]
async fn maps_error_statuses_with_readable_messages() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({"error": "Unknown model: no-such-model"}))
                .insert_header("x-typesafe-request-id", "req_bad"),
        )
        .mount(&server)
        .await;

    let err = client(&server).system_one("x", billing()).model("no-such-model").await.unwrap_err();
    let Error::Api(api) = &err else { panic!("expected an API error, got {err:?}") };
    assert_eq!(api.kind, ApiErrorKind::BadRequest);
    assert_eq!(err.to_string(), "400 Unknown model: no-such-model");
    assert_eq!(err.request_id(), Some("req_bad"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1, "400 is not retried");
}

#[tokio::test]
async fn retries_a_429_honoring_retry_after_and_counting_retries() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after-ms", "20"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST")).respond_with(ok_json(SYSTEM_ONE_RESPONSE)).mount(&server).await;

    let started = std::time::Instant::now();
    client(&server).system_one("x", billing()).await.unwrap();
    assert!(started.elapsed() >= Duration::from_millis(20), "waited for retry-after-ms");

    let requests = server.received_requests().await.unwrap();
    let counts: Vec<_> = requests.iter().map(|r| header_of(r, "x-typesafe-retry-count")).collect();
    assert_eq!(counts, [None, Some("1")]);
}

#[tokio::test]
async fn gives_up_after_max_retries_with_the_last_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).insert_header("retry-after", "0"))
        .mount(&server)
        .await;

    let err = client(&server).system_one("x", billing()).await.unwrap_err();
    assert_eq!(err.status().map(|s| s.as_u16()), Some(503));
    assert_eq!(server.received_requests().await.unwrap().len(), 3);

    let none = client(&server).system_one("x", billing()).max_retries(0).await.unwrap_err();
    assert_eq!(none.status().map(|s| s.as_u16()), Some(503));
    assert_eq!(server.received_requests().await.unwrap().len(), 4, "per-call max_retries(0) disables retries");
}

#[tokio::test]
async fn retries_only_the_configured_statuses() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(409)).mount(&server).await;
    let mut policy = fast_retries(1);
    policy.http_statuses.insert(typesafe::reqwest::StatusCode::CONFLICT);

    client(&server).system_one("x", billing()).await.unwrap_err();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    client(&server).system_one("x", billing()).retry_policy(policy).await.unwrap_err();
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn rate_limit_errors_expose_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "2"))
        .mount(&server)
        .await;

    let err = client(&server).system_one("x", billing()).max_retries(0).await.unwrap_err();
    let Error::Api(api) = err else { panic!("expected an API error") };
    assert_eq!(api.kind, ApiErrorKind::RateLimit);
    assert_eq!(api.retry_after, Some(Duration::from_secs(2)));
}

#[tokio::test]
async fn times_out_slow_responses_and_retries_with_a_fresh_timeout() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ok_json(SYSTEM_ONE_RESPONSE).set_delay(Duration::from_millis(500)))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST")).respond_with(ok_json(SYSTEM_ONE_RESPONSE)).mount(&server).await;
    let client = client(&server);

    let response = client.system_one("x", billing()).timeout(Duration::from_millis(100)).await.unwrap();
    assert_eq!(response.model, "jev-1");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn a_timeout_is_reported_and_can_be_excluded_from_retries() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ok_json(SYSTEM_ONE_RESPONSE).set_delay(Duration::from_millis(500)))
        .mount(&server)
        .await;
    let policy = RetryPolicy { retry_timeouts: false, ..fast_retries(2) };

    let err = client(&server)
        .system_one("x", billing())
        .timeout(Duration::from_millis(50))
        .retry_policy(policy)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Timeout(t) if t == Duration::from_millis(50)), "{err:?}");
    assert_eq!(err.to_string(), "Request timed out after 50ms.");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn connection_failures_are_connection_errors() {
    // Bind and drop a listener to get a local port with nothing behind it.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let client = Client::builder()
        .api_key("k")
        .base_url(format!("http://127.0.0.1:{port}"))
        .retry_policy(fast_retries(1))
        .build()
        .unwrap();

    let err = client.models().list().await.unwrap_err();
    assert!(matches!(err, Error::Connection(_)), "{err:?}");
    assert!(std::error::Error::source(&err).is_some());
}

#[tokio::test]
async fn dropping_a_request_cancels_it() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(header("accept", "application/json"))
        .respond_with(ok_json(r#"{"models": []}"#).set_delay(Duration::from_secs(5)))
        .mount(&server)
        .await;

    let started = std::time::Instant::now();
    let outcome = tokio::time::timeout(Duration::from_millis(50), client(&server).models().list()).await;
    assert!(outcome.is_err(), "the caller's timeout fired first");
    assert!(started.elapsed() < Duration::from_secs(1));
}
