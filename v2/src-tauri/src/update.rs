//! Version check against the project's GitHub release and jsDelivr manifest.
//!
//! The manifest is preferred because GitHub API/raw endpoints can be slow or
//! blocked on some networks; the GitHub API is the fallback.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const MANIFEST_URL: &str =
    "https://cdn.jsdelivr.net/gh/vlln/xiaomi-remote-voice-coding-adapter@main/v2/update.json";
const API_URL: &str =
    "https://api.github.com/repos/vlln/xiaomi-remote-voice-coding-adapter/releases/latest";

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub available: bool,
    pub url: String,
    pub asset_url: Option<String>,
    pub asset_name: Option<String>,
    pub notes: Option<String>,
    pub source: String,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    version: String,
    url: String,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    assets: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub async fn check() -> Result<UpdateInfo, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let client = reqwest::Client::builder()
        .user_agent(format!("Xiaomi-Remote/{current}"))
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| format!("无法创建更新客户端: {e}"))?;

    if let Ok(info) = from_manifest(&client, &current).await {
        if info.available || info.latest != "unknown" {
            return Ok(info);
        }
    }
    from_github(&client, &current).await
}

async fn from_manifest(client: &reqwest::Client, current: &str) -> Result<UpdateInfo, String> {
    let response = client
        .get(MANIFEST_URL)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let manifest: Manifest = response.json().await.map_err(|e| e.to_string())?;
    let latest = normalize_version(&manifest.version);
    let asset_key = platform_asset_key();
    let asset = manifest.assets.get(asset_key).cloned();
    Ok(UpdateInfo {
        current: current.to_string(),
        available: is_newer(current, &latest),
        latest,
        url: manifest.url,
        asset_name: asset.as_ref().and_then(|url| {
            url.rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
                .map(ToString::to_string)
        }),
        asset_url: asset,
        notes: manifest.notes.map(|notes| truncate(&notes, 500)),
        source: "manifest".into(),
    })
}

async fn from_github(client: &reqwest::Client, current: &str) -> Result<UpdateInfo, String> {
    let response = client
        .get(API_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("无法连接 GitHub: {e}"))?
        .error_for_status()
        .map_err(|e| format!("GitHub 返回错误: {e}"))?;
    let release: GithubRelease = response
        .json()
        .await
        .map_err(|e| format!("解析 GitHub 版本信息失败: {e}"))?;
    let latest = normalize_version(&release.tag_name);
    let asset = select_github_asset(&release.assets);
    Ok(UpdateInfo {
        current: current.to_string(),
        available: is_newer(current, &latest),
        latest,
        url: release.html_url,
        asset_name: asset.as_ref().map(|asset| asset.name.clone()),
        asset_url: asset.map(|asset| asset.browser_download_url),
        notes: release.body.map(|notes| truncate(&notes, 500)),
        source: "github".into(),
    })
}

fn select_github_asset(assets: &[GithubAsset]) -> Option<GithubAsset> {
    let suffix = platform_suffix();
    assets
        .iter()
        .find(|asset| asset.name.ends_with(suffix))
        .map(|asset| GithubAsset {
            name: asset.name.clone(),
            browser_download_url: asset.browser_download_url.clone(),
        })
}

fn platform_asset_key() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        "other"
    }
}

fn platform_suffix() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        ".dmg"
    }
    #[cfg(target_os = "windows")]
    {
        ".exe"
    }
    #[cfg(target_os = "linux")]
    {
        ".deb"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        ""
    }
}

fn normalize_version(version: &str) -> String {
    version.trim().trim_start_matches('v').to_string()
}

fn is_newer(current: &str, latest: &str) -> bool {
    let Ok(current) = semver::Version::parse(normalize_version(current).as_str()) else {
        return false;
    };
    let Ok(latest) = semver::Version::parse(normalize_version(latest).as_str()) else {
        return false;
    };
    latest > current
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("2.5.10", "2.5.11"));
        assert!(is_newer("v2.5.10", "v2.6.0"));
        assert!(!is_newer("2.5.11", "2.5.11"));
        assert!(!is_newer("2.5.11", "2.5.10"));
    }

    #[test]
    fn truncates_unicode_safely() {
        assert_eq!(truncate("遥控器更新", 3), "遥控器…");
    }
}
