//! Application configuration.
//!
//! Configuration is loaded from a TOML file at:
//! 1. `$MBOXSHELL_CONFIG` (environment variable)
//! 2. The platform config directory ([`dirs::config_dir`]):
//!    - Linux: `$XDG_CONFIG_HOME/mboxshell/config.toml` or `~/.config/mboxshell/config.toml`
//!    - macOS: `~/Library/Application Support/mboxshell/config.toml`
//!    - Windows: `%APPDATA%\mboxshell\config.toml`
//! 3. Built-in defaults
//!
//! `main` loads it once and stores it with [`set_active`]; the rest of the
//! program reads it through [`active`], which falls back to the defaults
//! (as in unit tests, which never set one). `mboxshell config` prints the
//! path, the file and the defaults.

use std::path::PathBuf;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Top-level configuration.
///
/// Unknown keys are ignored, so a file written for an older version (which
/// had `[columns]`, `[performance]` and a few more options) still loads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// General behavior settings.
    pub general: GeneralConfig,
    /// Display and layout settings.
    pub display: DisplayConfig,
    /// Export defaults.
    pub export: ExportConfig,
}

/// General behavior settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Initial sort column of the message list: "date", "from", "subject", "size".
    pub default_sort: String,
    /// Initial sort direction: "asc" or "desc".
    pub sort_order: String,
    /// `strftime` format string for dates in the message list.
    pub date_format: String,
    /// Override the cache directory for fallback indexes and the log file.
    pub cache_dir: Option<PathBuf>,
    /// Log level: "error", "warn", "info", "debug", "trace".
    pub log_level: String,
}

/// Display and layout settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// Color theme: "dark", "light" or "terminal".
    pub theme: String,
    /// Initial layout: "horizontal", "vertical", "list-only".
    pub layout: String,
    /// Show the label sidebar on startup when the mailbox has labels.
    pub show_sidebar: bool,
    /// Maximum number of decoded messages in the LRU cache.
    pub max_cached_messages: usize,
}

/// Export defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportConfig {
    /// CSV field separator character.
    pub csv_separator: char,
    /// How `attachments` names each message's folder: "dts" (date and
    /// subject), "seq-no" (position in the mailbox, from 1) or "seq-no0"
    /// (from 0). `export` names its eml/txt/html files the same way.
    pub attachment_dirname: String,
    /// Minimum digits of a "seq-no"/"seq-no0" folder, zero-padded. 0 picks
    /// the digits of the mailbox's message count, so folders sort in order.
    pub attachment_seq_width: usize,
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
            show_sidebar: true,
            max_cached_messages: 50,
        }
    }
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            csv_separator: ',',
            attachment_dirname: "dts".to_string(),
            attachment_seq_width: 0,
        }
    }
}

// ── Validation ──────────────────────────────────────────────────

impl Config {
    /// Replace every value the program cannot use with its default, so a
    /// typo degrades one setting instead of the whole file. Returns one
    /// warning per replaced value. `attachment_dirname` is checked where it
    /// is used, and an invalid one stops the command there.
    pub fn sanitized(mut self) -> (Self, Vec<String>) {
        let defaults = Config::default();
        let mut warnings = Vec::new();
        let mut warn_invalid = |key: &str, value: &str| {
            warnings.push(format!(
                "{} {key}: {value:?}; {}",
                crate::i18n::msg_config_invalid_value(),
                crate::i18n::msg_config_using_default()
            ));
        };

        let g = &mut self.general;
        if !matches!(
            g.default_sort.as_str(),
            "date" | "from" | "subject" | "size"
        ) {
            warn_invalid("general.default_sort", &g.default_sort);
            g.default_sort = defaults.general.default_sort;
        }
        if !matches!(g.sort_order.as_str(), "asc" | "desc") {
            warn_invalid("general.sort_order", &g.sort_order);
            g.sort_order = defaults.general.sort_order;
        }
        if !is_valid_date_format(&g.date_format) {
            warn_invalid("general.date_format", &g.date_format);
            g.date_format = defaults.general.date_format;
        }
        if !matches!(
            g.log_level.as_str(),
            "error" | "warn" | "info" | "debug" | "trace"
        ) {
            warn_invalid("general.log_level", &g.log_level);
            g.log_level = defaults.general.log_level;
        }

        let d = &mut self.display;
        if crate::tui::theme::ThemeKind::from_name(&d.theme).is_none() {
            warn_invalid("display.theme", &d.theme);
            d.theme = defaults.display.theme;
        }
        if !matches!(d.layout.as_str(), "horizontal" | "vertical" | "list-only") {
            warn_invalid("display.layout", &d.layout);
            d.layout = defaults.display.layout;
        }
        if d.max_cached_messages == 0 {
            warn_invalid("display.max_cached_messages", "0");
            d.max_cached_messages = defaults.display.max_cached_messages;
        }

        let e = &mut self.export;
        if matches!(e.csv_separator, '"' | '\n' | '\r') {
            warn_invalid("export.csv_separator", &e.csv_separator.to_string());
            e.csv_separator = defaults.export.csv_separator;
        }

        (self, warnings)
    }
}

