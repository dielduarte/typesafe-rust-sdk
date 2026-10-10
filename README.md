# TypeSafe AI Rust SDK

Rust SDK for [TypeSafe AI](https://typesafe.ai): ask named questions about text or structured
state and get back probabilities, choices, and scores.

> Status: pre-release, not yet published to crates.io. The crate name is still to be decided,
> because `typesafe-sdk`, `typesafe-ai` and `typesafe-rust-sdk` are taken by unofficial crates.
> The library is imported as `typesafe`.

## Quickstart

```toml
[dependencies]
typesafe-rust-sdk = { git = "https://github.com/dielduarte/typesafe-rust-sdk" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Set `TYPESAFE_API_KEY`, then describe your questions as a struct. Every answer is typed by the
question that produced it:

```rust
use typesafe::{Choice, Choices, Client, Noul, QuestionSet, Score};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Choices)]
enum Category { Billing, Technical, Other }

#[derive(QuestionSet)]
struct Ticket {
    category: Choice<Category>,
    angry: Noul,
    urgency: Score<3>,
}

#[tokio::main]
async fn main() -> typesafe::Result<()> {
    let client = Client::new()?;
    let response = client
        .system_one("I was charged twice. Please fix this ASAP.", Ticket {
            category: Choice::new("What is this ticket about?"),
            angry: Noul::new("Is the customer angry?"),
            urgency: Score::new("How urgent is it?", ["can wait", "this week", "today"]),
        })
        .await?;

    let answers = response.answers; // TicketAnswers
    match answers.category.choice {
        Category::Billing => println!("billing, p = {:.2}", answers.category.probabilities[Category::Billing]),
        Category::Technical | Category::Other => println!("elsewhere"),
    }
    println!("angry: {:.2}, urgency: {:.2}/2", answers.angry.noul, answers.urgency.score);
    Ok(())
}
```

What the compiler checks for you:

- **Choice labels are an enum.** A label typo can't compile, and `match` must handle every label.
- **Score arity is part of the type.** `Score<3>` takes exactly three rubric entries, returns
  `[f64; 3]` probabilities, and `Score<1>` is rejected.
- **Answers are validated.** A response that doesn't match your questions becomes
  `Error::Decode` with the request ID, not a missing field later.

### Questions known only at runtime

```rust
use typesafe::Question;

let response = client
    .system_one(&ticket, [
        ("billing", Question::noul("Is this about billing?")),
        ("tone", Question::choice("Tone?", [("calm", serde_json::Value::Null), ("angry", "shouting".into())])),
    ])
    .await?;
let billing = response.answers.get("billing").and_then(|a| a.as_noul());
```

`state` can be anything `Serialize`: a `&str`, a `serde_json::Value`, or your own struct.

## Configuration

| Builder method | Environment variable | Default |
|---|---|---|
| `api_key` | `TYPESAFE_API_KEY` | required |
| `base_url` | `TYPESAFE_BASE_URL` | `https://api.typesafe.ai` |
| `default_model` | `TYPESAFE_DEFAULT_MODEL` | `jev-latest` |
| `timeout` | | 10s per attempt, including the full body |
| `retry_policy` / `max_retries` | | 2 retries on 408, 429, 5xx, timeouts and connection errors, with exponential backoff that honors `Retry-After` |
| `default_header` | | |
| `http_client` | | your own `reqwest::Client` (proxies, TLS roots, pools) |

Per call, chain `.model(..)`, `.timeout(..)`, `.max_retries(..)`, `.retry_policy(..)` or `.header(..)`
before awaiting. `.with_response()` also returns the status, headers and request ID.

Requests are lazy: nothing is sent until you `.await`. Dropping the future cancels the
request, including a pending retry, so `tokio::time::timeout` and `select!` work as expected.

## Errors

`typesafe::Error` is an enum: `Config`, `InvalidRequest`, `Api` (status, `ApiErrorKind`, body,
request ID, `retry_after`), `Timeout`, `Connection` and `Decode`.

## Features

- `derive` (default): `#[derive(QuestionSet)]` and `#[derive(Choices)]`.
- `tracing`: one span per request and events per attempt through `tracing`. Credential headers
  are redacted; bodies at `debug` are not.

## Development

```sh
cargo test --workspace                                                       # unit, mock-server, compile-fail and doc tests
TYPESAFE_API_KEY=... cargo test --test live -- --ignored --nocapture         # live API
cargo run --example demo                                                     # needs TYPESAFE_API_KEY
```

Minimum supported Rust version: 1.88.
