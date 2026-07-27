//! Check for new releases on GitHub.

use leptos::prelude::*;

/// The current version baked in at compile time.
#[cfg(feature = "ssr")]
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Result of an update check.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpdateInfo {
    /// Latest release tag (e.g. "v0.2.0").
    pub latest_tag: String,
    /// Whether the latest version is newer than the running build.
    pub update_available: bool,
    /// URL to the release page.
    pub release_url: String,
}

/// Fetch the latest release tag from GitHub and compare to the running version.
#[server(CheckForUpdate, "/api")]
pub async fn check_for_update() -> Result<Option<UpdateInfo>, ServerFnError> {
    let client = reqwest::Client::builder()
        .user_agent("nanorp-update-check")
        .build()
        .map_err(|e| ServerFnError::new(format!("http client: {e}")))?;

    let resp = client
        .get("https://api.github.com/repos/coder3101/nanorp/releases/latest")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| ServerFnError::new(format!("GitHub request failed: {e}")))?;

    if !resp.status().is_success() {
        // Don't treat a failed check as an error — just return None.
        tracing::debug!("GitHub release check returned {}", resp.status());
        return Ok(None);
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ServerFnError::new(format!("parse GitHub response: {e}")))?;

    let tag = body["tag_name"]
        .as_str()
        .unwrap_or("")
        .trim_start_matches('v')
        .to_string();

    let html_url = body["html_url"]
        .as_str()
        .unwrap_or("https://github.com/coder3101/nanorp/releases")
        .to_string();

    if tag.is_empty() {
        return Ok(None);
    }

    let update_available = version_is_newer(&tag, CURRENT_VERSION);

    Ok(Some(UpdateInfo {
        latest_tag: format!("v{tag}"),
        update_available,
        release_url: html_url,
    }))
}

/// Simple semver comparison: returns `true` if `latest` > `current`.
#[cfg(feature = "ssr")]
fn version_is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let l = parse(latest);
    let c = parse(current);
    l > c
}
