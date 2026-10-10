//! Live API tests. Run with `TYPESAFE_API_KEY=... cargo test --test live -- --ignored --nocapture`.
#![cfg(feature = "derive")]

use std::time::Duration;

use serde_json::json;
use typesafe::{ApiErrorKind, Choice, Choices, Client, Error, Noul, Question, QuestionSet, Score};

fn client() -> Client {
    Client::builder().timeout(Duration::from_secs(120)).build().expect("TYPESAFE_API_KEY is set")
}

fn ticket() -> serde_json::Value {
    json!({
        "subject": "Charged twice this month",
        "body": "I see two charges of $49 on my card for August. I only have one account. Please fix this ASAP.",
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Choices)]
enum Tone {
    Calm,
    Frustrated,
    Angry,
}

#[derive(QuestionSet)]
struct Triage {
    #[question(rename = "isBilling")]
    is_billing: Noul,
    sentiment: Choice<Tone>,
    urgency: Score<3>,
}

#[tokio::test]
#[ignore = "calls the live API"]
async fn lists_models() {
    let response = client().models().list().with_response().await.unwrap();
    println!("{:#?}", response.data);
    assert!(response.request_id.unwrap().starts_with("req_"));
    assert!(!response.data.is_empty());
}

#[tokio::test]
#[ignore = "calls the live API"]
async fn answers_typed_questions() {
    let response = client()
        .system_one(
            ticket(),
            Triage {
                is_billing: Noul::new("Is this ticket about billing?"),
                sentiment: Choice::new("What is the customer's tone?"),
                urgency: Score::new("How urgent is this ticket?", ["can wait", "this week", "today"]),
            },
        )
        .await
        .unwrap();
    println!("{response:#?}");

    let answers = response.answers;
    assert!((0.0..=1.0).contains(&answers.is_billing.noul));
    let total: f64 = answers.sentiment.probabilities.iter().map(|(_, p)| p).sum();
    assert!((total - 1.0).abs() < 0.05, "probabilities sum to {total}");
    assert!((0.0..=2.0).contains(&answers.urgency.score));
    assert_eq!(answers.urgency.legend, [json!("can wait"), json!("this week"), json!("today")]);
}

#[tokio::test]
#[ignore = "calls the live API"]
async fn answers_dynamic_questions_with_rich_descriptions() {
    let response = client()
        .system_one(
            ticket(),
            [
                (
                    "duplicate",
                    Question::noul("Is the customer reporting a duplicate charge?").yes(
                        json!({"meaning": "the same amount charged more than once", "examples": ["billed twice"]}),
                    ),
                ),
                ("tone", Question::choice("Tone?", [("calm", json!({"summary": "measured"})), ("upset", json!(null))])),
            ],
        )
        .await
        .unwrap();
    let tone = response.answers.get("tone").and_then(|a| a.as_choice()).unwrap();
    assert_eq!(tone.probabilities.keys().collect::<Vec<_>>().len(), 2);
}

#[tokio::test]
#[ignore = "calls the live API"]
async fn reports_api_errors_readably() {
    let bad_key = Client::builder().api_key("not-a-real-key").max_retries(0).build().unwrap();
    let err = bad_key.models().list().await.unwrap_err();
    assert!(matches!(&err, Error::Api(e) if e.kind == ApiErrorKind::Authentication), "{err:?}");

    let err = client()
        .system_one("hello", [("q", Question::noul("Is this a greeting?"))])
        .model("no-such-model")
        .max_retries(0)
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "400 Unknown model: no-such-model");
    assert!(err.request_id().unwrap().starts_with("req_"));

    let err = client()
        .system_one("x", [("q", Question::score("?", [json!(123), json!("ok")]))])
        .max_retries(0)
        .await
        .unwrap_err();
    assert_eq!(err.status().map(|s| s.as_u16()), Some(422));
    assert!(err.to_string().starts_with("422 questions.q.score.criteria.0"), "{err}");
}
