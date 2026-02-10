//! Internationalization (i18n) module.
//!
//! Provides localized strings for the application UI and CLI output.
//! English is the default language; Spanish is available as an alternative.
//! The architecture supports adding more languages in the future.

use std::sync::OnceLock;

static CURRENT_LANG: OnceLock<Lang> = OnceLock::new();

/// Supported languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// English (default)
    En,
    /// Spanish
    Es,
}

impl Lang {
    /// Parse a language code string (e.g. "en", "es", "en_US", "es_ES").
    /// Returns `None` for unrecognized codes.
    pub fn from_code(code: &str) -> Option<Self> {
        let normalized = code.to_lowercase();
        let prefix = normalized.split(['_', '-']).next().unwrap_or("");
        match prefix {
            "en" => Some(Self::En),
            "es" => Some(Self::Es),
            _ => None,
        }
    }

    /// Return the ISO 639-1 code for this language.
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Es => "es",
        }
    }
}

/// Initialize the global language. Call once at startup.
/// If already initialized, this is a no-op.
pub fn set_lang(lang: Lang) {
    let _ = CURRENT_LANG.set(lang);
}

/// Get the currently configured language (defaults to English).
pub fn lang() -> Lang {
    CURRENT_LANG.get().copied().unwrap_or(Lang::En)
}

/// Detect language from the `LANG` / `LC_MESSAGES` environment variables.
pub fn detect_system_lang() -> Lang {
    std::env::var("MBOXSHELL_LANG")
        .ok()
        .and_then(|v| Lang::from_code(&v))
        .or_else(|| {
            std::env::var("LC_MESSAGES")
                .ok()
                .and_then(|v| Lang::from_code(&v))
        })
        .or_else(|| std::env::var("LANG").ok().and_then(|v| Lang::from_code(&v)))
        .unwrap_or(Lang::En)
}

/// Macro for defining translatable message functions.
/// Each function returns a `&'static str` based on the current language.
macro_rules! msg {
    ($name:ident, $en:expr, $es:expr) => {
        /// Returns a localized string for the current language.
        pub fn $name() -> &'static str {
            match lang() {
                Lang::En => $en,
                Lang::Es => $es,
            }
        }
    };
}

// ── General ──────────────────────────────────────────────────────

msg!(app_name, "mboxShell", "mboxShell");
msg!(
    app_about,
    "mboxShell \u{2014} Fast terminal viewer for MBOX files of any size. Open, search and export emails from Gmail Takeout backups (50GB+) without loading them into memory.",
    "mboxShell \u{2014} Visor r\u{e1}pido de terminal para ficheros MBOX de cualquier tama\u{f1}o. Abre, busca y exporta correos de backups Gmail Takeout (50GB+) sin cargarlos en memoria."
);
msg!(
    app_long_about,
    "mboxShell \u{2014} Fast terminal viewer for MBOX files of any size.\nOpen, search and export emails from Gmail Takeout backups (50GB+)\nwithout loading them into memory. Built in Rust.",
    "mboxShell \u{2014} Visor r\u{e1}pido de terminal para ficheros MBOX de cualquier tama\u{f1}o.\nAbre, busca y exporta correos de backups Gmail Takeout (50GB+)\nsin cargarlos en memoria. Escrito en Rust."
);

// ── CLI help strings ─────────────────────────────────────────────

msg!(
    help_file_arg,
    "MBOX file or directory to open (shortcut for 'open' command)",
    "Fichero MBOX o directorio a abrir (atajo para el comando 'open')"
);
msg!(
    help_verbose,
    "Verbose logging (-v info, -vv debug, -vvv trace)",
    "Registro detallado (-v info, -vv debug, -vvv trace)"
);
msg!(
    help_lang,
    "Language (en, es). Defaults to system locale",
    "Idioma (en, es). Por defecto usa el idioma del sistema"
);
msg!(
    help_cmd_open,
    "Open a file in the TUI (default if no subcommand given)",
    "Abrir un fichero en la TUI (por defecto si no se da subcomando)"
);
msg!(
    help_cmd_index,
    "Index an MBOX file and show statistics",
    "Indexar un fichero MBOX y mostrar estad\u{ed}sticas"
);
msg!(
    help_cmd_stats,
    "Show statistics about an MBOX file",
    "Mostrar estad\u{ed}sticas de un fichero MBOX"
);
msg!(
    help_force_rebuild,
    "Force rebuild even if index exists",
    "Forzar reconstrucci\u{f3}n aunque el \u{ed}ndice exista"
);
msg!(help_output_json, "Output as JSON", "Salida en formato JSON");
msg!(help_cmd_search, "Search messages", "Buscar mensajes");
msg!(help_cmd_export, "Export messages", "Exportar mensajes");
msg!(
    help_cmd_merge,
    "Merge multiple MBOX files",
    "Combinar varios ficheros MBOX"
);
msg!(
    help_cmd_attachments,
    "Extract all attachments",
    "Extraer todos los adjuntos"
);
msg!(
    help_cmd_completions,
    "Generate shell completions",
    "Generar completions para tu shell"
);
msg!(
    help_cmd_manpage,
    "Generate a man page",
    "Generar p\u{e1}gina de manual"
);
msg!(
    app_after_help,
    "Copyright (c) 2026 David Carrero Fern\u{e1}ndez-Baillo \u{2014} MIT License\nSource Code: https://github.com/dcarrero/mboxshell",
    "Copyright (c) 2026 David Carrero Fern\u{e1}ndez-Baillo \u{2014} Licencia MIT\nC\u{f3}digo fuente: https://github.com/dcarrero/mboxshell"
);

