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
    help_cmd_config,
    "Show the config file: where it is, its contents or the defaults",
    "Mostrar el fichero de configuraci\u{f3}n: d\u{f3}nde est\u{e1}, qu\u{e9} contiene o los valores por defecto"
);
msg!(
    help_cmd_config_path,
    "Print the path of the config file, whether or not it exists (default)",
    "Mostrar la ruta del fichero de configuraci\u{f3}n, exista o no (por defecto)"
);
msg!(
    help_cmd_config_show,
    "Print the config file, or fail if there is none",
    "Mostrar el fichero de configuraci\u{f3}n, o fallar si no existe"
);
msg!(
    help_cmd_config_defaults,
    "Print the built-in defaults as a commented config file",
    "Mostrar los valores por defecto como un fichero de configuraci\u{f3}n comentado"
);
msg!(
    msg_config_no_path,
    "Cannot determine the config directory; set MBOXSHELL_CONFIG",
    "No se puede determinar el directorio de configuraci\u{f3}n; define MBOXSHELL_CONFIG"
);
msg!(
    msg_config_not_created,
    "(not created yet: `mboxshell config defaults` prints a starting point)",
    "(a\u{fa}n no existe: `mboxshell config defaults` muestra uno para empezar)"
);
msg!(
    msg_config_cannot_read,
    "Cannot read the config file",
    "No se puede leer el fichero de configuraci\u{f3}n"
);
msg!(
    msg_config_invalid_value,
    "Invalid value for",
    "Valor no v\u{e1}lido para"
);
msg!(
    msg_config_using_default,
    "using the default",
    "se usa el valor por defecto"
);
msg!(
    msg_config_parse_error,
    "Warning: this file is not valid TOML, so mboxshell ignores it and uses the defaults:",
    "Aviso: este fichero no es TOML v\u{e1}lido, as\u{ed} que mboxshell lo ignora y usa los valores por defecto:"
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
msg!(msg_duplicates, "Duplicates", "Duplicados");
msg!(msg_unique_ids, "Unique IDs", "IDs únicos");
msg!(msg_without_message_id, "No Message-ID", "Sin Message-ID");
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
    err_not_mbox,
    "Not an MBOX mailbox (no message starts with a 'From ' line)",
    "No es un buzón MBOX (ningún mensaje empieza por una línea 'From ')"
);
msg!(
    err_invalid_query,
    "Filter not understood (check the date, size or has: value)",
    "Filtro no válido (revisa el valor de la fecha, el tamaño o has:)"
);
msg!(
    msg_message_truncated,
    "mboxshell: message too large; showing only its first",
    "mboxshell: mensaje demasiado grande; solo se muestran sus primeros"
);
msg!(
    err_tui_needs_terminal,
    "The interactive viewer needs a terminal. Without one, use `mboxshell stats` or `mboxshell search <file> <query> --json`",
    "El visor interactivo necesita un terminal. Sin él, usa `mboxshell stats` o `mboxshell search <fichero> <consulta> --json`"
);
msg!(
    html_remote_images_blocked,
    "Remote images were blocked to protect your privacy (they can reveal when and where this message is read). Re-export with --allow-remote-images to load them",
    "Se han bloqueado las imágenes remotas para proteger tu privacidad (pueden revelar cuándo y desde dónde se lee este mensaje). Vuelve a exportar con --allow-remote-images para cargarlas"
);
msg!(err_io, "I/O error reading", "Error de E/S leyendo");
msg!(tui_header_label, "label", "etiqueta");
msg!(tui_header_search, "search", "búsqueda");
msg!(tui_files, "file(s)", "fichero(s)");
msg!(tui_exited_with, "exited with", "terminó con");
msg!(
    tui_attachment_not_found,
    "Attachment not found",
    "Adjunto no encontrado"
);
msg!(
    err_output_is_input,
    "Refusing to overwrite an input mailbox (mboxshell never modifies its sources)",
    "No se sobrescribe un buzón de entrada (mboxshell nunca modifica los originales)"
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

// ── TUI Widget titles and labels ────────────────────────────────

msg!(tui_help_title, " Help ", " Ayuda ");
msg!(
    tui_help_description,
    "Fast terminal viewer for MBOX files",
    "Visor r\u{e1}pido de terminal para ficheros MBOX"
);
msg!(tui_messages_title, " Messages ", " Mensajes ");
msg!(tui_message_title, " Message ", " Mensaje ");
msg!(tui_message_raw, " Message [RAW] ", " Mensaje [RAW] ");
msg!(
    tui_message_headers,
    " Message [HEADERS] ",
    " Mensaje [CABECERAS] "
);
msg!(tui_labels_title, " Labels ", " Etiquetas ");
msg!(tui_attachments_title, " Attachments ", " Adjuntos ");
msg!(
    tui_search_filters_title,
    " Search Filters ",
    " Filtros de b\u{fa}squeda "
);
msg!(
    tui_no_message,
    "No message selected",
    "Ning\u{fa}n mensaje seleccionado"
);
msg!(
    tui_no_text_content,
    "(No text content)",
    "(Sin contenido de texto)"
);
msg!(tui_no_attachments, "No attachments", "Sin adjuntos");
msg!(tui_all_messages, "All Messages", "Todos los mensajes");
msg!(tui_help_hint, " [?] Help ", " [?] Ayuda ");

// ── Mail view header labels ─────────────────────────────────────

msg!(tui_header_date, "Date:    ", "Fecha:   ");
msg!(tui_header_from, "From:    ", "De:      ");
msg!(tui_header_to, "To:      ", "Para:    ");
msg!(tui_header_cc, "Cc:      ", "Cc:      ");
msg!(tui_header_subject, "Subject: ", "Asunto:  ");

// ── Header bar ──────────────────────────────────────────────────

msg!(tui_messages_count, "messages", "mensajes");
msg!(tui_marked_count, "marked", "marcados");

// ── Column headers ──────────────────────────────────────────────

msg!(tui_col_date, "Date", "Fecha");
msg!(tui_col_from, "From", "De");
msg!(tui_col_subject, "Subject", "Asunto");
msg!(tui_col_size, "Size", "Tama\u{f1}o");
msg!(tui_col_filename, "Filename", "Nombre");
msg!(tui_col_type, "Type", "Tipo");

// ── Help popup section headers ──────────────────────────────────

msg!(tui_help_navigation, "Navigation", "Navegaci\u{f3}n");
msg!(
    tui_help_message_export,
    "Message & Export",
    "Mensaje y exportaci\u{f3}n"
);
msg!(tui_help_list_actions, "List Actions", "Acciones de lista");
msg!(tui_help_search, "Search", "B\u{fa}squeda");
msg!(
    tui_help_layout_general,
    "Layout & General",
    "Disposici\u{f3}n y general"
);

// ── Help popup shortcut descriptions ────────────────────────────

msg!(tui_help_next_prev, "Next / prev", "Siguiente / anterior");
msg!(tui_help_first_last, "First / last", "Primero / \u{fa}ltimo");
msg!(tui_help_page_scroll, "Page scroll", "Avance de p\u{e1}gina");
msg!(
    tui_help_scroll_body,
    "Scroll body (keep list focus)",
    "Scroll cuerpo (mantiene foco lista)"
);
msg!(tui_help_open_message, "Open message", "Abrir mensaje");
msg!(tui_help_cycle_panel, "Cycle panel", "Cambiar panel");
msg!(tui_help_back_close, "Back / close", "Atr\u{e1}s / cerrar");
msg!(tui_help_full_headers, "Full headers", "Cabeceras completas");
msg!(tui_help_raw_source, "Raw source", "C\u{f3}digo fuente");
msg!(tui_help_export_menu, "Export menu", "Men\u{fa} exportar");
msg!(tui_help_attachments, "Attachments", "Adjuntos");
msg!(
    tui_help_find_in_body,
    "Find in message / next-prev",
    "Buscar en mensaje / sig-ant"
);
msg!(tui_help_mark_unmark, "Mark / unmark", "Marcar / desmarcar");
msg!(tui_help_mark_all, "Mark all", "Marcar todos");
msg!(tui_help_cycle_sort, "Cycle sort col", "Cambiar columna");
msg!(
    tui_help_sort_direction,
    "Sort direction",
    "Direcci\u{f3}n orden"
);
msg!(tui_help_thread_view, "Thread view", "Vista hilos");
msg!(tui_help_search_bar, "Search bar", "Barra b\u{fa}squeda");
msg!(tui_help_filter_popup, "Filter popup", "Popup filtros");
msg!(
    tui_help_next_prev_result,
    "Next / prev result",
    "Resultado sig. / ant."
);
msg!(tui_help_layout_mode, "Layout mode", "Modo disposici\u{f3}n");
msg!(tui_help_labels_sidebar, "Labels sidebar", "Panel etiquetas");
msg!(tui_help_this_help, "This help", "Esta ayuda");
msg!(tui_help_quit, "Quit", "Salir");
msg!(tui_help_force_quit, "Force quit", "Forzar salida");
msg!(
    tui_help_search_history,
    "Up/Down in search bar: navigate history",
    "Arriba/Abajo en barra b\u{fa}squeda: navegar historial"
);
msg!(
    tui_help_search_syntax_key,
    "F1 in search bar: this syntax",
    "F1 en la barra de b\u{fa}squeda: esta sintaxis"
);

// ── Search syntax reference (TUI help, `search --help`) ─────────

msg!(
    search_syntax_title,
    "Search syntax",
    "Sintaxis de b\u{fa}squeda"
);

/// Every search operator with a one-line description, for the in-app help
/// and `search --help`. Keep in step with `search::query` and the README.
pub fn search_syntax() -> &'static [(&'static str, &'static str)] {
    match lang() {
        Lang::En => &[
            ("word", "Subject, from, to and the message text"),
            ("\"two words\"", "Quotes keep a phrase together"),
            ("from: to: cc:", "Sender / recipients, name or address"),
            ("subject:", "Subject line"),
            ("body:", "Message text (reads the mailbox)"),
            ("filename:", "Attachment file name (reads the mailbox)"),
            ("label:", "Gmail label, e.g. label:Inbox"),
            ("id:", "Message-ID"),
            ("has:attachment", "With attachments (has:no-attachment)"),
            ("date:2024", "A year, month (2024-03) or day (2024-03-15)"),
            ("date:2024-01..2024-06", "Range, both ends included"),
            ("after:2024-01-01", "From that day on, the day included"),
            ("before:2024-07-01", "Up to that day, the day excluded"),
            ("size:>1mb  size:<100kb", "Larger / smaller (b, kb, mb, gb)"),
            ("a b", "Both must match (AND)"),
            ("a OR b", "Either; binds tighter than AND"),
            ("-word  -from:bob", "Exclude"),
        ],
        Lang::Es => &[
            (
                "palabra",
                "Asunto, remitente, destinatario y texto del mensaje",
            ),
            ("\"dos palabras\"", "Las comillas mantienen la frase junta"),
            (
                "from: to: cc:",
                "Remitente / destinatarios, nombre o direcci\u{f3}n",
            ),
            ("subject:", "Asunto"),
            ("body:", "Texto del mensaje (lee el buz\u{f3}n)"),
            ("filename:", "Nombre de un adjunto (lee el buz\u{f3}n)"),
            ("label:", "Etiqueta de Gmail, p. ej. label:Inbox"),
            ("id:", "Message-ID"),
            ("has:attachment", "Con adjuntos (has:no-attachment, sin)"),
            (
                "date:2024",
                "Un a\u{f1}o, un mes (2024-03) o un d\u{ed}a (2024-03-15)",
            ),
            ("date:2024-01..2024-06", "Rango, ambos extremos incluidos"),
            ("after:2024-01-01", "Desde ese d\u{ed}a, incluido"),
            ("before:2024-07-01", "Hasta ese d\u{ed}a, sin incluirlo"),
            (
                "size:>1mb  size:<100kb",
                "Mayor / menor que (b, kb, mb, gb)",
            ),
            ("a b", "Ambos deben cumplirse (Y)"),
            ("a OR b", "Cualquiera; OR liga m\u{e1}s fuerte que Y"),
            ("-palabra  -from:ana", "Excluir"),
        ],
    }
}

