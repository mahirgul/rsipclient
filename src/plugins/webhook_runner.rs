//! Asynchronous HTTP Webhook Dispatcher
//!
//! Sends JSON event payloads to configured Webhook HTTP endpoints and parses
//! returned dynamic action responses for IVR sessions.

use super::types::{PluginActionResult, PluginEvent, WebhookPluginConfig};
use reqwest::Client;
use std::collections::HashMap;
use std::time::Duration;

/// Async Webhook Dispatcher
#[allow(dead_code)]
pub struct WebhookRunner {
    client: Client,
}

#[allow(dead_code)]
impl WebhookRunner {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self { client }
    }

    /// Dispatch an event payload to a webhook endpoint asynchronously
    pub async fn dispatch_event(&self, config: &WebhookPluginConfig, event: &PluginEvent) {
        if !config.enabled {
            return;
        }

        let event_type = event.event_type();
        if !config.events.iter().any(|e| e == "*" || e == event_type) {
            return; // Event type not subscribed
        }

        let mut req = self.client.post(&config.url);

        if let Some(ref headers) = config.headers {
            for (k, v) in headers {
                req = req.header(k, v);
            }
        }

        if let Some(timeout_ms) = config.timeout_ms {
            req = req.timeout(Duration::from_millis(timeout_ms));
        }

        let url = config.url.clone();
        let name = config.name.clone();

        match req.json(event).send().await {
            Ok(resp) => {
                log::info!(
                    "[Webhook Plugin '{}'] Posted event '{}' to {} (HTTP {})",
                    name,
                    event_type,
                    url,
                    resp.status()
                );
            }
            Err(e) => {
                log::warn!(
                    "[Webhook Plugin '{}'] Failed to post event '{}' to {}: {}",
                    name,
                    event_type,
                    url,
                    e
                );
            }
        }
    }

    /// Dispatch IVR Webhook query and parse returned JSON action
    pub async fn dispatch_ivr_webhook(
        &self,
        url: &str,
        context: &HashMap<String, String>,
    ) -> Result<PluginActionResult, String> {
        let resp = self
            .client
            .post(url)
            .json(context)
            .send()
            .await
            .map_err(|e| format!("Webhook request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Webhook HTTP error: Status {}", resp.status()));
        }

        let action_res: PluginActionResult = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse webhook JSON action response: {}", e))?;

        Ok(action_res)
    }
}
