//! Shared request deadlines. Streaming responses deliberately have no total
//! request timeout: only connection/response startup and gaps between chunks
//! are bounded, so a healthy long-running completion is never cut off.

use anyhow::{anyhow, Context, Result};
use reqwest::{RequestBuilder, Response};
use std::time::Duration;

pub(crate) const MODELS_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const TEST_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const JSON_REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
pub(crate) const STREAM_FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(45);
pub(crate) const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// Send a finite request with a deadline covering headers and response body.
pub(crate) async fn send(request: RequestBuilder, timeout: Duration) -> Result<Response> {
    request
        .timeout(timeout)
        .send()
        .await
        .with_context(|| format!("HTTP request failed ({} second limit)", timeout.as_secs()))
}

/// Wait for response headers before handing the body to a streaming parser.
pub(crate) async fn start_stream(request: RequestBuilder) -> Result<Response> {
    match tokio::time::timeout(STREAM_FIRST_BYTE_TIMEOUT, request.send()).await {
        Ok(result) => result.context("HTTP streaming request failed"),
        Err(_) => Err(anyhow!(
            "Provider did not start responding within {} seconds",
            STREAM_FIRST_BYTE_TIMEOUT.as_secs()
        )),
    }
}

pub(crate) fn connection_error(base_url: &str, error: &anyhow::Error) -> String {
    let message = format!("{error:#}");
    if message.contains("timed out") || message.contains("timeout") {
        format!(
            "Timed out contacting {base_url}. Check that the endpoint is reachable and not overloaded: {message}"
        )
    } else {
        format!(
            "Could not contact {base_url}. Check the API URL, network, and TLS configuration: {message}"
        )
    }
}
