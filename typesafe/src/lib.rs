//! Rust SDK for the [TypeSafe AI](https://typesafe.ai) API.
//!
//! ```no_run
//! use typesafe::{Choice, Choices, Client, QuestionSet};
//!
//! #[derive(Debug, Clone, Copy, PartialEq, Eq, Choices)]
//! enum Category { Billing, Technical, Other }
//!
//! #[derive(QuestionSet)]
//! struct Ticket {
//!     category: Choice<Category>,
//! }
//!
//! # async fn run() -> typesafe::Result<()> {
//! let client = Client::new()?; // reads TYPESAFE_API_KEY
//! let response = client
//!     .system_one("I was charged twice. Please fix this ASAP.", Ticket {
//!         category: Choice::new("What is this ticket about?"),
//!     })
//!     .await?;
//!
//! if response.answers.category.choice == Category::Billing {
//!     println!("routing to billing");
//! }
//! # Ok(()) }
//! ```
//!
//! For questions only known at runtime, pass `(name, Question)` pairs and read answers from
//! [`Answers`] by name; see [`Client::system_one`].
//!
//! # Features
//!
//! - `derive` (default): `#[derive(QuestionSet)]` and `#[derive(Choices)]`.
//! - `tracing`: request spans and events through [`tracing`](https://docs.rs/tracing). `info`
//!   logs one line per attempt; `debug` adds headers, with credentials redacted, and bodies,
//!   which are not redacted.

#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate self as typesafe;

mod client;
mod error;
mod logging;
mod models;
mod request;
mod retry;
pub mod typed;
pub mod types;

pub use client::{Client, ClientBuilder, DEFAULT_BASE_URL, DEFAULT_MODEL, env};
pub use error::{ApiError, ApiErrorKind, DecodeError, Error, REQUEST_ID_HEADER, Result};
pub use models::Models;
pub use request::{ApiRequest, VERSION, WithResponse};
/// The HTTP client used by the SDK, for building a custom client with [`ClientBuilder::http_client`].
pub use reqwest;
pub use retry::{DEFAULT_TIMEOUT, RetryPolicy};
pub use typed::{Choice, ChoiceAnswer, Choices, Noul, Probabilities, QuestionSet, Score, ScoreAnswer, TypedQuestion};
pub use types::{
    Answer, Answers, ChoiceResult, ModelCard, NoulAnswer, Question, Questions, ScoreResult, SystemOneResponse, Usage,
};
/// Derive [`Choices`] for a fieldless enum.
///
/// Labels default to the variant name in `snake_case`; override with `#[choice(label = "...")]`.
#[cfg(feature = "derive")]
pub use typesafe_macros::Choices;
/// Derive [`QuestionSet`] for a struct of [`TypedQuestion`] fields, generating a
/// `<Name>Answers` struct with one typed answer per field.
///
/// Question names default to the field name; override with `#[question(rename = "...")]`.
#[cfg(feature = "derive")]
pub use typesafe_macros::QuestionSet;

#[doc(hidden)]
pub mod __private {
    pub use serde;
}
