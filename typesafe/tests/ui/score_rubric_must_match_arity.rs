use typesafe::{Noul, QuestionSet, Score};

#[derive(QuestionSet)]
struct Triage {
    billing: Noul,
    urgency: Score<3>,
}

fn main() {
    let _ = Triage { billing: Noul::new("Billing?"), urgency: Score::new("Urgent?", ["later", "now"]) };
}
