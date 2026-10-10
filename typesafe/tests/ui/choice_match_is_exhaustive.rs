use typesafe::{Choice, Choices, QuestionSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Choices)]
enum Tone {
    Calm,
    Angry,
}

#[derive(QuestionSet)]
struct Triage {
    tone: Choice<Tone>,
}

fn route(answers: TriageAnswers) -> &'static str {
    match answers.tone.choice {
        Tone::Calm => "queue",
    }
}

fn main() {}
