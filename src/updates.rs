//! Stable-release checks, verified downloads and installation on quit.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(windows)]
mod windows;

const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/Master0fFate/oxidify/releases/latest";
const RELEASE_PAGES: &str = "https://github.com/Master0fFate/oxidify/releases/tag";
const DOWNLOAD_BASE: &str = "https://github.com/Master0fFate/oxidify/releases/download";
const USER_AGENT: &str = concat!("Oxidify/", env!("CARGO_PKG_VERSION"));
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_PACKAGE_BYTES: u64 = 250 * 1024 * 1024;
// v0.8.4 and older Inno installers lack the wait and installed-version checks.
const LAST_LEGACY_WINDOWS_RELEASE: &str = "0.8.4";

/// How often a running app asks again.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallTarget {
    #[cfg(target_os = "linux")]
    Linux(PathBuf),
    #[cfg(target_os = "macos")]
    Macos(PathBuf),
    #[cfg(windows)]
    Windows(PathBuf),
}

impl InstallTarget {
    /// Identify a direct install that this copy can safely update.
    pub fn detect() -> Option<Self> {
        // Keep source and other downstream builds on their own update channel.
        if option_env!("OXIDIFY_RELEASE_BUILD") != Some("1")
            || cfg!(debug_assertions)
            || cfg!(feature = "demo")
        {
            return None;
        }
        let executable = std::env::current_exe().ok()?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let target = unix::detect(&executable);
        #[cfg(windows)]
        let target = windows::detect(&executable);
        target
    }

    fn asset_name(&self, version: &str) -> Option<String> {
        asset_name(std::env::consts::OS, std::env::consts::ARCH, version)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub asset: Option<Asset>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    name: String,
    size: u64,
    sha256: String,
}

#[cfg(any(test, feature = "demo"))]
impl Asset {
    pub fn demo() -> Self {
        Self {
            name: "demo".into(),
            size: 0,
            sha256: String::new(),
        }
    }
}

#[derive(Deserialize)]
struct UploadedAsset {
    name: String,
    state: String,
    size: u64,
    digest: Option<String>,
}

impl UploadedAsset {
    fn verify_metadata(self) -> Option<Asset> {
        if self.state != "uploaded" || !(1..=MAX_PACKAGE_BYTES).contains(&self.size) {
            return None;
        }
        let digest = self.digest.as_deref()?.strip_prefix("sha256:")?;
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        Some(Asset {
            name: self.name,
            size: self.size,
            sha256: digest.to_ascii_lowercase(),
        })
    }
}

#[derive(Deserialize)]
struct LatestRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    #[serde(default)]
    assets: Vec<UploadedAsset>,
}

impl LatestRelease {
    fn newer_than(
        self,
        current: &str,
        asset_for: impl FnOnce(&str) -> Option<String>,
    ) -> Option<Release> {
        if self.draft || self.prerelease {
            return None;
        }
        let version = self.tag_name.strip_prefix('v')?;
        if !is_newer(version, current) {
            return None;
        }
        let asset = asset_for(version).and_then(|name| {
            let mut matches = self.assets.into_iter().filter(|asset| asset.name == name);
            let candidate = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            candidate.verify_metadata()
        });
        // Build official URLs from the validated tag and known asset name.
        Some(Release {
            version: version.to_string(),
            url: format!("{RELEASE_PAGES}/{}", self.tag_name),
            asset,
        })
    }
}

/// The newest stable release, when it is newer than this build.
pub async fn newer_release(
    http: &reqwest::Client,
    target: Option<InstallTarget>,
) -> Result<Option<Release>> {
    let mut response = http
        .get(LATEST_RELEASE_URL)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", USER_AGENT)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await?
        .error_for_status()?;
    ensure!(
        response.url().scheme() == "https" && response.url().host_str() == Some("api.github.com"),
        "release listing redirected outside the official GitHub API"
    );
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
    Ok(latest.newer_than(env!("CARGO_PKG_VERSION"), |version| {
        target?.asset_name(version)
    }))
}

fn asset_name(os: &str, arch: &str, version: &str) -> Option<String> {
    if !matches!(arch, "x86_64" | "aarch64") || parse(version).is_none() {
        return None;
    }
    let suffix = match os {
        "linux" => format!("{arch}-unknown-linux-gnu.tar.gz"),
        "macos" => "macos-universal.dmg".into(),
        "windows" if is_newer(version, LAST_LEGACY_WINDOWS_RELEASE) => {
            format!("{arch}-pc-windows-msvc-setup.exe")
        }
        _ => return None,
    };
    Some(format!("oxidify-v{version}-{suffix}"))
}

