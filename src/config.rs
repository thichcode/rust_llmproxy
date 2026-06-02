use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::AppError;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_model")]
    pub default_model: String,
    #[serde(default)]
    pub server: ServerConfig,
    pub models: HashMap<String, ModelConfig>,
    #[serde(default)]
    pub rtk: RtkConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub api_base: String,
    pub api_key_env: String,
    pub model: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RtkConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_max_message_chars")]
    pub max_message_chars: usize,
    #[serde(default = "default_preserve_head")]
    pub preserve_head_chars: usize,
    #[serde(default = "default_preserve_tail")]
    pub preserve_tail_chars: usize,
}

fn default_model() -> String {
    "gpt-4o-mini".to_string()
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    20128
}

fn default_max_message_chars() -> usize {
    8000
}

fn default_preserve_head() -> usize {
    2000
}

fn default_preserve_tail() -> usize {
    2000
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            host: default_host(),
            port: default_port(),
        }
    }
}

impl Default for RtkConfig {
    fn default() -> Self {
        RtkConfig {
            enabled: false,
            max_message_chars: default_max_message_chars(),
            preserve_head_chars: default_preserve_head(),
            preserve_tail_chars: default_preserve_tail(),
        }
    }
}

impl Config {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, AppError> {
        let contents = std::fs::read_to_string(path.as_ref())
            .map_err(|e| AppError::Config(format!("Failed to read config file: {}", e)))?;
        serde_yaml::from_str(&contents)
            .map_err(|e| AppError::Config(format!("Failed to parse config file: {}", e)))
    }
}
