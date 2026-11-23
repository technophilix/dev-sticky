//! "Check for Updates…": asks GitHub for the latest DevSticky release.
//!
//! Runs only when the user picks the menu item; nothing is sent otherwise.

use anyhow::{Context, Result};
use std::time::Duration;

const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/technophilix/dev-sticky/releases/latest";

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, PartialEq)]
pub struct Release {
    /// Version without the leading "v", e.g. "0.4.0".
    pub version: String,
    /// The release page, where the packages can be downloaded.
    pub url: String,
}

impl Release {
    pub fn is_newer_than_running(&self) -> bool {
        is_newer(&self.version, CURRENT_VERSION)
    }
}

#[derive(serde::Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
}

/// Blocking: call it off the GTK main thread.
pub fn fetch_latest() -> Result<Release> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let body = agent
        .get(LATEST_RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", concat!("DevSticky/", env!("CARGO_PKG_VERSION")))
        .call()
        .context("contacting GitHub")?
        .body_mut()
        .read_to_string()
        .context("reading GitHub's reply")?;
    parse(&body)
}

fn parse(json: &str) -> Result<Release> {
    let r: ApiRelease = serde_json::from_str(json).context("unexpected reply from GitHub")?;
    Ok(Release {
        version: r.tag_name.trim_start_matches(['v', 'V']).to_string(),
        url: r.html_url,
    })
}

/// "1.2.3" (pre-release / build suffixes ignored) as numbers; `None` if malformed.
fn version_parts(v: &str) -> Option<Vec<u64>> {
    let core = v.trim_start_matches(['v', 'V']).split(['-', '+']).next()?;
    core.split('.').map(|p| p.parse().ok()).collect()
}

fn is_newer(latest: &str, current: &str) -> bool {
    let (Some(mut a), Some(mut b)) = (version_parts(latest), version_parts(current)) else {
        return false;
    };
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("0.3.2", "0.3.1"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("v1.0", "0.99.0"));
        assert!(!is_newer("0.3.1", "0.3.1"));
        assert!(!is_newer("0.3", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
        assert!(!is_newer("garbage", "0.3.0"));
        assert!(!is_newer("0.3.1-beta", "0.3.1"));
    }

    #[test]
    fn parses_github_reply() {
        let json = r#"{"tag_name":"v0.4.0","html_url":"https://github.com/technophilix/dev-sticky/releases/tag/v0.4.0","name":"x"}"#;
        assert_eq!(
            parse(json).unwrap(),
            Release {
                version: "0.4.0".into(),
                url: "https://github.com/technophilix/dev-sticky/releases/tag/v0.4.0".into(),
            }
        );
        assert!(parse("{}").is_err());
    }
}

#[cfg(test)]
mod live {
    #[test]
    #[ignore = "needs network"]
    fn fetches_latest_release() {
        let r = super::fetch_latest().unwrap();
        println!("latest: {r:?}, newer: {}", r.is_newer_than_running());
    }
}
