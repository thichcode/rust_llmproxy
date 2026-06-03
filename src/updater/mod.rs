use serde::Deserialize;
use tracing::info;

use crate::error::AppError;

const GITHUB_REPO: &str = "thichcode/rust_llmproxy";

#[derive(Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    #[serde(default)]
    pub browser_download_url: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub size: Option<u64>,
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub async fn fetch_latest_release() -> Result<GitHubRelease, AppError> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", GITHUB_REPO);
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("User-Agent", "mini-ai-router-rs/0.1.0")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| AppError::Provider(format!("Failed to fetch latest release: {}", e)))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Provider(format!(
            "GitHub API returned {}: {}",
            status,
            body
        )));
    }

    let release: GitHubRelease = response.json().await.map_err(|e| {
        AppError::Provider(format!("Failed to parse release response: {}", e))
    })?;

    Ok(release)
}

pub fn compare_versions(current: &str, latest: &str) -> Result<Option<bool>, AppError> {
    let cur = semver::Version::parse(current.trim_start_matches('v')).map_err(|e| {
        AppError::Config(format!("Failed to parse current version '{}': {}", current, e))
    })?;
    let lat = semver::Version::parse(latest.trim_start_matches('v')).map_err(|e| {
        AppError::Config(format!("Failed to parse latest version '{}': {}", latest, e))
    })?;

    if lat > cur {
        Ok(Some(true))
    } else {
        Ok(Some(false))
    }
}

pub async fn download_exe(release: &GitHubRelease) -> Result<Vec<u8>, AppError> {
    let asset_name = format!("mini-ai-router-rs.exe");
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| {
            AppError::Provider(format!(
                "No '{}' found in release {}",
                asset_name, release.tag_name
            ))
        })?;

    let url = &asset.browser_download_url;
    info!("Downloading {} from {}", asset_name, url);

    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header("User-Agent", "mini-ai-router-rs/0.1.0")
        .send()
        .await
        .map_err(|e| AppError::Provider(format!("Download failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(AppError::Provider(format!(
            "Download returned {}",
            response.status()
        )));
    }

    let bytes = response.bytes().await.map_err(|e| {
        AppError::Provider(format!("Failed to read download: {}", e))
    })?;

    Ok(bytes.to_vec())
}

pub fn apply_update(new_exe: &[u8]) -> Result<(), AppError> {
    let current_exe = std::env::current_exe().map_err(|e| {
        AppError::Config(format!("Cannot determine current executable path: {}", e))
    })?;

    let backup_path = current_exe.with_extension("exe.bak");
    let update_temp = current_exe.with_extension("exe.new");

    std::fs::write(&update_temp, new_exe).map_err(|e| {
        AppError::Config(format!("Failed to write update file: {}", e))
    })?;

    if backup_path.exists() {
        std::fs::remove_file(&backup_path).ok();
    }
    std::fs::rename(&current_exe, &backup_path).map_err(|e| {
        AppError::Config(format!("Failed to backup current executable: {}", e))
    })?;

    std::fs::rename(&update_temp, &current_exe).map_err(|e| {
        AppError::Config(format!("Failed to replace executable: {}", e))
    })?;

    info!("Update applied. Backup saved as: {:?}", backup_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_versions_newer() {
        let result = compare_versions("0.1.0", "0.2.0").unwrap();
        assert_eq!(result, Some(true));
    }

    #[test]
    fn test_compare_versions_older() {
        let result = compare_versions("0.2.0", "0.1.0").unwrap();
        assert_eq!(result, Some(false));
    }

    #[test]
    fn test_compare_versions_equal() {
        let result = compare_versions("0.1.0", "0.1.0").unwrap();
        assert_eq!(result, Some(false));
    }

    #[test]
    fn test_compare_versions_with_v_prefix() {
        let result = compare_versions("0.1.0", "v0.2.0").unwrap();
        assert_eq!(result, Some(true));
    }

    #[test]
    fn test_compare_versions_major() {
        let result = compare_versions("1.0.0", "2.0.0").unwrap();
        assert_eq!(result, Some(true));
    }

    #[test]
    fn test_current_version_parses() {
        let ver = semver::Version::parse(current_version().trim_start_matches('v'));
        assert!(ver.is_ok());
    }
}
