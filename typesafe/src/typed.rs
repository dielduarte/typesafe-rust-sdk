//! Typed questions whose answers are checked at compile time.
//!
//! Derive [`QuestionSet`](macro@crate::QuestionSet) on a struct whose fields are [`Noul`],
//! [`Choice`], or [`Score`] questions. The derive generates a `<Name>Answers` struct with one
//! typed answer per field:
//!
//! ```no_run
//! # async fn run(client: typesafe::Client) -> typesafe::Result<()> {
//! use typesafe::{Choice, Choices, Noul, QuestionSet, Score};
//!
//! #[derive(Debug, Clone, Copy, PartialEq, Eq, Choices)]
//! enum Tone { Calm, Frustrated, Angry }
//!
//! #[derive(QuestionSet)]
//! struct Triage {
//!     is_billing: Noul,
//!     tone: Choice<Tone>,
//!     urgency: Score<3>,
//! }
//!
//! let response = client
//!     .system_one("I was charged twice!", Triage {
//!         is_billing: Noul::new("Is this ticket about billing?"),
//!         tone: Choice::new("What is the customer's tone?"),
//!         urgency: Score::new("How urgent is this?", ["can wait", "this week", "today"]),
//!     })
//!     .await?;
//!
//! let answers: TriageAnswers = response.answers;
//! match answers.tone.choice {
//!     Tone::Angry => println!("escalate"),
//!     Tone::Calm | Tone::Frustrated => {}
//! }
//! let p_today: f64 = answers.urgency.probabilities[2];
//! # Ok(()) }
//! ```

use std::fmt;
use std::marker::PhantomData;
use std::ops::Index;

use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::types::{
    Answers, ChoiceQuestion, ChoiceResult, NoulAnswer, NoulCriteria, NoulQuestion, Question, Questions, ScoreQuestion,
    ScoreResult,
};

/// A set of named questions and the type its answers deserialize into.
///
/// Implemented by [`Questions`], arrays and vectors of `(name, Question)` pairs (answers are
/// [`Answers`]), and by `#[derive(QuestionSet)]` structs (answers are the generated struct).
pub trait QuestionSet {
    /// The answers to these questions.
    type Answers: DeserializeOwned + Send + 'static;

    /// The questions, keyed by the names their answers are returned under.
    fn into_questions(self) -> Questions;
}

impl QuestionSet for Questions {
    type Answers = Answers;

    fn into_questions(self) -> Questions {
        self
    }
}

impl<K: Into<String>, const N: usize> QuestionSet for [(K, Question); N] {
    type Answers = Answers;

    fn into_questions(self) -> Questions {
        self.into_iter().collect()
    }
}

impl<K: Into<String>> QuestionSet for Vec<(K, Question)> {
    type Answers = Answers;

    fn into_questions(self) -> Questions {
        self.into_iter().collect()
    }
}

/// A question with a statically known answer type; the field types of a `QuestionSet` derive.
pub trait TypedQuestion {
    /// The answer to this question.
    type Answer: DeserializeOwned;

    /// Compile-time validity check, evaluated by the `QuestionSet` derive for each field.
    #[doc(hidden)]
    const CHECK: () = ();

    /// The question as sent on the wire.
    fn into_question(self) -> Question;
}

// ---------------------------------------------------------------------------
// Noul
// ---------------------------------------------------------------------------

/// A yes/no question. Its answer is a [`NoulAnswer`].
#[derive(Debug, Clone, PartialEq)]
pub struct Noul(NoulQuestion);

impl Noul {
    /// A yes/no question.
    pub fn new(instructions: impl Into<Value>) -> Self {
        Self(NoulQuestion { instructions: instructions.into(), criteria: None })
    }

    /// Describe the yes outcome.
    #[must_use]
    pub fn yes(mut self, description: impl Into<Value>) -> Self {
        self.0.criteria.get_or_insert_with(NoulCriteria::default).yes = Some(description.into());
        self
    }

    /// Describe the no outcome.
    #[must_use]
    pub fn no(mut self, description: impl Into<Value>) -> Self {
        self.0.criteria.get_or_insert_with(NoulCriteria::default).no = Some(description.into());
        self
    }
}

impl TypedQuestion for Noul {
    type Answer = NoulAnswer;

    fn into_question(self) -> Question {
        Question::Noul(self.0)
    }
}

// ---------------------------------------------------------------------------
// Choice
// ---------------------------------------------------------------------------