/// Download the installer selected for this installation and prepare it off the UI thread.
pub async fn download(
    http: &reqwest::Client,
    release: &Release,
    target: InstallTarget,
) -> Result<PreparedUpdate> {
    ensure!(
        is_newer(&release.version, env!("CARGO_PKG_VERSION")),
        "the release is no longer newer than Oxidify"
    );
    let asset = release
        .asset
        .as_ref()
        .context("no compatible release asset")?;
    ensure!(
        target.asset_name(&release.version).as_ref() == Some(&asset.name),
        "release asset does not match this installation"
    );
    let staging = tokio::task::spawn_blocking(new_download_directory)
        .await?
        .context("could not create download directory")?;
    let package = staging.path().join(&asset.name);
    let url = format!("{DOWNLOAD_BASE}/v{}/{}", release.version, asset.name);
    fetch_package(http, &url, asset, &package).await?;

    #[cfg(windows)]
    {
        let InstallTarget::Windows(directory) = target;
        Ok(PreparedUpdate {
            staging,
            directory,
            package,
            version: release.version.clone(),
        })
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let version = release.version.clone();
        tokio::task::spawn_blocking(move || {
            let update = unix::prepare(&target, &package, &version)?;
            drop(staging);
            Ok(PreparedUpdate { update })
        })
        .await?
    }
}