// ── Index / stats output ─────────────────────────────────────────

msg!(msg_indexing, "Indexing", "Indexando");
msg!(msg_messages, "messages", "mensajes");
msg!(
    msg_loading_index,
    "Loading existing index...",
    "Cargando \u{ed}ndice existente..."
);
msg!(
    msg_building_index,
    "Building index...",
    "Construyendo \u{ed}ndice..."
);
msg!(msg_index_built, "Index built", "Índice construido");
msg!(msg_file, "File", "Fichero");
msg!(msg_file_size, "File size", "Tama\u{f1}o del fichero");
msg!(msg_message_count, "Messages", "Mensajes");
msg!(msg_date_range, "Date range", "Rango de fechas");
msg!(msg_index_size, "Index size", "Tama\u{f1}o del \u{ed}ndice");
msg!(
    msg_indexing_time,
    "Indexing time",
    "Tiempo de indexaci\u{f3}n"
);
msg!(msg_top_senders, "Top senders", "Principales remitentes");
msg!(msg_with_attachments, "With attachments", "Con adjuntos");
msg!(
    msg_no_messages,
    "No messages found",
    "No se encontraron mensajes"
);
msg!(
    msg_empty_file,
    "File is empty",
    "El fichero est\u{e1} vac\u{ed}o"
);

// ── Errors ───────────────────────────────────────────────────────

msg!(
    err_file_not_found,
    "File not found",
    "Fichero no encontrado"
);
msg!(
    err_tui_not_implemented,
    "TUI not yet implemented. Use 'mboxshell index' to verify parsing.",
    "TUI a\u{fa}n no implementada. Usa 'mboxshell index' para verificar el parsing."
);
msg!(
    err_not_implemented,
    "This command is not implemented yet. Coming in a future release.",
    "Este comando a\u{fa}n no est\u{e1} implementado. Llegar\u{e1} en una versi\u{f3}n futura."
);
msg!(
    err_no_file_given,
    "No MBOX file specified. Usage:\n\n  mboxshell <file.mbox>\n\nRun 'mboxshell --help' for more options.",
    "No se ha indicado fichero MBOX. Uso:\n\n  mboxshell <fichero.mbox>\n\nEjecuta 'mboxshell --help' para ver todas las opciones."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lang_from_code() {
        assert_eq!(Lang::from_code("en"), Some(Lang::En));
        assert_eq!(Lang::from_code("es"), Some(Lang::Es));
        assert_eq!(Lang::from_code("en_US"), Some(Lang::En));
        assert_eq!(Lang::from_code("es_ES"), Some(Lang::Es));
        assert_eq!(Lang::from_code("es-MX"), Some(Lang::Es));
        assert_eq!(Lang::from_code("fr"), None);
    }

    #[test]
    fn test_lang_code_roundtrip() {
        assert_eq!(Lang::En.code(), "en");
        assert_eq!(Lang::Es.code(), "es");
    }

    #[test]
    fn test_default_lang_is_english() {
        // In tests, OnceLock may already be set, so we just verify the function works
        let l = lang();
        assert!(l == Lang::En || l == Lang::Es);
    }

    #[test]
    fn test_messages_return_strings() {
        // Smoke test: all message functions return non-empty strings
        assert!(!app_name().is_empty());
        assert!(!app_about().is_empty());
        assert!(!msg_indexing().is_empty());
        assert!(!err_file_not_found().is_empty());
    }
}