/// `true` if `format` is a non-empty `strftime` string chrono can render.
///
/// chrono panics when a formatted date with an unknown specifier is turned
/// into a `String`, so the format is checked once here instead.
fn is_valid_date_format(format: &str) -> bool {
    use chrono::format::{Item, StrftimeItems};
    !format.is_empty() && !StrftimeItems::new(format).any(|item| matches!(item, Item::Error))
}

// ── Active configuration ────────────────────────────────────────

static ACTIVE: OnceLock<Config> = OnceLock::new();

/// Make `config` the configuration for this run. Only the first call has any
/// effect; `main` makes it once, right after [`load_config`].
pub fn set_active(config: Config) {
    let _ = ACTIVE.set(config);
}

/// The configuration for this run, or the defaults if none was set.
pub fn active() -> &'static Config {
    ACTIVE.get_or_init(Config::default)
}

// ── Load / save ─────────────────────────────────────────────────

/// Load configuration, searching standard locations.
///
/// Returns the default configuration if no file is found or on parse error,
/// and replaces invalid values with their defaults (see [`Config::sanitized`]).
/// The second value lists what went wrong: it is returned rather than logged
/// because the config, which sets the log level, is read before logging
/// starts.
pub fn load_config() -> (Config, Vec<String>) {
    let Some(path) = config_file_path() else {
        return (Config::default(), Vec::new());
    };
    if !path.exists() {
        return (Config::default(), Vec::new());
    }
    let shown = path.display();
    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str::<Config>(&contents) {
            Ok(cfg) => {
                let (cfg, warnings) = cfg.sanitized();
                let warnings = warnings
                    .into_iter()
                    .map(|w| format!("{shown}: {w}"))
                    .collect();
                (cfg, warnings)
            }
            Err(e) => (
                Config::default(),
                vec![format!("Failed to parse {shown}, using defaults: {e}")],
            ),
        },
        Err(e) => (
            Config::default(),
            vec![format!("Failed to read {shown}, using defaults: {e}")],
        ),
    }
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

/// Return the cache directory for fallback indexes, logs, etc.
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

