use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubTokenData {
    pub github_access_token: String,
    pub token_type: String,
    pub scope: Option<String>,
    pub created_at: u64,
}

pub struct TokenStore {
    path: PathBuf,
}

impl TokenStore {
    pub fn new() -> Result<Self, AppError> {
        let base = directories::ProjectDirs::from("com", "mini-ai-router-rs", "mini-ai-router-rs")
            .ok_or_else(|| AppError::Config("Cannot determine config directory".to_string()))?;
        let data_dir = base.data_dir().to_path_buf();
        Ok(TokenStore {
            path: data_dir.join("copilot_token.json"),
        })
    }

    #[allow(dead_code)]
    pub fn from_path(path: PathBuf) -> Self {
        TokenStore { path }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn load(&self) -> Result<Option<GithubTokenData>, AppError> {
        if !self.path.exists() {
            return Ok(None);
        }
        let contents = std::fs::read_to_string(&self.path)
            .map_err(|e| AppError::Config(format!("Failed to read token file: {}", e)))?;
        let data: GithubTokenData = serde_json::from_str(&contents)
            .map_err(|e| AppError::Config(format!("Failed to parse token file: {}", e)))?;
        Ok(Some(data))
    }

    pub fn save(&self, data: &GithubTokenData) -> Result<(), AppError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::Config(format!("Failed to create config directory: {}", e))
            })?;
        }

        let contents = serde_json::to_string_pretty(data)
            .map_err(|e| AppError::Config(format!("Failed to serialize token: {}", e)))?;

        std::fs::write(&self.path, &contents)
            .map_err(|e| AppError::Config(format!("Failed to write token file: {}", e)))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = std::fs::metadata(&self.path) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o600);
                let _ = std::fs::set_permissions(&self.path, perms);
            }
        }

        Ok(())
    }

    pub fn delete(&self) -> Result<(), AppError> {
        if self.path.exists() {
            std::fs::remove_file(&self.path)
                .map_err(|e| AppError::Config(format!("Failed to delete token file: {}", e)))?;
        }
        Ok(())
    }
}

pub fn mask_token(token: &str) -> String {
    if token.len() <= 6 {
        return "*".repeat(token.len());
    }
    let prefix = &token[..6];
    format!("{}...", prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_token_short() {
        assert_eq!(mask_token("abc"), "***");
    }

    #[test]
    fn test_mask_token_normal() {
        let result = mask_token("gho_abcdefghijklmnop");
        assert_eq!(result, "gho_ab...");
    }

    #[test]
    fn test_mask_token_exact_six() {
        assert_eq!(mask_token("123456"), "******");
    }

    #[test]
    fn test_mask_token_empty() {
        assert_eq!(mask_token(""), "");
    }

    #[test]
    fn test_token_store_path_resolution() {
        let store = TokenStore::from_path(PathBuf::from("/tmp/test/copilot_token.json"));
        assert_eq!(store.path(), &PathBuf::from("/tmp/test/copilot_token.json"));
    }

    #[test]
    fn test_save_and_load() {
        let dir = std::env::temp_dir().join(format!("mini_router_test_{}", std::process::id()));
        let path = dir.join("copilot_token.json");
        let store = TokenStore::from_path(path.clone());

        let data = GithubTokenData {
            github_access_token: "test_token_value".to_string(),
            token_type: "bearer".to_string(),
            scope: Some("read:user".to_string()),
            created_at: 1710000000,
        };

        store.save(&data).unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.github_access_token, "test_token_value");
        assert_eq!(loaded.token_type, "bearer");
        assert_eq!(loaded.created_at, 1710000000);

        store.delete().unwrap();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_nonexistent() {
        let store = TokenStore::from_path(PathBuf::from("/tmp/nonexistent_path_deadbeef.json"));
        let result = store.load().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_delete_nonexistent() {
        let store = TokenStore::from_path(PathBuf::from("/tmp/nonexistent_path_deadbeef2.json"));
        assert!(store.delete().is_ok());
    }
}
