use typesafe::{QuestionSet, Score};

#[derive(QuestionSet)]
struct OneLevel {
    urgency: Score<1>,
}

fn main() {}