/// A fixed set of choice labels, usually derived on a fieldless enum.
///
/// The derive uses each variant's name in `snake_case` as its label; override it with
/// `#[choice(label = "...")]`.
pub trait Choices: Copy + Eq + fmt::Debug + Send + Sync + 'static {
    /// Labels in declaration order, indexed by [`index`](Self::index).
    const LABELS: &'static [&'static str];

    /// The position of this choice in [`LABELS`](Self::LABELS).
    fn index(self) -> usize;

    /// The choice at `index`, if any.
    fn from_index(index: usize) -> Option<Self>;

    /// The wire label of this choice.
    fn label(self) -> &'static str {
        Self::LABELS[self.index()]
    }

    /// The choice with wire label `label`, if any.
    fn from_label(label: &str) -> Option<Self> {
        Self::LABELS.iter().position(|l| *l == label).and_then(Self::from_index)
    }

    /// Every choice, in declaration order.
    fn all() -> impl Iterator<Item = Self> {
        (0..Self::LABELS.len()).filter_map(Self::from_index)
    }
}

/// A question that selects one of the labels of `E`. Its answer is a [`ChoiceAnswer<E>`].
#[derive(Debug, Clone, PartialEq)]
pub struct Choice<E: Choices> {
    instructions: Value,
    descriptions: Vec<Value>,
    _labels: PhantomData<E>,
}

impl<E: Choices> Choice<E> {
    /// A choice question with every label undescribed.
    pub fn new(instructions: impl Into<Value>) -> Self {
        Self {
            instructions: instructions.into(),
            descriptions: vec![Value::Null; E::LABELS.len()],
            _labels: PhantomData,
        }
    }

    /// Describe one label.
    #[must_use]
    pub fn describe(mut self, choice: E, description: impl Into<Value>) -> Self {
        self.descriptions[choice.index()] = description.into();
        self
    }
}

impl<E: Choices> TypedQuestion for Choice<E> {
    type Answer = ChoiceAnswer<E>;

    fn into_question(self) -> Question {
        Question::Choice(ChoiceQuestion {
            instructions: self.instructions,
            criteria: E::LABELS.iter().copied().zip(self.descriptions).collect(),
        })
    }
}

/// The selected choice and the probability of each choice.
#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceAnswer<E: Choices> {
    /// The selected choice.
    pub choice: E,
    /// Reported confidence in the selected choice.
    pub confidence: f64,
    /// Probability of each choice.
    pub probabilities: Probabilities<E>,
}

impl<'de, E: Choices> Deserialize<'de> for ChoiceAnswer<E> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = ChoiceResult::deserialize(deserializer)?;
        let choice = E::from_label(&raw.choice).ok_or_else(|| {
            D::Error::custom(format!("unexpected choice {:?}, expected one of {:?}", raw.choice, E::LABELS))
        })?;
        let values = E::LABELS
            .iter()
            .map(|label| {
                raw.probabilities
                    .get(label)
                    .copied()
                    .ok_or_else(|| D::Error::custom(format!("missing probability for choice {label:?}")))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { choice, confidence: raw.confidence, probabilities: Probabilities { values, _labels: PhantomData } })
    }
}

/// Probabilities indexed by choice: `probabilities[Tone::Angry]`.
#[derive(Clone, PartialEq)]
pub struct Probabilities<E: Choices> {
    values: Vec<f64>,
    _labels: PhantomData<E>,
}

impl<E: Choices> Probabilities<E> {
    /// The probability of `choice`.
    pub fn get(&self, choice: E) -> f64 {
        self.values[choice.index()]
    }

    /// Each choice with its probability, in declaration order.
    pub fn iter(&self) -> impl Iterator<Item = (E, f64)> + '_ {
        E::all().zip(self.values.iter().copied())
    }
}

impl<E: Choices> Index<E> for Probabilities<E> {
    type Output = f64;

    fn index(&self, choice: E) -> &f64 {
        &self.values[choice.index()]
    }
}

impl<E: Choices> fmt::Debug for Probabilities<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

// ---------------------------------------------------------------------------
// Score
// ---------------------------------------------------------------------------

/// A question scored from `0` to `N - 1` against an `N`-entry rubric. Its answer is a
/// [`ScoreAnswer<N>`]. `N` must be at least 2, which is checked at compile time.
#[derive(Debug, Clone, PartialEq)]
pub struct Score<const N: usize> {
    instructions: Value,
    rubric: [Value; N],
}

