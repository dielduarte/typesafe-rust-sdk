use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::OrderedMap;

/// Questions keyed by the names used to identify their answers, in insertion order.
pub type Questions = OrderedMap<Question>;

/// A question identified on the wire by its `type` field.
///
/// Instructions and criteria descriptions take any JSON: text, an object, an array, or `null`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// A yes/no question.
    Noul(NoulQuestion),
    /// A question that selects between named alternatives.
    Choice(ChoiceQuestion),
    /// A question that assigns a score using an ordered rubric.
    Score(ScoreQuestion),
}

/// A yes/no question with optional descriptions for either outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulQuestion {
    /// The question.
    pub instructions: Value,
    /// Optional descriptions of the yes and no outcomes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<NoulCriteria>,
}

/// Descriptions of the yes and no outcomes of a noul question.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NoulCriteria {
    /// Description of the yes outcome.
    #[serde(rename = "true", default, skip_serializing_if = "Option::is_none")]
    pub yes: Option<Value>,
    /// Description of the no outcome.
    #[serde(rename = "false", default, skip_serializing_if = "Option::is_none")]
    pub no: Option<Value>,
}

/// A question that selects between named alternatives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceQuestion {
    /// The question.
    pub instructions: Value,
    /// Labels mapped to descriptions; `null` leaves a label undescribed.
    pub criteria: OrderedMap<Value>,
}

/// A question that assigns a score using an ordered rubric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreQuestion {
    /// The question.
    pub instructions: Value,
    /// Descriptions indexed by score from zero; at least two are required.
    pub criteria: Vec<Value>,
}

impl Question {
    /// A yes/no question.
    pub fn noul(instructions: impl Into<Value>) -> Self {
        Self::Noul(NoulQuestion { instructions: instructions.into(), criteria: None })
    }

    /// A question that selects one of `labels`, each with a description or `Value::Null`.
    ///
    /// ```
    /// use typesafe::Question;
    /// use serde_json::Value;
    ///
    /// let tone = Question::choice("What is the customer's tone?", [
    ///     ("calm", Value::Null),
    ///     ("angry", "raised voice, threats to leave".into()),
    /// ]);
    /// ```
    pub fn choice<K, V>(instructions: impl Into<Value>, labels: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<String>,
        V: Into<Value>,
    {
        Self::Choice(ChoiceQuestion {
            instructions: instructions.into(),
            criteria: labels.into_iter().map(|(k, v)| (k, v.into())).collect(),
        })
    }

    /// A question scored from zero against `rubric`, which needs at least two entries.
    ///
    /// The count is checked when the request is sent.
    pub fn score<V: Into<Value>>(instructions: impl Into<Value>, rubric: impl IntoIterator<Item = V>) -> Self {
        Self::Score(ScoreQuestion {
            instructions: instructions.into(),
            criteria: rubric.into_iter().map(Into::into).collect(),
        })
    }

    /// Describe the yes outcome of a noul question. Has no effect on other questions.
    #[must_use]
    pub fn yes(mut self, description: impl Into<Value>) -> Self {
        if let Self::Noul(q) = &mut self {
            q.criteria.get_or_insert_with(NoulCriteria::default).yes = Some(description.into());
        }
        self
    }

    /// Describe the no outcome of a noul question. Has no effect on other questions.
    #[must_use]
    pub fn no(mut self, description: impl Into<Value>) -> Self {
        if let Self::Noul(q) = &mut self {
            q.criteria.get_or_insert_with(NoulCriteria::default).no = Some(description.into());
        }
        self
    }
}

/// Reject empty question sets and score questions with fewer than two criteria.
pub(crate) fn validate(questions: &Questions) -> Result<(), String> {
    if questions.is_empty() {
        return Err("At least one question is required.".into());
    }
    for (name, question) in questions.iter() {
        let Question::Score(q) = question else { continue };
        if q.criteria.len() < 2 {
            return Err(format!(
                "Score question \"{name}\" has {} criteria; at least two scores are required.",
                q.criteria.len()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn serializes_the_wire_format() {
        let questions: Questions = [
            ("billing", Question::noul("Billing?").yes("about money")),
            ("tone", Question::choice("Tone?", [("calm", Value::Null), ("angry", json!({"x": 1}))])),
            ("urgency", Question::score("Urgent?", ["later", "now"])),
        ]
        .into_iter()
        .collect();

        assert_eq!(
            serde_json::to_value(&questions).unwrap(),
            json!({
                "billing": {"type": "noul", "instructions": "Billing?", "criteria": {"true": "about money"}},
                "tone": {"type": "choice", "instructions": "Tone?", "criteria": {"calm": null, "angry": {"x": 1}}},
                "urgency": {"type": "score", "instructions": "Urgent?", "criteria": ["later", "now"]},
            })
        );
    }

    #[test]
    fn noul_without_criteria_omits_them() {
        assert_eq!(
            serde_json::to_value(Question::noul(Value::Null)).unwrap(),
            json!({"type": "noul", "instructions": null})
        );
    }

    #[test]
    fn validates_question_sets() {
        assert!(validate(&Questions::new()).is_err());
        let one: Questions = [("s", Question::score("?", ["only"]))].into_iter().collect();
        assert_eq!(
            validate(&one).unwrap_err(),
            "Score question \"s\" has 1 criteria; at least two scores are required."
        );
    }
}