/// The built-in defaults as a commented `config.toml`, for
/// `mboxshell config defaults`.
///
/// The values come from [`Config::default`]; a test checks that this text
/// parses back to exactly those defaults, so it cannot drift from the code.
pub fn defaults_toml() -> String {
    /// `key = value`, with the comment aligned in one column.
    fn line(out: &mut String, setting: String, comment: &str) {
        out.push_str(&format!("{setting:<34}# {comment}\n"));
    }

    let c = Config::default();
    let (g, d, e) = (&c.general, &c.display, &c.export);
    let mut out = String::from(
        "# mboxshell configuration. Every key is optional: a missing key keeps\n\
         # the default shown here. `mboxshell config path` prints where it goes.\n",
    );

    out.push_str("\n[general]\n");
    line(
        &mut out,
        format!("default_sort = {:?}", g.default_sort),
        "date | from | subject | size",
    );
    line(
        &mut out,
        format!("sort_order = {:?}", g.sort_order),
        "desc | asc",
    );
    line(
        &mut out,
        format!("date_format = {:?}", g.date_format),
        "strftime format for the message list",
    );
    line(
        &mut out,
        format!("log_level = {:?}", g.log_level),
        "error | warn | info | debug | trace",
    );
    line(
        &mut out,
        "# cache_dir = \"/path/to/dir\"".to_string(),
        "fallback indexes and log (default: the system cache dir)",
    );

    out.push_str("\n[display]\n");
    line(
        &mut out,
        format!("theme = {:?}", d.theme),
        "dark | light | terminal (NO_COLOR forces terminal)",
    );
    line(
        &mut out,
        format!("layout = {:?}", d.layout),
        "horizontal | vertical | list-only",
    );
    line(
        &mut out,
        format!("show_sidebar = {}", d.show_sidebar),
        "label sidebar, when the mailbox has labels",
    );
    line(
        &mut out,
        format!("max_cached_messages = {}", d.max_cached_messages),
        "decoded messages kept in memory",
    );

    out.push_str("\n[export]\n");
    line(
        &mut out,
        format!("csv_separator = {:?}", e.csv_separator.to_string()),
        "one character, e.g. \";\" or \"\\t\"",
    );
    line(
        &mut out,
        format!("attachment_dirname = {:?}", e.attachment_dirname),
        "dts | seq-no | seq-no0",
    );
    line(
        &mut out,
        format!("attachment_seq_width = {}", e.attachment_seq_width),
        "minimum digits for seq-no names (0 = automatic)",
    );
    out
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
        assert_eq!(cfg.display.max_cached_messages, 50);
        assert_eq!(cfg.export.csv_separator, ',');
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let cfg = Config::default();
        let toml_str = toml::to_string_pretty(&cfg).expect("serialize");
        let parsed: Config = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed, cfg);
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
        assert_eq!(cfg.display.max_cached_messages, 50);
    }

    #[test]
    fn test_old_config_with_removed_options_still_loads() {
        let old = r#"
[display]
layout = "vertical"
message_text_width = 80

[columns]
date_width = 20

[performance]
read_buffer_size = 65536

[export]
default_format = "html"
csv_separator = ";"
"#;
        let cfg: Config = toml::from_str(old).expect("old config parses");
        assert_eq!(cfg.display.layout, "vertical");
        assert_eq!(cfg.export.csv_separator, ';');
    }

    #[test]
    fn test_defaults_toml_matches_config_default() {
        let parsed: toml::Value = toml::from_str(&defaults_toml()).expect("defaults parse");
        let expected = toml::Value::try_from(Config::default()).expect("serialize defaults");
        assert_eq!(parsed, expected);
    }

    #[test]
    fn test_sanitized_replaces_invalid_values() {
        let mut cfg = Config::default();
        cfg.general.default_sort = "colour".into();
        cfg.general.sort_order = "up".into();
        cfg.general.date_format = "%Y-%Q".into();
        cfg.general.log_level = "loud".into();
        cfg.display.theme = "solarized".into();
        cfg.display.layout = "stacked".into();
        cfg.display.max_cached_messages = 0;
        cfg.export.csv_separator = '"';
        let (cfg, warnings) = cfg.sanitized();
        assert_eq!(cfg, Config::default());
        assert_eq!(warnings.len(), 8);
    }

    #[test]
    fn test_sanitized_keeps_valid_values() {
        let mut cfg = Config::default();
        cfg.general.default_sort = "subject".into();
        cfg.general.sort_order = "asc".into();
        cfg.general.date_format = "%d/%m/%Y".into();
        cfg.display.theme = "Terminal".into();
        cfg.display.layout = "list-only".into();
        cfg.export.csv_separator = '\t';
        assert_eq!(cfg.clone().sanitized(), (cfg, Vec::new()));
    }

    #[test]
    fn test_date_format_validation() {
        assert!(is_valid_date_format("%Y-%m-%d %H:%M"));
        assert!(is_valid_date_format("%a %e %b"));
        assert!(!is_valid_date_format(""));
        assert!(!is_valid_date_format("%Q"));
    }
}
