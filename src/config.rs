//! Application configuration.
//!
//! Configuration is loaded from a TOML file at:
//! 1. `$MBOXSHELL_CONFIG` (environment variable)
//! 2. `~/.config/mboxshell/config.toml` (Linux/macOS)
//!    `%APPDATA%\mboxshell\config.toml` (Windows)
//! 3. Built-in defaults

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Top-level configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// General behavior settings.
    pub general: GeneralConfig,
    /// Display and layout settings.
    pub display: DisplayConfig,
    /// Column widths for the message list.
    pub columns: ColumnsConfig,
    /// Export defaults.
    pub export: ExportConfig,
    /// Performance tuning.
    pub performance: PerformanceConfig,
}

/// General behavior settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Default sort column: "date", "from", "subject", "size".
    pub default_sort: String,
    /// Default sort direction: "asc" or "desc".
    pub sort_order: String,
    /// `strftime` format string for dates in the message list.
    pub date_format: String,
    /// Override cache directory for indexes and logs.
    pub cache_dir: Option<PathBuf>,
    /// Log level: "error", "warn", "info", "debug", "trace".
    pub log_level: String,
}

/// Display and layout settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// Color theme: "dark" or "light".
    pub theme: String,
    /// Initial layout: "horizontal", "vertical", "list-only".
    pub layout: String,
    /// Show sidebar on startup.
    pub show_sidebar: bool,
    /// Maximum number of decoded messages in the LRU cache.
    pub max_cached_messages: usize,
    /// Preferred text width for the message view (0 = terminal width).
    pub message_text_width: usize,
}

/// Column width overrides for the message list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnsConfig {
    /// Date column width.
    pub date_width: u16,
    /// From column width.
    pub from_width: u16,
    /// Size column width.
    pub size_width: u16,
}

/// Export defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportConfig {
    /// Default export format: "eml", "csv", "txt".
    pub default_format: String,
    /// Default output directory.
    pub default_output_dir: Option<PathBuf>,
    /// CSV field separator character.
    pub csv_separator: char,
}

/// Performance tuning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformanceConfig {
    /// Read buffer size in bytes (default: 131072 = 128 KB).
    pub read_buffer_size: usize,
    /// Maximum message size in bytes (default: 268435456 = 256 MB).
    pub max_message_size: usize,
    /// Number of entries in the decoded-message LRU cache.
    pub lru_cache_size: usize,
}

// ── Default implementations ─────────────────────────────────────

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            default_sort: "date".to_string(),
            sort_order: "desc".to_string(),
            date_format: "%Y-%m-%d %H:%M".to_string(),
            cache_dir: None,
            log_level: "warn".to_string(),
        }
    }
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            layout: "horizontal".to_string(),
            show_sidebar: false,
            max_cached_messages: 50,
            message_text_width: 0,
        }
    }
}

impl Default for ColumnsConfig {
    fn default() -> Self {
        Self {
            date_width: 17,
            from_width: 20,
            size_width: 8,
        }
    }
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            default_format: "eml".to_string(),
            default_output_dir: None,
            csv_separator: ',',
        }
    }
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            read_buffer_size: 128 * 1024,        // 128 KB
            max_message_size: 256 * 1024 * 1024, // 256 MB
            lru_cache_size: 50,
        }
    }
}

// ── Load / save ─────────────────────────────────────────────────

/// Load configuration, searching standard locations.
///
/// Returns the default configuration if no file is found or on parse error.
pub fn load_config() -> Config {
    if let Some(path) = config_file_path() {
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(contents) => match toml::from_str::<Config>(&contents) {
                    Ok(cfg) => {
                        tracing::info!(path = %path.display(), "Loaded config");
                        return cfg;
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %e,
                            "Failed to parse config, using defaults"
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "Failed to read config file, using defaults"
                    );
                }
            }
        }
    }
    Config::default()
}

/// Save configuration to the standard location.
pub fn save_config(config: &Config) -> anyhow::Result<()> {
    let path = config_file_path()
        .ok_or_else(|| anyhow::anyhow!("Could not determine config file path"))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let contents = toml::to_string_pretty(config)?;
    std::fs::write(&path, contents)?;
    tracing::info!(path = %path.display(), "Saved config");
    Ok(())
}

/// Determine the config file path (checking env var first, then standard dirs).
pub fn config_file_path() -> Option<PathBuf> {
    // 1. Environment variable override
    if let Ok(env_path) = std::env::var("MBOXSHELL_CONFIG") {
        return Some(PathBuf::from(env_path));
    }

    // 2. Standard config directory
    dirs::config_dir().map(|d| d.join("mboxshell").join("config.toml"))
}

/// Return the cache directory for indexes, logs, etc.
pub fn cache_dir(config: &Config) -> PathBuf {
    if let Some(ref dir) = config.general.cache_dir {
        return dir.clone();
    }
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mboxshell")
}

/// Return the log file path.
pub fn log_file_path(config: &Config) -> PathBuf {
    cache_dir(config).join("mboxshell.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.general.default_sort, "date");
        assert_eq!(cfg.general.sort_order, "desc");
        assert_eq!(cfg.display.theme, "dark");
        assert_eq!(cfg.performance.lru_cache_size, 50);
        assert_eq!(cfg.export.csv_separator, ',');
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let cfg = Config::default();
        let toml_str = toml::to_string_pretty(&cfg).expect("serialize");
        let parsed: Config = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.general.default_sort, cfg.general.default_sort);
        assert_eq!(parsed.display.theme, cfg.display.theme);
        assert_eq!(
            parsed.performance.read_buffer_size,
            cfg.performance.read_buffer_size
        );
    }

    #[test]
    fn test_partial_config_uses_defaults() {
        let partial = r#"
[general]
default_sort = "from"

[display]
theme = "light"
"#;
        let cfg: Config = toml::from_str(partial).expect("parse partial");
        assert_eq!(cfg.general.default_sort, "from");
        assert_eq!(cfg.display.theme, "light");
        // Other fields use defaults
        assert_eq!(cfg.general.sort_order, "desc");
        assert_eq!(cfg.performance.lru_cache_size, 50);
    }

    #[test]
    fn test_config_file_path_env_override() {
        // Cannot reliably test this without modifying env, so just verify the function works
        let path = config_file_path();
        // Should return Some on most systems (has config dir)
        // On CI it might be None, so we just check it doesn't panic
        let _ = path;
    }
}
