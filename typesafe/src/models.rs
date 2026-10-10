//! The Models API.

use reqwest::Method;
use serde::Deserialize;

use crate::client::Client;
use crate::request::ApiRequest;
use crate::types::ModelCard;

/// Access to the Models API, from [`Client::models`].
#[derive(Debug, Clone, Copy)]
pub struct Models<'a> {
    client: &'a Client,
}

impl<'a> Models<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// List the models available to the account.
    pub fn list(&self) -> ApiRequest<Vec<ModelCard>> {
        #[derive(Deserialize)]
        struct Wire {
            models: Vec<ModelCard>,
        }
        ApiRequest::new(self.client.clone(), Method::GET, "/v1/models", |bytes| {
            serde_json::from_slice::<Wire>(bytes).map(|w| w.models)
        })
    }
}
