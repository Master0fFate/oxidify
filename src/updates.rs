//! Stable-release checks against the official Oxidify repository.

use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/Master0fFate/oxidify/releases/latest";
const RELEASE_PAGES: &str = "https://github.com/Master0fFate/oxidify/releases/tag";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// How often a running app asks again.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
}

#[derive(Deserialize)]
struct LatestRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}

impl LatestRelease {
    fn newer_than(self, current: &str) -> Option<Release> {
        if self.draft || self.prerelease {
            return None;
        }
        let version = self.tag_name.strip_prefix('v')?;
        if !is_newer(version, current) {
            return None;
        }
        // Never open a URL supplied in a remote release description or asset.
        Some(Release {
            version: version.to_string(),
            url: format!("{RELEASE_PAGES}/{}", self.tag_name),
        })
    }
}

/// The newest stable release, when it is newer than this build.
pub async fn newer_release(http: &reqwest::Client) -> Result<Option<Release>> {
    let mut response = http
        .get(LATEST_RELEASE_URL)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", concat!("Oxidify/", env!("CARGO_PKG_VERSION")))
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await?
        .error_for_status()?;
    ensure!(
        response
            .content_length()
            .is_none_or(|size| size <= MAX_RESPONSE_BYTES as u64),
        "release listing is too large"
    );
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            body.len() + chunk.len() <= MAX_RESPONSE_BYTES,
            "release listing is too large"
        );
        body.extend_from_slice(&chunk);
    }
    let latest: LatestRelease =
        serde_json::from_slice(&body).context("unexpected release listing")?;
    Ok(latest.newer_than(env!("CARGO_PKG_VERSION")))
}

fn parse(version: &str) -> Option<[u64; 3]> {
    let mut parts = version.split('.');
    let mut numbers = [0; 3];
    for number in &mut numbers {
        let part = parts.next()?;
        if part.is_empty()
            || !part.bytes().all(|byte| byte.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        *number = part.parse().ok()?;
    }
    parts.next().is_none().then_some(numbers)
}

/// Invalid versions and prereleases are never announced.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse(candidate), parse(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.1.4", "0.1.3"));
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(!is_newer("0.1.3", "0.1.3"));
        assert!(!is_newer("0.1.2", "0.1.3"));
    }

    #[test]
    fn malformed_versions_and_prereleases_are_rejected() {
        for version in [
            "0.2.0-rc1",
            "nightly",
            "1.2.3.4",
            "1.2",
            "1.2.3+build",
            "01.2.3",
            "+1.2.3",
            " 1.2.3",
            "1.2.3/../../elsewhere",
            "18446744073709551616.0.0",
        ] {
            assert!(parse(version).is_none(), "{version}");
            assert!(!is_newer(version, "0.1.0"));
            assert!(!is_newer("9.0.0", version));
        }
    }

    #[test]
    fn notices_use_only_stable_releases_and_the_official_page() {
        let listing = |draft, prerelease| {
            serde_json::from_value::<LatestRelease>(serde_json::json!({
                "tag_name": "v1.2.3",
                "draft": draft,
                "prerelease": prerelease,
                "html_url": "https://untrusted.example/download.exe"
            }))
            .unwrap()
        };
        let notice = listing(false, false).newer_than("1.2.2").unwrap();
        assert_eq!(notice.version, "1.2.3");
        assert_eq!(
            notice.url,
            "https://github.com/Master0fFate/oxidify/releases/tag/v1.2.3"
        );
        assert!(listing(true, false).newer_than("1.2.2").is_none());
        assert!(listing(false, true).newer_than("1.2.2").is_none());
        assert!(listing(false, false).newer_than("1.2.3").is_none());
        assert!(listing(false, false).newer_than("2.0.0").is_none());
    }
}
