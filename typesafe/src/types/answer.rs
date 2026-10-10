use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::OrderedMap;
use super::map::indexed;

/// Answers keyed by question name, in the order the server returned them.
pub type Answers = OrderedMap<Answer>;

/// The answer to one question.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Answer {
    /// The answer to a yes/no question.
    Noul(NoulAnswer),
    /// The answer to a choice question.
    Choice(ChoiceResult),
    /// The answer to a score question.
    Score(ScoreResult),
    /// An answer type this SDK version does not know, kept as raw JSON.
    Unknown(Value),
}

impl Answer {
    /// The noul answer, if this is one.
    pub fn as_noul(&self) -> Option<&NoulAnswer> {
        match self {
            Self::Noul(a) => Some(a),
            _ => None,
        }
    }

    /// The choice answer, if this is one.
    pub fn as_choice(&self) -> Option<&ChoiceResult> {
        match self {
            Self::Choice(a) => Some(a),
            _ => None,
        }
    }

    /// The score answer, if this is one.
    pub fn as_score(&self) -> Option<&ScoreResult> {
        match self {
            Self::Score(a) => Some(a),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for Answer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let parsed = match value.get("type").and_then(Value::as_str) {
            Some("noul") => serde_json::from_value(value).map(Self::Noul),
            Some("choice") => serde_json::from_value(value).map(Self::Choice),
            Some("score") => serde_json::from_value(value).map(Self::Score),
            _ => return Ok(Self::Unknown(value)),
        };
        parsed.map_err(D::Error::custom)
    }
}

/// A yes/no answer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NoulAnswer {
    /// Probability of a yes answer, from zero to one.
    pub noul: f64,
}

/// A selected label and its probabilities.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ChoiceResult {
    /// The selected label.
    pub choice: String,
    /// Reported confidence in the selected label.
    pub confidence: f64,
    /// Probabilities keyed by label.
    pub probabilities: OrderedMap<f64>,
}

/// An expected score with its rubric and probabilities.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ScoreResult {
    /// Expected score, which may fall between integer rubric levels.
    pub score: f64,
    /// Reported confidence in the score.
    pub confidence: f64,
    /// Rubric descriptions indexed by score.
    #[serde(deserialize_with = "indexed")]
    pub legend: Vec<Value>,
    /// Probabilities indexed by score.
    #[serde(deserialize_with = "indexed")]
    pub probabilities: Vec<f64>,
}

/// Token usage for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Number of input tokens used.
    pub input_tokens: u64,
    /// Number of output tokens used.
    pub output_tokens: u64,
}

/// The response to a System One request.
///
/// `A` is [`Answers`] for dynamic questions, or the generated answers struct of a
/// `#[derive(QuestionSet)]` type.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SystemOneResponse<A = Answers> {
    /// The model used to answer the request.
    pub model: String,
    /// Answers to the questions.
    pub answers: A,
    /// Token usage for the request.
    pub usage: Usage,
}

/// Metadata for an available model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCard {
    /// Model name, used as `model` in requests.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Release date as reported by the API.
    pub release_date: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_every_answer_type() {
        let response: SystemOneResponse = serde_json::from_value(json!({
            "model": "jev-1",
            "answers": {
                "billing": {"type": "noul", "noul": 0.9},
                "tone": {"type": "choice", "choice": "angry", "confidence": 0.8,
                         "probabilities": {"calm": 0.2, "angry": 0.8}},
                "urgency": {"type": "score", "score": 1.4, "confidence": 0.6,
                            "legend": {"0": "later", "1": "soon", "2": "now"},
                            "probabilities": {"0": 0.1, "1": 0.4, "2": 0.5}},
                "future": {"type": "ranking", "order": ["a"]},
            },
            "usage": {"input_tokens": 10, "output_tokens": 2},
        }))
        .unwrap();

        let answers = &response.answers;
        assert_eq!(answers.get("billing").and_then(Answer::as_noul).unwrap().noul, 0.9);
        assert_eq!(answers.get("tone").and_then(Answer::as_choice).unwrap().choice, "angry");
        let urgency = answers.get("urgency").and_then(Answer::as_score).unwrap();
        assert_eq!(urgency.legend, vec![json!("later"), json!("soon"), json!("now")]);
        assert_eq!(urgency.probabilities, vec![0.1, 0.4, 0.5]);
        assert!(matches!(answers.get("future"), Some(Answer::Unknown(_))));
    }

    #[test]
    fn a_malformed_known_answer_is_an_error() {
        let err = serde_json::from_value::<Answer>(json!({"type": "noul"})).unwrap_err();
        assert!(err.to_string().contains("noul"), "{err}");
    }
}