fn new_download_directory() -> std::io::Result<tempfile::TempDir> {
    #[cfg(windows)]
    {
        let cache = crate::paths::AppDirs::discover().cache.join("updates");
        std::fs::create_dir_all(&cache)?;
        // A Windows installer keeps its downloaded EXE open until it exits.
        // Remove old staging directories on the next update, after a grace period.
        if let Ok(entries) = std::fs::read_dir(&cache) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with("download-")
                    && entry.file_type().is_ok_and(|kind| kind.is_dir())
                    && entry
                        .metadata()
                        .and_then(|metadata| metadata.modified())
                        .is_ok_and(|modified| {
                            modified.elapsed().is_ok_and(|age| age > 7 * CHECK_INTERVAL)
                        })
                {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
        tempfile::Builder::new()
            .prefix("download-")
            .tempdir_in(cache)
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    tempfile::Builder::new().prefix("oxidify-update-").tempdir()
}

fn is_release_download(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.port().is_none()
        && matches!(
            url.host_str(),
            Some(
                "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
}

async fn fetch_package(
    http: &reqwest::Client,
    url: &str,
    asset: &Asset,
    path: &Path,
) -> Result<()> {
    let mut response = http
        .get(url)
        .header("User-Agent", USER_AGENT)
        .timeout(DOWNLOAD_TIMEOUT)
        .send()
        .await
        .map_err(reqwest::Error::without_url)?
        .error_for_status()
        .map_err(reqwest::Error::without_url)?;
    ensure!(
        is_release_download(response.url()),
        "download redirected outside GitHub release hosting"
    );
    ensure!(
        response
            .content_length()
            .is_none_or(|size| size == asset.size),
        "download size does not match its release"
    );
    let mut file = tokio::fs::File::create_new(path).await?;
    let mut hasher = Sha256::new();
    let mut received = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(reqwest::Error::without_url)?
    {
        received += chunk.len() as u64;
        ensure!(received <= asset.size, "download is larger than expected");
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    verify_download(asset, received, hasher)?;
    file.flush().await?;
    file.sync_all().await?;
    Ok(())
}

fn verify_download(asset: &Asset, size: u64, hasher: Sha256) -> Result<()> {
    ensure!(size == asset.size, "download is incomplete");
    ensure!(
        format!("{:x}", hasher.finalize()) == asset.sha256,
        "download did not pass its SHA-256 check"
    );
    Ok(())
}

/// A verified update held until the app shuts down.
pub struct PreparedUpdate {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    update: unix::PlatformUpdate,
    #[cfg(windows)]
    staging: tempfile::TempDir,
    #[cfg(windows)]
    directory: PathBuf,
    #[cfg(windows)]
    package: PathBuf,
    #[cfg(windows)]
    version: String,
}

impl std::fmt::Debug for PreparedUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedUpdate").finish_non_exhaustive()
    }
}

impl PreparedUpdate {
    /// Install or hand off a verified update after the application shuts down.
    pub fn install_on_exit(self) -> Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.update.install()
        }
        #[cfg(windows)]
        {
            windows::install(&self.directory, &self.package, &self.version)?;
            let _ = self.staging.keep();
            Ok(())
        }
    }
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
        let notice = listing(false, false).newer_than("1.2.2", |_| None).unwrap();
        assert_eq!(notice.version, "1.2.3");
        assert_eq!(
            notice.url,
            "https://github.com/Master0fFate/oxidify/releases/tag/v1.2.3"
        );
        assert!(notice.asset.is_none());
        assert!(listing(true, false).newer_than("1.2.2", |_| None).is_none());
        assert!(listing(false, true).newer_than("1.2.2", |_| None).is_none());
        assert!(
            listing(false, false)
                .newer_than("1.2.3", |_| None)
                .is_none()
        );
        assert!(
            listing(false, false)
                .newer_than("2.0.0", |_| None)
                .is_none()
        );
    }

    #[test]
    fn packages_match_the_running_architecture_and_install_type() {
        for arch in ["x86_64", "aarch64"] {
            assert_eq!(
                asset_name("windows", arch, "1.2.3").unwrap(),
                format!("oxidify-v1.2.3-{arch}-pc-windows-msvc-setup.exe")
            );
            assert_eq!(
                asset_name("linux", arch, "1.2.3").unwrap(),
                format!("oxidify-v1.2.3-{arch}-unknown-linux-gnu.tar.gz")
            );
            assert_eq!(
                asset_name("macos", arch, "1.2.3").unwrap(),
                "oxidify-v1.2.3-macos-universal.dmg"
            );
        }
        assert!(asset_name("linux", "arm", "1.2.3").is_none());
        assert!(asset_name("windows", "x86_64", "0.8.4").is_none());
        assert!(asset_name("macos", "aarch64", "../escape").is_none());
    }

    #[test]
    fn asset_selection_requires_one_exact_uploaded_package_with_a_digest() {
        let name = "oxidify-v1.2.3-x86_64-pc-windows-msvc-setup.exe";
        let digest = format!("sha256:{}", "A".repeat(64));
        let select = |assets| {
            serde_json::from_value::<LatestRelease>(serde_json::json!({
                "tag_name": "v1.2.3", "draft": false, "prerelease": false, "assets": assets
            }))
            .unwrap()
            .newer_than("1.2.2", |_| Some(name.into()))
            .unwrap()
            .asset
        };
        let entry = |name: &str, state: &str, size: u64, digest: Option<&str>| serde_json::json!({"name": name, "state": state, "size": size, "digest": digest});
        let proper = entry(name, "uploaded", 12, Some(&digest));
        let wrong_arch = entry(
            &name.replace("x86_64", "aarch64"),
            "uploaded",
            12,
            Some(&digest),
        );
        assert_eq!(
            select(vec![wrong_arch.clone(), proper.clone()])
                .unwrap()
                .sha256,
            "a".repeat(64)
        );
        assert!(select(vec![wrong_arch]).is_none());
        assert!(select(vec![proper.clone(), proper]).is_none());
        for bad in [
            entry(name, "starter", 12, Some(&digest)),
            entry(name, "uploaded", 0, Some(&digest)),
            entry(name, "uploaded", MAX_PACKAGE_BYTES + 1, Some(&digest)),
            entry(name, "uploaded", 12, None),
            entry(name, "uploaded", 12, Some("sha256:broken")),
        ] {
            assert!(select(vec![bad]).is_none());
        }
    }

    #[test]
    fn download_verification_rejects_partial_and_modified_files() {
        let bytes = b"installer";
        let asset = Asset {
            name: "installer".into(),
            size: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        };
        assert!(verify_download(&asset, asset.size, Sha256::new_with_prefix(bytes)).is_ok());
        assert!(verify_download(&asset, asset.size - 1, Sha256::new_with_prefix(bytes)).is_err());
        assert!(
            verify_download(&asset, asset.size, Sha256::new_with_prefix(b"corrupted")).is_err()
        );
    }

    #[test]
    fn download_redirects_require_official_https_hosts() {
        for url in [
            "https://github.com/releases/download/asset",
            "https://release-assets.githubusercontent.com/github-production-release-asset/",
        ] {
            assert!(is_release_download(&url.parse().unwrap()));
        }
        for url in [
            "https://github.com.evil.example/",
            "https://github.com:8080/",
            "http://github.com/",
            "https://evil.example/?host=github.com",
        ] {
            assert!(!is_release_download(&url.parse().unwrap()));
        }
    }
}
