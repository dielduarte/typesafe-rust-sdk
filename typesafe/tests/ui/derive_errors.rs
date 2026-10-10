use typesafe::{Choices, QuestionSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Choices)]
enum WithData {
    Calm(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Choices)]
enum Duplicate {
    Calm,
    #[choice(label = "calm")]
    AlsoCalm,
}

#[derive(QuestionSet)]
struct Tuple(typesafe::Noul);

#[derive(QuestionSet)]
struct Renamed {
    #[question(rename = "q")]
    a: typesafe::Noul,
    #[question(rename = "q")]
    b: typesafe::Noul,
}

fn main() {}