impl<const N: usize> Score<N> {
    /// A score question with one rubric description per score, from zero.
    pub fn new<V: Into<Value>>(instructions: impl Into<Value>, rubric: [V; N]) -> Self {
        const { assert!(N >= 2, "a score question needs at least two rubric entries") };
        Self { instructions: instructions.into(), rubric: rubric.map(Into::into) }
    }
}

impl<const N: usize> TypedQuestion for Score<N> {
    type Answer = ScoreAnswer<N>;

    const CHECK: () = assert!(N >= 2, "a score question needs at least two rubric entries");

    fn into_question(self) -> Question {
        Question::Score(ScoreQuestion { instructions: self.instructions, criteria: self.rubric.into() })
    }
}

/// An expected score with its rubric and the probability of each score.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreAnswer<const N: usize> {
    /// Expected score, which may fall between integer rubric levels.
    pub score: f64,
    /// Reported confidence in the score.
    pub confidence: f64,
    /// Rubric descriptions indexed by score.
    pub legend: [Value; N],
    /// Probabilities indexed by score.
    pub probabilities: [f64; N],
}

impl<'de, const N: usize> Deserialize<'de> for ScoreAnswer<N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = ScoreResult::deserialize(deserializer)?;
        let wrong_len = |what: &str, len: usize| D::Error::custom(format!("expected {N} {what}, got {len}"));
        let legend_len = raw.legend.len();
        let probabilities_len = raw.probabilities.len();
        Ok(Self {
            score: raw.score,
            confidence: raw.confidence,
            legend: raw.legend.try_into().map_err(|_| wrong_len("legend entries", legend_len))?,
            probabilities: raw.probabilities.try_into().map_err(|_| wrong_len("probabilities", probabilities_len))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Tone {
        Calm,
        Angry,
    }

    impl Choices for Tone {
        const LABELS: &'static [&'static str] = &["calm", "angry"];

        fn index(self) -> usize {
            self as usize
        }

        fn from_index(index: usize) -> Option<Self> {
            [Self::Calm, Self::Angry].get(index).copied()
        }
    }

    #[test]
    fn choice_serializes_every_label_in_order() {
        let q = Choice::<Tone>::new("Tone?").describe(Tone::Angry, "shouting").into_question();
        assert_eq!(
            serde_json::to_value(q).unwrap(),
            json!({"type": "choice", "instructions": "Tone?", "criteria": {"calm": null, "angry": "shouting"}})
        );
    }

    #[test]
    fn choice_answers_are_typed_and_strict() {
        let answer: ChoiceAnswer<Tone> = serde_json::from_value(json!({
            "type": "choice", "choice": "angry", "confidence": 0.9,
            "probabilities": {"angry": 0.9, "calm": 0.1},
        }))
        .unwrap();
        assert_eq!(answer.choice, Tone::Angry);
        assert_eq!(answer.probabilities[Tone::Calm], 0.1);
        assert_eq!(answer.probabilities.iter().collect::<Vec<_>>(), [(Tone::Calm, 0.1), (Tone::Angry, 0.9)]);

        let unknown = json!({"choice": "meh", "confidence": 1, "probabilities": {"calm": 0.5, "angry": 0.5}});
        assert!(serde_json::from_value::<ChoiceAnswer<Tone>>(unknown).is_err());
        let missing = json!({"choice": "calm", "confidence": 1, "probabilities": {"calm": 1}});
        assert!(serde_json::from_value::<ChoiceAnswer<Tone>>(missing).is_err());
    }

    #[test]
    fn score_answers_have_fixed_arity() {
        let wire = json!({
            "type": "score", "score": 0.4, "confidence": 0.7,
            "legend": {"0": "low", "1": "high"}, "probabilities": {"0": 0.6, "1": 0.4},
        });
        let answer: ScoreAnswer<2> = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(answer.probabilities, [0.6, 0.4]);
        assert_eq!(answer.legend, [json!("low"), json!("high")]);

        let err = serde_json::from_value::<ScoreAnswer<3>>(wire).unwrap_err();
        assert!(err.to_string().contains("expected 3 legend entries, got 2"), "{err}");
    }

    #[test]
    fn noul_round_trips_criteria() {
        let q = Noul::new("Billing?").no("not about money").into_question();
        assert_eq!(
            serde_json::to_value(q).unwrap(),
            json!({"type": "noul", "instructions": "Billing?", "criteria": {"false": "not about money"}})
        );
    }
}
