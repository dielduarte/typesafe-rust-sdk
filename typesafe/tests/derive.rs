//! Typed questions end to end, plus compile-time checks.
#![cfg(feature = "derive")]

use serde_json::{Value, json};
use typesafe::{Choice, Choices, Client, Noul, QuestionSet, Score};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Choices)]
enum Tone {
    Calm,
    VeryAngry,
    #[choice(label = "meh")]
    Indifferent,
}

#[derive(QuestionSet)]
struct Triage {
    #[question(rename = "isBilling")]
    is_billing: Noul,
    tone: Choice<Tone>,
    urgency: Score<3>,
}

#[test]
fn derives_labels() {
    assert_eq!(Tone::LABELS, ["calm", "very_angry", "meh"]);
    assert_eq!(Tone::from_label("very_angry"), Some(Tone::VeryAngry));
    assert_eq!(Tone::Indifferent.label(), "meh");
    assert_eq!(Tone::all().collect::<Vec<_>>(), [Tone::Calm, Tone::VeryAngry, Tone::Indifferent]);
}

#[tokio::test]
async fn sends_typed_questions_and_parses_typed_answers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "jev-1",
            "answers": {
                "isBilling": {"type": "noul", "noul": 0.9},
                "tone": {"type": "choice", "choice": "very_angry", "confidence": 0.7,
                         "probabilities": {"calm": 0.1, "very_angry": 0.7, "meh": 0.2}},
                "urgency": {"type": "score", "score": 1.8, "confidence": 0.6,
                            "legend": {"0": "later", "1": "soon", "2": "now"},
                            "probabilities": {"0": 0.1, "1": 0.2, "2": 0.7}},
            },
            "usage": {"input_tokens": 9, "output_tokens": 3},
        })))
        .mount(&server)
        .await;
    let client = Client::builder().api_key("k").base_url(server.uri()).build().unwrap();

    let response = client
        .system_one(
            "I was charged twice!",
            Triage {
                is_billing: Noul::new("Billing?").yes("about money"),
                tone: Choice::new("Tone?").describe(Tone::VeryAngry, json!({"signs": ["caps", "threats"]})),
                urgency: Score::new("Urgent?", ["later", "soon", "now"]),
            },
        )
        .await
        .unwrap();

    let TriageAnswers { is_billing, tone, urgency } = response.answers;
    assert_eq!(is_billing.noul, 0.9);
    assert_eq!(tone.choice, Tone::VeryAngry);
    assert_eq!(tone.probabilities[Tone::Indifferent], 0.2);
    assert_eq!(urgency.probabilities, [0.1, 0.2, 0.7]);

    let sent = server.received_requests().await.unwrap()[0].body_json::<Value>().unwrap();
    assert_eq!(
        sent["questions"],
        json!({
            "isBilling": {"type": "noul", "instructions": "Billing?", "criteria": {"true": "about money"}},
            "tone": {"type": "choice", "instructions": "Tone?",
                     "criteria": {"calm": null, "very_angry": {"signs": ["caps", "threats"]}, "meh": null}},
            "urgency": {"type": "score", "instructions": "Urgent?", "criteria": ["later", "soon", "now"]},
        })
    );
}

#[tokio::test]
async fn an_answer_outside_the_declared_labels_is_a_decode_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "jev-1",
            "answers": {
                "isBilling": {"type": "noul", "noul": 0.9},
                "tone": {"type": "choice", "choice": "furious", "confidence": 1, "probabilities": {}},
                "urgency": {"type": "score", "score": 0, "confidence": 1,
                            "legend": {"0": "a", "1": "b", "2": "c"}, "probabilities": {"0": 1, "1": 0, "2": 0}},
            },
            "usage": {"input_tokens": 1, "output_tokens": 1},
        })))
        .mount(&server)
        .await;
    let client = Client::builder().api_key("k").base_url(server.uri()).build().unwrap();

    let err = client
        .system_one(
            "x",
            Triage { is_billing: Noul::new("?"), tone: Choice::new("?"), urgency: Score::new("?", ["a", "b", "c"]) },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, typesafe::Error::Decode(_)), "{err:?}");
    assert!(err.to_string().contains("unexpected choice \"furious\""), "{err}");
}

#[test]
fn compile_time_checks() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