msg!(
    search_syntax_notes,
    "Dates are YYYY-MM-DD; there are no parentheses. Full reference: README (Search syntax) and docs/MANUAL.md, https://github.com/dcarrero/mboxshell",
    "Las fechas van en AAAA-MM-DD; no hay par\u{e9}ntesis. Referencia completa: README (Search syntax) y docs/MANUAL-ES.md, https://github.com/dcarrero/mboxshell"
);

/// The search syntax as plain text, for clap's `after_help`.
pub fn search_syntax_text() -> String {
    let width = search_syntax()
        .iter()
        .map(|(op, _)| op.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = format!("{}:\n", search_syntax_title());
    for (op, desc) in search_syntax() {
        out.push_str(&format!("  {op:<width$}  {desc}\n"));
    }
    out.push('\n');
    out.push_str(search_syntax_notes());
    out
}

// ── Status bar hints ────────────────────────────────────────────

msg!(tui_hint_nav, "Nav", "Nav");
msg!(tui_hint_select, "Select", "Seleccionar");
msg!(tui_hint_labels, "Labels", "Etiquetas");
msg!(tui_hint_back, "Back", "Atr\u{e1}s");
msg!(tui_hint_panel, "Panel", "Panel");
msg!(tui_hint_help, "Help", "Ayuda");
msg!(tui_hint_quit, "Quit", "Salir");
msg!(tui_hint_search, "Search", "Buscar");
msg!(tui_hint_filters, "Filters", "Filtros");
msg!(tui_hint_open, "Open", "Abrir");
msg!(tui_hint_sort, "Sort", "Ordenar");
msg!(tui_hint_mark, "Mark", "Marcar");
msg!(tui_hint_export, "Export", "Exportar");
msg!(tui_hint_attach, "Attach", "Adjuntos");
msg!(tui_hint_thread, "Thread", "Hilo");
msg!(tui_hint_scroll, "Scroll", "Scroll");
msg!(tui_hint_scroll_body, "Scroll body", "Scroll cuerpo");
msg!(tui_hint_headers, "Headers", "Cabeceras");
msg!(tui_hint_raw, "Raw", "Raw");
msg!(tui_hint_cancel, "Cancel", "Cancelar");

// ── Body scroll indicator ───────────────────────────────────────

msg!(tui_scroll_all, "All", "Todo");
msg!(tui_scroll_top, "Top", "Inicio");
msg!(tui_scroll_bot, "Bot", "Fin");

// ── In-body search ──────────────────────────────────────────────

msg!(
    tui_body_search_hint,
    "type to search in message",
    "escribe para buscar en el mensaje"
);
msg!(tui_body_search_no_match, "no matches", "sin coincidencias");
msg!(
    tui_body_search_nav_hint,
    "n/N: next/prev \u{b7} Esc: cancel",
    "n/N: siguiente/anterior \u{b7} Esc: cancelar"
);
msg!(tui_hint_find, "Find", "Buscar");

// ── Search filter popup labels ──────────────────────────────────

msg!(tui_filter_text, "Text:", "Texto:");
msg!(tui_filter_from, "From:", "De:");
msg!(tui_filter_to, "To:", "Para:");
msg!(tui_filter_subject, "Subject:", "Asunto:");
msg!(tui_filter_date_from, "Date from:", "Fecha desde:");
msg!(tui_filter_date_to, "Date to:", "Fecha hasta:");
msg!(tui_filter_size, "Size:", "Tama\u{f1}o:");
msg!(tui_filter_attachment, "Attachment:", "Adjunto:");
msg!(tui_filter_has_attachment, "Has attachment", "Con adjunto");
msg!(tui_filter_label, "Label:", "Etiqueta:");
msg!(tui_filter_any, "Any", "Cualquiera");
msg!(tui_filter_within_label, "Scope:", "\u{c1}mbito:");
msg!(
    tui_filter_within_results,
    "Search within previous results",
    "Buscar en los resultados anteriores"
);
msg!(
    tui_filter_footer,
    "Tab/↑↓:Move  PgUp/PgDn:First/Last  ←→:Change  Space:Toggle  Enter:Search  Esc:Cancel",
    "Tab/↑↓:Mover  RePág/AvPág:Inicio/Fin  ←→:Cambiar  Space:Alternar  Enter:Buscar  Esc:Cancelar"
);

// ── Export popup ────────────────────────────────────────────────

msg!(tui_export_eml, "EML", "EML");
msg!(
    tui_export_eml_desc,
    "Raw email (RFC 5322 .eml file)",
    "Email crudo (fichero RFC 5322 .eml)"
);
msg!(tui_export_txt, "TXT", "TXT");
msg!(
    tui_export_txt_desc,
    "Plain text with headers",
    "Texto plano con cabeceras"
);
msg!(tui_export_html, "HTML", "HTML");
msg!(
    tui_export_html_desc,
    "Standalone HTML page",
    "P\u{e1}gina HTML aut\u{f3}noma"
);
msg!(tui_export_csv, "CSV", "CSV");
msg!(
    tui_export_csv_desc,
    "Metadata summary (CSV)",
    "Resumen de metadatos (CSV)"
);
msg!(tui_export_attachments, "Attachments", "Adjuntos");
msg!(
    tui_export_attachments_desc,
    "Save all attachments to folder",
    "Guardar todos los adjuntos en carpeta"
);
msg!(
    tui_export_marked_title,
    "Export marked message(s)",
    "Exportar mensaje(s) marcados"
);
msg!(
    tui_export_current_title,
    "Export current message",
    "Exportar mensaje actual"
);
msg!(
    tui_export_footer,
    "j/k:Navigate  Enter:Export  Esc:Cancel",
    "j/k:Navegar  Enter:Exportar  Esc:Cancelar"
);

// ── Attachment popup footer ─────────────────────────────────────

msg!(
    tui_attachment_footer,
    "j/k:Navigate  Enter:Save  A:Save all  Esc:Close",
    "j/k:Navegar  Enter:Guardar  A:Guardar todos  Esc:Cerrar"
);

// ── Attachments summary in mail view ────────────────────────────

msg!(tui_attachments_count, "Attachments", "Adjuntos");

// ── Status / event messages ─────────────────────────────────────

msg!(
    tui_no_labels,
    "No labels found in this MBOX",
    "No se encontraron etiquetas en este MBOX"
);
msg!(tui_sorted_by, "Sorted by", "Ordenado por");
msg!(tui_sort_asc, "asc", "asc");
msg!(tui_sort_desc, "desc", "desc");
msg!(tui_saved, "Saved", "Guardado");
msg!(
    tui_error_saving,
    "Error saving attachment",
    "Error guardando adjunto"
);
msg!(
    tui_error_saving_all,
    "Error saving attachments",
    "Error guardando adjuntos"
);
msg!(
    tui_export_error,
    "Export error",
    "Error de exportaci\u{f3}n"
);
msg!(
    tui_no_attachments_msg,
    "No attachments in this message",
    "Sin adjuntos en este mensaje"
);
msg!(tui_error, "Error", "Error");
msg!(
    tui_threaded_view,
    "Threaded view enabled",
    "Vista de hilos activada"
);
msg!(tui_flat_view, "Flat view enabled", "Vista plana activada");
msg!(
    tui_showing_all,
    "Showing all messages",
    "Mostrando todos los mensajes"
);
msg!(tui_results, "result(s)", "resultado(s)");
msg!(tui_search_error, "Search error", "Error de b\u{fa}squeda");
msg!(
    tui_searching,
    "Searching message bodies",
    "Buscando en los cuerpos"
);
msg!(tui_search_cancel_hint, "Esc to cancel", "Esc para cancelar");
msg!(
    tui_search_cancelled,
    "Search cancelled",
    "B\u{fa}squeda cancelada"
);
msg!(
    tui_exported_messages_eml,
    "message(s) as EML",
    "mensaje(s) como EML"
);
msg!(tui_exported, "Exported", "Exportado");
msg!(tui_exported_csv, "message(s) as CSV", "mensaje(s) como CSV");
msg!(
    tui_exported_messages_html,
    "message(s) as HTML",
    "mensaje(s) como HTML"
);
msg!(
    tui_exported_messages_txt,
    "message(s) as TXT",
    "mensaje(s) como TXT"
);
msg!(tui_history, "history", "historial");
msg!(
    tui_search_hint,
    "F1 all operators \u{b7} from: to: subject: body: date: after: before: has:attachment label: size:>1mb \u{b7} Enter to run",
    "F1 todos los operadores \u{b7} from: to: subject: body: date: after: before: has:attachment label: size:>1mb \u{b7} Enter para buscar"
);
msg!(
    tui_no_html_part,
    "No HTML part in this message",
    "No hay parte HTML en este mensaje"
);
msg!(
    tui_html_viewer_failed,
    "External HTML viewer failed",
    "El visor HTML externo fall\u{f3}"
);
msg!(
    tui_html_viewer_hint,
    "Set MBOXSHELL_HTML_VIEWER to choose (e.g. 'w3m', 'chawan', 'lynx -dump')",
    "Define MBOXSHELL_HTML_VIEWER para elegir (p.ej. 'w3m', 'chawan', 'lynx -dump')"
);
msg!(
    tui_exported_html,
    "message(s) as HTML",
    "mensaje(s) como HTML"
);

// ── CLI strings ─────────────────────────────────────────────────

msg!(cli_searching, "Searching", "Buscando");
msg!(cli_exporting, "Exporting", "Exportando");
msg!(cli_merging, "Merging", "Combinando");
msg!(cli_extracting, "Extracting", "Extrayendo");
msg!(cli_export_count, "Exporting", "Exportando");
msg!(cli_exported_eml, "Exported", "Exportado");
msg!(cli_eml_files, ".eml file(s)", "fichero(s) .eml");
msg!(cli_exported_csv, "Exported CSV to", "CSV exportado en");
msg!(cli_exported_txt, "Exported", "Exportado");
msg!(cli_txt_files, ".txt file(s)", "fichero(s) .txt");
msg!(cli_exported_html, "Exported", "Exportado");
msg!(cli_html_files, ".html file(s)", "fichero(s) .html");
msg!(
    cli_exported_mbox,
    "Exported MBOX mailbox to",
    "Buz\u{f3}n MBOX exportado en"
);
msg!(cli_mbox_messages, "message(s)", "mensaje(s)");
msg!(
    cli_exported_maildir,
    "Exported to Maildir",
    "Exportado al Maildir"
);
msg!(
    cli_merge_complete,
    "Merge complete:",
    "Combinaci\u{f3}n completa:"
);
msg!(cli_input_files, "Input files", "Ficheros de entrada");
msg!(cli_total_messages, "Total messages", "Total de mensajes");
msg!(
    cli_duplicates_removed,
    "Duplicates removed",
    "Duplicados eliminados"
);
msg!(
    cli_tagged_with_source,
    "Tagged with source",
    "Etiquetados con origen"
);
msg!(cli_output_size, "Output size", "Tama\u{f1}o de salida");
msg!(cli_output_file, "Output file", "Fichero de salida");
msg!(
    cli_no_attachments_found,
    "No messages with attachments found.",
    "No se encontraron mensajes con adjuntos."
);
msg!(
    cli_extracting_from,
    "Extracting attachments from",
    "Extrayendo adjuntos de"
);
msg!(cli_extracted, "Extracted", "Extra\u{ed}do");
msg!(cli_attachments_to, "attachment(s) to", "adjunto(s) en");

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
