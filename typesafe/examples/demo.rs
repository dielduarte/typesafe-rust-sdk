//! Run with `cargo run --example demo`. Needs TYPESAFE_API_KEY in the environment.

use serde::Serialize;
use typesafe::{Choice, Choices, Client, Error, Noul, QuestionSet, Score};

#[derive(Serialize)]
struct Ticket {
    subject: &'static str,
    body: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Choices)]
enum Sentiment {
    Calm,
    Frustrated,
    Angry,
}

#[derive(QuestionSet)]
struct Triage {
    is_billing: Noul,
    sentiment: Choice<Sentiment>,
    urgency: Score<4>,
    refund_risk: Score<3>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let client = Client::new()?;

    let models = client.models().list().await?;
    let names: Vec<_> = models.iter().map(|m| m.name.as_str()).collect();
    println!("Available models: {}", names.join(", "));

    let ticket = Ticket {
        subject: "Charged twice this month",
        body: "Hi, I see two charges of $49 on my card for August. I only have one account. \
               Please fix this ASAP, I'm pretty frustrated.",
    };
    let questions = Triage {
        is_billing: Noul::new("Is this ticket about billing?"),
        sentiment: Choice::new("What is the customer's tone?"),
        urgency: Score::new("How urgent is this ticket?", ["can wait", "this week", "today", "right now"]),
        refund_risk: Score::new("How likely is the customer to demand a refund?", ["unlikely", "possible", "likely"]),
    };

    match client.system_one(&ticket, questions).await {
        Ok(response) => {
            // Every answer is typed by the question that produced it.
            let TriageAnswers { is_billing, sentiment, urgency, refund_risk } = response.answers;
            println!("billing?     {:.2}", is_billing.noul);
            println!("tone         {:?} ({:.2})", sentiment.choice, sentiment.probabilities[sentiment.choice]);
            println!("urgency      {:.2} on a 0-3 scale: {:?}", urgency.score, urgency.legend);
            println!("refund risk  {:.2} ({:.2} confidence)", refund_risk.score, refund_risk.confidence);
            println!("tokens       {} in / {} out", response.usage.input_tokens, response.usage.output_tokens);
        }
        Err(Error::Api(err)) => {
            eprintln!(
                "API error {} (request {}): {:?}",
                err.status,
                err.request_id.as_deref().unwrap_or("unknown"),
                err.body
            );
        }
        Err(other) => return Err(other),
    }
    Ok(())
}
