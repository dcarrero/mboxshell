# Full Build Specification

## Meta-instrucciones para Claude Code

Este documento es el plan completo para construir `mbox-tui`, un lector de ficheros MBOX de terminal escrito en Rust. Léelo completo antes de empezar a programar. Cada fase incluye instrucciones exactas, estructuras de datos, algoritmos, tests requeridos y criterios de aceptación. No avances a la siguiente fase hasta que todos los criterios de la fase actual estén verificados con `cargo test` y `cargo clippy` sin warnings.

---

## Justificación de Rust como lenguaje

Se ha evaluado Rust vs Go vs C++ vs Zig para este proyecto:

| Criterio | Rust | Go | C++ | Zig |
|----------|------|----|-----|-----|
| Rendimiento con ficheros >50GB | ★★★★★ | ★★★☆☆ (GC pauses) | ★★★★★ | ★★★★★ |
| Memory-mapped I/O seguro | ★★★★★ | ★★★☆☆ | ★★★☆☆ (segfaults) | ★★★★☆ |
| Ecosistema TUI | ★★★★★ (ratatui) | ★★★★☆ (bubbletea) | ★★☆☆☆ | ★☆☆☆☆ |
| Parseo MIME/email | ★★★★☆ (mail-parser) | ★★★★☆ | ★★★☆☆ | ★☆☆☆☆ |
| Cross-compilation | ★★★★★ | ★★★★★ | ★★☆☆☆ | ★★★★☆ |
| Seguridad de memoria | ★★★★★ | ★★★★☆ | ★★☆☆☆ | ★★★☆☆ |
| Distribución (binario estático) | ★★★★★ | ★★★★★ | ★★★☆☆ | ★★★★★ |

**Decisión: Rust** — combinación única de rendimiento sin GC, seguridad de memoria (crítica con mmap de ficheros enormes), ecosistema maduro de TUI y parsing, y binarios estáticos multiplataforma.

---

## Descripción del proyecto

### Qué es
`mbox-tui` es una aplicación de terminal para leer, buscar y exportar correos desde ficheros MBOX de cualquier tamaño (desde KB hasta 100+ GB). Es la herramienta que falta en el ecosistema: no existe un visor de MBOX multiplataforma para terminal que maneje ficheros enormes.

### Qué NO es
- No es un cliente de correo (no envía ni recibe)
- No es un servidor IMAP/POP3
- No modifica el fichero MBOX original (solo lectura)
- No es un gestor de correo (no mueve mensajes entre carpetas)

### Usuarios objetivo
- Administradores de sistemas que necesitan analizar backups de correo
- Usuarios que exportan su correo de Gmail (Google Takeout) y quieren consultarlo
- Forenses digitales que analizan archivos de correo
- Cualquier persona con ficheros MBOX grandes que necesita buscar algo

### Formatos de entrada soportados
1. **MBOX (mboxrd)**: El formato principal. Ficheros donde cada mensaje empieza con una línea `From ` (RFC 4155). Es el formato de Thunderbird, Google Takeout, y la mayoría de servidores Unix.
2. **MBOX (mboxo)**: Variante antigua donde el escaping de `From ` en el body es inconsistente. Detectar automáticamente.
3. **EML**: Ficheros `.eml` individuales (un mensaje por fichero, sin línea `From `). RFC 5322.
4. **Directorios de EML**: Una carpeta con múltiples ficheros `.eml`.
5. **MBOX comprimidos**: Ficheros `.mbox.gz` — descomprimir al vuelo durante el parsing (feature opcional).

### Formato MBOX — Especificación detallada para el parser

Un fichero MBOX es texto plano. Cada mensaje empieza con una línea separadora:

```
From sender@example.com Thu Jan 01 00:00:00 2024
```

**Reglas exactas del separador:**
- La línea DEBE empezar con los 5 caracteres `From ` (F mayúscula, seguida de espacio)
- Solo es separador si está al principio del fichero O precedida por una línea vacía (`\n\n` o `\r\n\r\n`)
- El resto de la línea (dirección + fecha) NO es fiable — puede tener cualquier formato
- Después del separador viene el mensaje RFC 5322 completo (headers + body)
- El mensaje termina cuando aparece el siguiente separador o el final del fichero

**Escaping `From ` dentro del body (mboxrd):**
- Si una línea dentro del body empieza con `From `, se escapa como `>From `
- Si una línea empieza con `>From `, se escapa como `>>From ` (recursivo)
- Al leer, hay que revertir el escaping: quitar un nivel de `>`
- En mboxo (variante antigua), el escaping no es consistente — ser tolerante

**Problemas comunes en ficheros MBOX reales:**
- Líneas `From ` sin línea vacía previa (ficheros corruptos) — tratarlas como separador igualmente con un warning en el log
- Ficheros con mezcla de `\n` y `\r\n` como fin de línea — soportar ambos
- Mensajes truncados al final del fichero — no crashear, indexar hasta donde sea posible
- Caracteres nulos `\0` embebidos — tratarlos como parte del body
- BOM de UTF-8 al principio del fichero — ignorarlo
- Ficheros MBOX vacíos (0 bytes) — reportar como vacío, sin error

---

## FASE 1: Core — Parser, índice y CLI básico

### 1.1 Estructura del proyecto

Ejecutar exactamente:

```bash
cargo init mbox-tui
cd mbox-tui
```

Crear la siguiente estructura de directorios y ficheros:

```
mbox-tui/
├── Cargo.toml
├── README.md
├── LICENSE                    # MIT
├── .github/
│   └── workflows/
│       └── ci.yml             # CI para Linux, macOS, Windows
├── src/
│   ├── main.rs                # Entry point, CLI con clap
│   ├── lib.rs                 # Re-exporta módulos públicos
│   ├── error.rs               # Tipos de error centralizados
│   ├── parser/
│   │   ├── mod.rs             # Re-exporta submodulos
│   │   ├── mbox.rs            # Parser MBOX streaming
│   │   ├── eml.rs             # Parser para ficheros EML individuales
│   │   ├── mime.rs            # Decodificación MIME, multipart, charsets
│   │   └── header.rs          # Parsing de headers RFC 5322 (folding, encoded-words)
│   ├── index/
│   │   ├── mod.rs
│   │   ├── builder.rs         # Construcción del índice desde el parser
│   │   ├── reader.rs          # Lectura y consulta del índice
│   │   └── format.rs          # Formato binario del fichero de índice
│   ├── model/
│   │   ├── mod.rs
│   │   ├── mail.rs            # MailEntry, MailBody
│   │   ├── attachment.rs      # Attachment metadata
│   │   └── address.rs         # Parsing de direcciones email (RFC 5322 §3.4)
│   ├── store/
│   │   ├── mod.rs
│   │   └── reader.rs          # Lectura de mensajes individuales desde el MBOX por offset
│   ├── export/                # Fase 4
│   │   └── mod.rs
│   ├── search/                # Fase 3
│   │   └── mod.rs
│   └── tui/                   # Fase 2
│       └── mod.rs
├── tests/
│   ├── fixtures/
│   │   ├── simple.mbox        # 5 mensajes básicos texto plano
│   │   ├── multipart.mbox     # 3 mensajes con adjuntos y multipart
│   │   ├── charsets.mbox      # Mensajes con ISO-8859-1, Windows-1252, UTF-8, KOI8-R
│   │   ├── malformed.mbox     # Mensajes con headers corruptos, truncados
│   │   ├── gmail_takeout.mbox # Ejemplo con X-Gmail-Labels y formato Google
│   │   ├── empty.mbox         # Fichero vacío
│   │   ├── single.eml         # Un mensaje EML individual
│   │   ├── encoded_words.mbox # Subjects con =?UTF-8?B?...?= y =?ISO-8859-1?Q?...?=
│   │   └── large_attachment.mbox  # Mensaje con adjunto base64 de 10MB
│   ├── parser_tests.rs
│   ├── index_tests.rs
│   └── integration_tests.rs
└── benches/
    └── parsing.rs             # Benchmarks con criterion
```

### 1.2 Cargo.toml completo para Fase 1

```toml
[package]
name = "mbox-tui"
version = "0.1.0"
edition = "2021"
rust-version = "1.75"
description = "A fast terminal MBOX viewer for files of any size"
license = "MIT"
repository = "https://github.com/TU_USUARIO/mbox-tui"
keywords = ["mbox", "email", "tui", "terminal", "viewer"]
categories = ["command-line-utilities", "email"]

[dependencies]
# Parsing de email
mail-parser = "0.9"
encoding_rs = "0.8"

# Fecha y hora
chrono = { version = "0.4", features = ["serde"] }

# Serialización
serde = { version = "1.0", features = ["derive"] }
bincode = "1.3"

# Hashing para integridad del índice
sha2 = "0.10"

# I/O eficiente
memmap2 = "0.9"

# CLI
clap = { version = "4", features = ["derive", "env", "wrap_help"] }

# Errores
thiserror = "2"
anyhow = "1"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }

# Progreso
indicatif = "0.17"

# Directorios multiplataforma
dirs = "5"

# Utilidades
byteorder = "1"
humansize = "2"

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
tempfile = "3"
assert_fs = "1"
predicates = "3"

[[bench]]
name = "parsing"
harness = false
```

### 1.3 Tipos de error (src/error.rs)

```rust
use thiserror::Error;
use std::path::PathBuf;

#[derive(Error, Debug)]
pub enum MboxError {
    #[error("Error de I/O leyendo '{path}': {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Fichero MBOX no encontrado: {0}")]
    FileNotFound(PathBuf),

    #[error("El fichero no parece ser un MBOX válido: {0}")]
    InvalidMbox(PathBuf),

    #[error("Índice corrupto o incompatible para '{path}': {reason}")]
    InvalidIndex {
        path: PathBuf,
        reason: String,
    },

    #[error("Error parseando mensaje en offset {offset}: {reason}")]
    ParseError {
        offset: u64,
        reason: String,
    },

    #[error("Codificación no soportada: {0}")]
    UnsupportedEncoding(String),

    #[error("Error decodificando MIME: {0}")]
    MimeError(String),

    #[error("Operación cancelada por el usuario")]
    Cancelled,

    #[error("El fichero ha cambiado desde la última indexación")]
    FileModified,

    #[error("Error de exportación: {0}")]
    ExportError(String),

    #[error("Ruta no válida: {0}")]
    InvalidPath(String),
}

pub type Result<T> = std::result::Result<T, MboxError>;
```

### 1.4 Modelo de datos (src/model/)

#### src/model/address.rs
```rust
/// Dirección de email parseada según RFC 5322 §3.4
/// Ejemplo: "Juan García <juan@ejemplo.com>" → display_name="Juan García", address="juan@ejemplo.com"
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct EmailAddress {
    /// Nombre para mostrar (puede estar vacío)
    pub display_name: String,
    /// Dirección email pura (user@domain)
    pub address: String,
}

impl EmailAddress {
    /// Parsea una dirección email de un header.
    /// Soporta formatos:
    /// - "user@domain.com"
    /// - "<user@domain.com>"
    /// - "Display Name <user@domain.com>"
    /// - "\"Display, Name\" <user@domain.com>"
    /// Si no puede parsear, devuelve el string original como address.
    pub fn parse(raw: &str) -> Self { /* implementar */ }

    /// Parsea una lista de direcciones separadas por coma
    /// Ejemplo: "Juan <juan@a.com>, Maria <maria@b.com>"
    pub fn parse_list(raw: &str) -> Vec<Self> { /* implementar */ }

    /// Formato para mostrar: "Display Name <address>" o solo "address" si no hay nombre
    pub fn display(&self) -> String { /* implementar */ }
}
```

#### src/model/mail.rs
```rust
use chrono::{DateTime, Utc};
use super::address::EmailAddress;

/// Metadatos de un mensaje almacenados en el índice.
/// Se mantienen en memoria para toda la lista de mensajes.
/// IMPORTANTE: Mantener esta estructura lo más compacta posible.
/// Con 1 millón de mensajes y ~500 bytes por entrada = ~500 MB de RAM.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MailEntry {
    /// Posición en bytes del inicio del mensaje dentro del fichero MBOX
    /// (inicio de la línea "From ", no del primer header)
    pub offset: u64,

    /// Tamaño en bytes del mensaje completo (desde "From " hasta el siguiente "From " o EOF)
    pub length: u64,

    /// Fecha del mensaje. Parseada del header "Date:".
    /// Si no se puede parsear, usar la fecha del separador "From " o epoch.
    pub date: DateTime<Utc>,

    /// Remitente (primer From:)
    pub from: EmailAddress,

    /// Destinatarios principales (To:), truncado a los primeros 5
    pub to: Vec<EmailAddress>,

    /// Destinatarios en copia (CC:), truncado a los primeros 5
    pub cc: Vec<EmailAddress>,

    /// Asunto del mensaje, decodificado (encoded-words resueltos)
    pub subject: String,

    /// Message-ID único del mensaje (header Message-ID:)
    pub message_id: String,

    /// Message-ID del mensaje al que responde (header In-Reply-To:)
    pub in_reply_to: Option<String>,

    /// Lista de Message-IDs de la cadena de conversación (header References:)
    pub references: Vec<String>,

    /// ¿Tiene adjuntos? (detectado por Content-Type: multipart/mixed o similar)
    pub has_attachments: bool,

    /// Content-Type principal del mensaje (text/plain, multipart/mixed, etc.)
    pub content_type: String,

    /// Tamaño estimado del body en texto plano (para mostrar en la lista)
    pub text_size: u64,

    /// Gmail labels si existen (header X-Gmail-Labels:)
    pub labels: Vec<String>,

    /// Índice secuencial dentro del MBOX (0, 1, 2, ...)
    pub sequence: u64,
}

/// Cuerpo completo de un mensaje, cargado on-demand.
/// NO se almacena en el índice.
#[derive(Debug)]
pub struct MailBody {
    /// Texto plano del body (extraído de text/plain o convertido desde text/html)
    pub text: Option<String>,

    /// HTML del body (si existe text/html part)
    pub html: Option<String>,

    /// Headers completos como texto raw
    pub raw_headers: String,

    /// Lista de adjuntos con sus metadatos
    pub attachments: Vec<AttachmentMeta>,
}
```

#### src/model/attachment.rs
```rust
/// Metadatos de un adjunto. El contenido NO se carga hasta exportación.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AttachmentMeta {
    /// Nombre del fichero del adjunto. Si no tiene nombre, generar uno.
    pub filename: String,

    /// Content-Type del adjunto (image/jpeg, application/pdf, etc.)
    pub content_type: String,

    /// Tamaño del adjunto decodificado (estimado, puede no ser exacto hasta decodificar)
    pub size: u64,

    /// Content-Transfer-Encoding (base64, quoted-printable, 7bit, 8bit, binary)
    pub encoding: String,

    /// Content-ID si existe (para adjuntos inline referenciados desde HTML)
    pub content_id: Option<String>,

    /// ¿Es inline (embebido en el HTML) o adjunto real?
    pub is_inline: bool,

    /// Offset dentro del mensaje donde empieza el contenido codificado del adjunto.
    /// Relativo al inicio del mensaje en el MBOX.
    pub content_offset: u64,

    /// Longitud del contenido codificado en bytes
    pub content_length: u64,
}
```

### 1.5 Parser MBOX (src/parser/mbox.rs)

Implementar el parser con estas especificaciones exactas:

```rust
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::fs::File;

/// Tamaño del buffer de lectura. 128KB es óptimo para lectura secuencial en SSDs y HDDs.
const READ_BUFFER_SIZE: usize = 128 * 1024;

/// Callback invocado por cada mensaje encontrado durante el parsing.
/// Recibe: offset del mensaje, bytes crudos del mensaje completo.
/// Devuelve: si debe continuar (true) o abortar (false).
pub type MessageCallback = dyn FnMut(u64, &[u8]) -> bool;

/// Parser streaming de ficheros MBOX.
///
/// Diseño: Lee el fichero secuencialmente con un BufReader grande.
/// Nunca carga el fichero entero en memoria.
/// Para cada mensaje encontrado, invoca un callback con el offset y los bytes.
///
/// El parser es tolerante a errores:
/// - Ficheros con mezcla de \n y \r\n
/// - Líneas "From " sin línea vacía previa (warning pero acepta como separador)
/// - Mensajes truncados al final del fichero
/// - Caracteres nulos y binarios en el body
pub struct MboxParser {
    /// Ruta al fichero MBOX
    path: std::path::PathBuf,
    /// Tamaño total del fichero en bytes (para barra de progreso)
    file_size: u64,
}

impl MboxParser {
    /// Crea un nuevo parser para el fichero dado.
    /// Verifica que el fichero existe y es legible.
    /// NO verifica que sea un MBOX válido todavía.
    pub fn new(path: impl AsRef<Path>) -> crate::error::Result<Self> { /* implementar */ }

    /// Parsea el MBOX completo, invocando el callback para cada mensaje.
    ///
    /// Algoritmo detallado:
    /// 1. Abrir fichero con BufReader de READ_BUFFER_SIZE
    /// 2. Leer línea a línea
    /// 3. Detectar separador "From " al inicio de línea:
    ///    - SOLO es separador si:
    ///      a) Es la primera línea del fichero, O
    ///      b) La línea anterior estaba vacía (solo \n o \r\n)
    ///    - Si es un "From " sin línea vacía previa:
    ///      Loggear warning, pero tratarlo como separador igualmente
    ///      (muchos ficheros MBOX reales tienen este bug)
    /// 4. Cuando encuentra un separador:
    ///    a) Si había un mensaje acumulado, invocar callback con el mensaje previo
    ///    b) Registrar el offset del nuevo mensaje
    ///    c) Empezar a acumular bytes del nuevo mensaje
    /// 5. Al llegar a EOF, invocar callback con el último mensaje
    ///
    /// El offset reportado es la posición de la línea "From " en el fichero.
    ///
    /// Progress: Invocar progress_callback cada ~1MB leído para actualizar barra de progreso.
    pub fn parse(
        &self,
        message_callback: &mut MessageCallback,
        progress_callback: Option<&dyn Fn(u64, u64)>,  // (bytes_leídos, total)
    ) -> crate::error::Result<u64> { /* implementar, devuelve nº de mensajes */ }

    /// Parsea solo los headers de cada mensaje (más rápido que parsear completo).
    /// Útil para construir el índice sin decodificar bodies.
    ///
    /// Diferencia con parse(): al encontrar una línea vacía (fin de headers),
    /// deja de leer el body y salta al siguiente separador "From ".
    /// Pero DEBE seguir contando bytes para registrar la longitud del mensaje.
    pub fn parse_headers_only(
        &self,
        header_callback: &mut dyn FnMut(u64, u64, &[u8]) -> bool,  // offset, length, header_bytes
        progress_callback: Option<&dyn Fn(u64, u64)>,
    ) -> crate::error::Result<u64> { /* implementar */ }

    /// Lee un único mensaje dado su offset y longitud (desde el índice).
    /// Usa seek para posicionarse directamente, sin leer el fichero entero.
    pub fn read_message_at(path: impl AsRef<Path>, offset: u64, length: u64) -> crate::error::Result<Vec<u8>> {
        let mut file = File::open(path.as_ref()).map_err(|e| crate::error::MboxError::Io {
            path: path.as_ref().to_path_buf(),
            source: e,
        })?;
        file.seek(SeekFrom::Start(offset))?;
        let mut buffer = vec![0u8; length as usize];
        file.read_exact(&mut buffer)?;
        Ok(buffer)
    }
}
```

**IMPORTANTE sobre el buffer de mensajes**: Para ficheros enormes, un solo mensaje puede ser muy grande (emails con adjuntos de 100MB+). El parser debe tener un límite configurable de tamaño máximo de mensaje (default 256MB). Si un mensaje excede el límite, truncar el body y loggear un warning.

### 1.6 Parser de headers (src/parser/header.rs)

```rust
/// Parsea headers RFC 5322 desde bytes crudos.
///
/// Los headers tienen estas peculiaridades:
/// - "Header folding": un header puede continuar en la siguiente línea si empieza con espacio o tab
///   Ejemplo:
///   ```
///   Subject: Este es un asunto
///       muy largo que ocupa dos líneas
///   ```
///   Se interpreta como: "Subject: Este es un asunto muy largo que ocupa dos líneas"
///
/// - "Encoded words" (RFC 2047): texto no-ASCII en headers se codifica como:
///   =?charset?encoding?encoded_text?=
///   Donde encoding es B (base64) o Q (quoted-printable)
///   Ejemplo: =?UTF-8?B?SG9sYSBtdW5kbw==?= → "Hola mundo"
///   Ejemplo: =?ISO-8859-1?Q?caf=E9?= → "café"
///
/// - Los headers terminan en la primera línea vacía
///
/// - Puede haber headers duplicados (múltiples "Received:", etc.)
///
/// - El orden de los headers NO está garantizado

/// Extrae un MailEntry desde headers crudos en bytes.
/// Solo lee los headers que necesitamos para el índice.
pub fn parse_headers_to_entry(raw_headers: &[u8], offset: u64, message_length: u64, sequence: u64) -> crate::error::Result<crate::model::mail::MailEntry> {
    // 1. Decodificar bytes a string (intentar UTF-8, fallback a ISO-8859-1)
    // 2. Unfold headers (unir líneas continuadas)
    // 3. Extraer cada header relevante:
    //    - Date: → parsear a DateTime<Utc> usando múltiples formatos comunes
    //    - From: → EmailAddress::parse()
    //    - To: → EmailAddress::parse_list()
    //    - CC: → EmailAddress::parse_list()
    //    - Subject: → decodificar encoded-words
    //    - Message-ID: → extraer entre < >
    //    - In-Reply-To: → extraer entre < >
    //    - References: → extraer lista de Message-IDs
    //    - Content-Type: → detectar si es multipart (=tiene adjuntos)
    //    - X-Gmail-Labels: → split por coma, trim
    // 4. Construir MailEntry
    /* implementar */
}

/// Decodifica encoded-words (RFC 2047) en un string de header.
/// Ejemplo: "=?UTF-8?B?SG9sYQ==?= =?UTF-8?B?IG11bmRv?=" → "Hola mundo"
/// Si la decodificación falla, devuelve el string original sin modificar.
pub fn decode_encoded_words(input: &str) -> String { /* implementar */ }

/// Parsea una fecha de email. Soporta múltiples formatos porque en la realidad
/// los emails usan formatos muy variados:
/// - RFC 2822: "Thu, 01 Jan 2024 00:00:00 +0000"
/// - Sin día de la semana: "01 Jan 2024 00:00:00 +0000"
/// - Con nombre de zona: "Thu, 01 Jan 2024 00:00:00 EST"
/// - ISO 8601: "2024-01-01T00:00:00Z"
/// - Formatos rotos comunes: "Thu Jan  1 00:00:00 2024", "1/1/2024 12:00 AM"
/// Devuelve None si no puede parsear ninguno.
pub fn parse_date(date_str: &str) -> Option<chrono::DateTime<chrono::Utc>> { /* implementar */ }
```

### 1.7 Decodificación MIME (src/parser/mime.rs)

```rust
use crate::model::mail::MailBody;
use crate::model::attachment::AttachmentMeta;

/// Parsea un mensaje completo (headers + body) y extrae el contenido.
///
/// Usa la crate `mail-parser` internamente pero añade manejo de errores robusto
/// y fallbacks para mensajes malformados.
///
/// Algoritmo para extraer texto del body:
/// 1. Si Content-Type es text/plain → usar directamente (decodificar charset)
/// 2. Si Content-Type es text/html → guardar como html, generar texto plano con strip_tags
/// 3. Si Content-Type es multipart/alternative → preferir text/plain, fallback a text/html
/// 4. Si Content-Type es multipart/mixed → buscar la primera parte text/plain o text/html,
///    el resto son adjuntos
/// 5. Si Content-Type es multipart/related → como mixed pero los adjuntos inline
///    están referenciados desde el HTML via Content-ID
/// 6. Si Content-Type es message/rfc822 → parsear recursivamente el mensaje adjunto
/// 7. Profundidad máxima de recursión en multipart: 10 niveles (evitar ataques)
///
/// Para los charsets:
/// - Usar la crate `encoding_rs` para decodificar
/// - Si el charset declarado falla, intentar con UTF-8
/// - Si UTF-8 falla, intentar con ISO-8859-1 (acepta cualquier byte)
/// - Último recurso: reemplazar bytes inválidos con U+FFFD
pub fn parse_message_body(raw_message: &[u8]) -> crate::error::Result<MailBody> { /* implementar */ }

/// Lista los adjuntos de un mensaje SIN decodificar su contenido.
/// Solo extrae metadatos: nombre, tipo, tamaño, offset.
pub fn list_attachments(raw_message: &[u8]) -> crate::error::Result<Vec<AttachmentMeta>> { /* implementar */ }

/// Decodifica y extrae el contenido binario de un adjunto específico.
/// Lee solo la porción necesaria del mensaje.
pub fn extract_attachment(raw_message: &[u8], attachment: &AttachmentMeta) -> crate::error::Result<Vec<u8>> { /* implementar */ }

/// Convierte HTML a texto plano para mostrar en la terminal.
/// - Preserva saltos de línea de <br>, <p>, <div>
/// - Convierte <a href="url">text</a> a "text [url]"
/// - Convierte listas <li> a "• item"
/// - Elimina scripts y estilos
/// - Convierte entidades HTML (&amp; → &, etc.)
/// - Wrap a 80 columnas (configurable)
fn html_to_text(html: &str, width: usize) -> String { /* implementar */ }
```

### 1.8 Índice binario (src/index/)

#### Formato del fichero de índice (src/index/format.rs)

```rust
/// Formato del fichero de índice (.mbox-tui.idx):
///
/// ┌──────────────────────────────────────┐
/// │ HEADER (fijo, 64 bytes)              │
/// │  magic: [u8; 8] = b"MBOXTUI\0"      │
/// │  version: u32 = 1                     │
/// │  flags: u32                           │
/// │  message_count: u64                   │
/// │  mbox_file_size: u64                  │
/// │  mbox_modified_time: i64             │
/// │  sha256_first_4kb: [u8; 32]          │
/// │  ─── (total: 8+4+4+8+8+8+32 = 72)  │
/// │  ─── padding hasta 128 bytes ───     │
/// ├──────────────────────────────────────┤
/// │ ENTRIES (variable)                    │
/// │  Serializado con bincode:             │
/// │  Vec<MailEntry>                       │
/// └──────────────────────────────────────┘
///
/// Para verificar integridad:
/// 1. Comprobar magic bytes
/// 2. Comprobar version == VERSION_ACTUAL
/// 3. Comprobar que mbox_file_size coincida con el tamaño actual del fichero MBOX
/// 4. Comprobar que mbox_modified_time coincida con la fecha de modificación
/// 5. Comprobar SHA256 de los primeros 4KB del MBOX
///    (SHA256 completo sería demasiado lento para ficheros de 50GB+)
///
/// Si falla cualquier comprobación → re-indexar

pub const MAGIC: &[u8; 8] = b"MBOXTUI\0";
pub const VERSION: u32 = 1;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct IndexHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub flags: u32,
    pub message_count: u64,
    pub mbox_file_size: u64,
    pub mbox_modified_time: i64,
    pub sha256_first_4kb: [u8; 32],
}
```

#### Constructor del índice (src/index/builder.rs)

```rust
/// Construye el índice para un fichero MBOX.
///
/// Flujo completo:
/// 1. Comprobar si ya existe un índice válido → si sí, cargarlo y devolver
/// 2. Si no existe o es inválido:
///    a. Abrir el fichero MBOX
///    b. Calcular SHA256 de los primeros 4KB
///    c. Parsear headers de todos los mensajes (usando MboxParser::parse_headers_only)
///    d. Para cada mensaje, construir un MailEntry
///    e. Serializar todo el Vec<MailEntry> con bincode
///    f. Escribir el fichero de índice
/// 3. Devolver el Vec<MailEntry>
///
/// Ubicación del índice:
/// - Intentar: misma carpeta que el MBOX, con nombre: ".{nombre_mbox}.mbox-tui.idx"
///   (fichero oculto en Linux/macOS)
/// - Si no hay permisos de escritura: ~/.cache/mbox-tui/{sha256_path}.idx
/// - Si no se puede escribir en ningún sitio: warning y trabajar sin índice
pub fn build_index(
    mbox_path: &std::path::Path,
    force_rebuild: bool,
    progress: Option<&dyn Fn(u64, u64)>,
) -> anyhow::Result<Vec<crate::model::mail::MailEntry>> { /* implementar */ }

/// Intenta cargar un índice existente. Devuelve None si no existe o es inválido.
pub fn load_index(
    mbox_path: &std::path::Path,
) -> anyhow::Result<Option<Vec<crate::model::mail::MailEntry>>> { /* implementar */ }

/// Ruta donde se guardaría el fichero de índice para un MBOX dado.
fn index_path_for(mbox_path: &std::path::Path) -> std::path::PathBuf { /* implementar */ }

/// Ruta alternativa en ~/.cache/
fn cache_index_path_for(mbox_path: &std::path::Path) -> std::path::PathBuf { /* implementar */ }
```

### 1.9 Store / Reader (src/store/reader.rs)

```rust
/// Lee mensajes individuales desde un fichero MBOX usando el índice.
///
/// NO carga el MBOX en memoria. Para cada lectura:
/// 1. Abre el fichero (o reutiliza handle abierto)
/// 2. Hace seek al offset indicado
/// 3. Lee exactamente los bytes indicados por length
/// 4. Parsea el mensaje MIME
///
/// Implementa un cache LRU de mensajes decodificados:
/// - Tamaño configurable (default: 50 mensajes)
/// - Solo cachea el MailBody, no los bytes crudos
/// - Evita re-decodificar al navegar arriba/abajo en la lista
pub struct MboxStore {
    path: std::path::PathBuf,
    file: std::fs::File,
    cache: lru::LruCache<u64, crate::model::mail::MailBody>,  // key = offset
}

impl MboxStore {
    pub fn open(path: impl AsRef<std::path::Path>) -> crate::error::Result<Self> { /* implementar */ }

    /// Lee y parsea un mensaje dado su entry del índice.
    /// Usa cache LRU — si ya está cacheado, devuelve referencia.
    pub fn get_message(&mut self, entry: &crate::model::mail::MailEntry) -> crate::error::Result<&crate::model::mail::MailBody> { /* implementar */ }

    /// Lee los bytes crudos de un mensaje (para exportar como EML o ver raw).
    /// NO usa cache.
    pub fn get_raw_message(&mut self, entry: &crate::model::mail::MailEntry) -> crate::error::Result<Vec<u8>> { /* implementar */ }

    /// Extrae un adjunto decodificado de un mensaje.
    pub fn get_attachment(
        &mut self,
        entry: &crate::model::mail::MailEntry,
        attachment: &crate::model::attachment::AttachmentMeta,
    ) -> crate::error::Result<Vec<u8>> { /* implementar */ }
}
```

Añadir `lru = "0.12"` al Cargo.toml.

### 1.10 CLI básico (src/main.rs)

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mbox-tui")]
#[command(version, about = "Fast terminal viewer for MBOX files of any size")]
#[command(long_about = "mbox-tui is a terminal-based MBOX viewer optimized for large files (50GB+).\n\
    It creates a binary index for fast access and provides a TUI for browsing,\n\
    searching, and exporting emails.")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// MBOX file or directory to open (shortcut for 'open' command)
    #[arg(value_name = "FILE")]
    file: Option<std::path::PathBuf>,

    /// Verbose logging (-v info, -vv debug, -vvv trace)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

#[derive(Subcommand)]
enum Commands {
    /// Open a file in the TUI (default if no subcommand given)
    Open {
        /// MBOX file or directory containing MBOX/EML files
        path: std::path::PathBuf,
    },

    /// Index an MBOX file and show statistics
    Index {
        /// MBOX file to index
        path: std::path::PathBuf,

        /// Force rebuild even if index exists
        #[arg(short, long)]
        force: bool,
    },

    /// Show statistics about an MBOX file
    Stats {
        /// MBOX file to analyze
        path: std::path::PathBuf,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Search messages (Fase 3)
    Search {
        path: std::path::PathBuf,
        query: String,
        #[arg(long)]
        json: bool,
    },

    /// Export messages (Fase 4)
    Export {
        path: std::path::PathBuf,
        #[arg(short, long, default_value = "eml")]
        format: String,
        #[arg(short, long)]
        output: std::path::PathBuf,
        #[arg(long)]
        query: Option<String>,
    },

    /// Merge multiple MBOX files (Fase 4)
    Merge {
        /// Input MBOX files
        inputs: Vec<std::path::PathBuf>,
        /// Output MBOX file
        #[arg(short, long)]
        output: std::path::PathBuf,
        /// Remove duplicates by Message-ID
        #[arg(long, default_value = "true")]
        dedup: bool,
    },

    /// Extract all attachments (Fase 4)
    Attachments {
        path: std::path::PathBuf,
        #[arg(short, long)]
        output: std::path::PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Configurar logging según verbosidad
    let log_level = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level))
        )
        .init();

    match cli.command {
        Some(Commands::Index { path, force }) => cmd_index(&path, force),
        Some(Commands::Stats { path, json }) => cmd_stats(&path, json),
        Some(Commands::Open { path }) => cmd_open(&path),
        None => {
            if let Some(path) = cli.file {
                cmd_open(&path)
            } else {
                // Sin argumentos: abrir TUI con selector de fichero
                cmd_open_interactive()
            }
        }
        // Los demás se implementan en fases posteriores
        _ => {
            eprintln!("This command is not implemented yet. Coming in a future release.");
            Ok(())
        }
    }
}

fn cmd_index(path: &std::path::Path, force: bool) -> anyhow::Result<()> {
    // 1. Verificar que el fichero existe
    // 2. Mostrar barra de progreso con indicatif
    // 3. Llamar a build_index()
    // 4. Mostrar estadísticas:
    //    - Número de mensajes
    //    - Rango de fechas (primer y último mensaje)
    //    - Tamaño del fichero
    //    - Tamaño del índice
    //    - Tiempo de indexación
    //    - Top 10 remitentes
    //    - Mensajes con adjuntos
    /* implementar */
}

fn cmd_stats(path: &std::path::Path, json: bool) -> anyhow::Result<()> {
    // Cargar índice (construirlo si no existe)
    // Mostrar estadísticas detalladas en formato tabla o JSON
    /* implementar */
}

fn cmd_open(path: &std::path::Path) -> anyhow::Result<()> {
    // Fase 2 — por ahora imprimir mensaje
    eprintln!("TUI not yet implemented. Use 'mbox-tui index {}' to verify parsing.", path.display());
    Ok(())
}

fn cmd_open_interactive() -> anyhow::Result<()> {
    eprintln!("Interactive TUI not yet implemented. Pass a file path as argument.");
    Ok(())
}
```

### 1.11 Ficheros de test (tests/fixtures/)

Crear estos ficheros de test manualmente. Son fundamentales para verificar el parser.

#### tests/fixtures/simple.mbox
```
From user1@example.com Thu Jan 04 10:00:00 2024
From: User One <user1@example.com>
To: User Two <user2@example.com>
Subject: Hello World
Date: Thu, 04 Jan 2024 10:00:00 +0000
Message-ID: <msg001@example.com>
Content-Type: text/plain; charset=UTF-8

This is the first test message.
It has two lines.

From user2@example.com Thu Jan 04 11:00:00 2024
From: User Two <user2@example.com>
To: User One <user1@example.com>
Subject: Re: Hello World
Date: Thu, 04 Jan 2024 11:00:00 +0000
Message-ID: <msg002@example.com>
In-Reply-To: <msg001@example.com>
References: <msg001@example.com>
Content-Type: text/plain; charset=UTF-8

This is a reply.

From user3@example.com Fri Jan 05 09:00:00 2024
From: =?UTF-8?B?Sm9zw6kgR2FyY8Ota2E=?= <user3@example.com>
To: user1@example.com
Subject: =?UTF-8?Q?Caf=C3=A9_con_le=C3=B1a?=
Date: Fri, 05 Jan 2024 09:00:00 +0100
Message-ID: <msg003@example.com>
Content-Type: text/plain; charset=UTF-8

Mensaje con caracteres especiales: áéíóú ñ ü

From user4@example.com Fri Jan 05 14:00:00 2024
From: User Four <user4@example.com>
To: user1@example.com
Subject: Message with From in body
Date: Fri, 05 Jan 2024 14:00:00 +0000
Message-ID: <msg004@example.com>
Content-Type: text/plain; charset=UTF-8

This message has a tricky line:
>From the perspective of the user, this should not be a separator.
And this continues normally.

From user5@example.com Sat Jan 06 08:00:00 2024
From: User Five <user5@example.com>
To: user1@example.com, user2@example.com
Cc: user3@example.com
Subject: Meeting tomorrow
Date: Sat, 06 Jan 2024 08:00:00 -0500
Message-ID: <msg005@example.com>
Content-Type: text/plain; charset=US-ASCII

Please confirm attendance.
```

#### tests/fixtures/encoded_words.mbox
```
From sender@example.com Mon Feb 12 10:00:00 2024
From: =?ISO-8859-1?Q?Fran=E7ois?= <francois@example.com>
To: recipient@example.com
Subject: =?ISO-8859-1?Q?R=E9sum=E9_du_projet?=
Date: Mon, 12 Feb 2024 10:00:00 +0100
Message-ID: <enc001@example.com>
Content-Type: text/plain; charset=ISO-8859-1
Content-Transfer-Encoding: quoted-printable

Voici le r=E9sum=E9 du projet.

From sender2@example.com Mon Feb 12 11:00:00 2024
From: =?UTF-8?B?5bGx55Sw5aSq6YOO?= <yamada@example.jp>
To: recipient@example.com
Subject: =?UTF-8?B?44GK55+l44KJ44Gb?=
Date: Mon, 12 Feb 2024 11:00:00 +0900
Message-ID: <enc002@example.com>
Content-Type: text/plain; charset=UTF-8

日本語のテストメッセージです。

From sender3@example.com Mon Feb 12 12:00:00 2024
From: =?Windows-1252?Q?M=FCller?= <mueller@example.de>
To: recipient@example.com
Subject: =?Windows-1252?Q?Stra=DFenverkehr_=96_Bericht?=
Date: Mon, 12 Feb 2024 12:00:00 +0100
Message-ID: <enc003@example.com>
Content-Type: text/plain; charset=Windows-1252
Content-Transfer-Encoding: quoted-printable

Stra=DFenverkehr-Bericht f=FCr M=E4rz.
```

### 1.12 Tests requeridos (tests/parser_tests.rs)

```rust
/// TESTS MÍNIMOS REQUERIDOS — Todos deben pasar antes de avanzar a Fase 2

#[cfg(test)]
mod tests {
    use mbox_tui::parser::mbox::MboxParser;
    use mbox_tui::index::builder;
    use std::path::Path;

    /// Test 1: Parsear simple.mbox debe encontrar exactamente 5 mensajes
    #[test]
    fn test_parse_simple_mbox_count() { /* implementar */ }

    /// Test 2: El primer mensaje debe tener subject "Hello World", from "user1@example.com"
    #[test]
    fn test_parse_simple_mbox_first_message() { /* implementar */ }

    /// Test 3: El tercer mensaje tiene encoded-words en From y Subject
    /// From debe decodificarse a "José Garcíka" y Subject a "Café con leña"
    #[test]
    fn test_parse_encoded_words() { /* implementar */ }

    /// Test 4: El cuarto mensaje tiene ">From " en el body que NO debe ser separador
    #[test]
    fn test_from_escaping_in_body() { /* implementar */ }

    /// Test 5: Parsear un fichero vacío debe devolver 0 mensajes sin error
    #[test]
    fn test_parse_empty_mbox() { /* implementar */ }

    /// Test 6: El índice se genera y se puede recargar
    #[test]
    fn test_index_build_and_reload() {
        // 1. Indexar simple.mbox en un directorio temporal
        // 2. Verificar que el fichero .idx se creó
        // 3. Recargar el índice
        // 4. Verificar que tiene 5 entries
        // 5. Verificar que los datos coinciden
        /* implementar */
    }

    /// Test 7: Si el MBOX cambia, el índice se invalida
    #[test]
    fn test_index_invalidation() {
        // 1. Indexar simple.mbox en temp
        // 2. Modificar el MBOX (copiar y append un mensaje)
        // 3. Intentar cargar índice → debe devolver None (inválido)
        /* implementar */
    }

    /// Test 8: Parsear mensajes con charsets variados
    #[test]
    fn test_charset_decoding() {
        // Verificar que encoded_words.mbox se decodifica correctamente:
        // - ISO-8859-1 (François, résumé)
        // - UTF-8 (山田太郎, お知らせ)
        // - Windows-1252 (Müller, Straßenverkehr)
        /* implementar */
    }

    /// Test 9: Leer un mensaje específico por offset
    #[test]
    fn test_read_message_by_offset() {
        // 1. Indexar simple.mbox
        // 2. Leer el tercer mensaje por su offset
        // 3. Verificar que el body contiene "áéíóú ñ ü"
        /* implementar */
    }

    /// Test 10: Parsear EML individual
    #[test]
    fn test_parse_single_eml() {
        // Verificar que un fichero .eml se parsea como un solo mensaje
        /* implementar */
    }

    /// Test 11: Fechas en múltiples formatos
    #[test]
    fn test_date_parsing_formats() {
        // Verificar parsing de:
        // "Thu, 04 Jan 2024 10:00:00 +0000"
        // "04 Jan 2024 10:00:00 +0000"
        // "Thu, 04 Jan 2024 10:00:00 EST"
        // "2024-01-04T10:00:00Z"
        /* implementar */
    }

    /// Test 12: In-Reply-To y References se parsean correctamente
    #[test]
    fn test_threading_headers() {
        // El mensaje 2 de simple.mbox debe tener:
        // in_reply_to = Some("<msg001@example.com>")
        // references = vec!["<msg001@example.com>"]
        /* implementar */
    }
}
```

### 1.13 Benchmark (benches/parsing.rs)

```rust
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};

fn bench_parse_mbox(c: &mut Criterion) {
    let fixture_path = std::path::Path::new("tests/fixtures/simple.mbox");

    c.bench_function("parse_simple_mbox", |b| {
        b.iter(|| {
            let parser = mbox_tui::parser::mbox::MboxParser::new(fixture_path).unwrap();
            let mut count = 0u64;
            parser.parse(&mut |_offset, _bytes| { count += 1; true }, None).unwrap();
            count
        })
    });
}

fn bench_index_load(c: &mut Criterion) {
    let fixture_path = std::path::Path::new("tests/fixtures/simple.mbox");
    // Asegurar que el índice existe
    mbox_tui::index::builder::build_index(fixture_path, false, None).unwrap();

    c.bench_function("load_index_simple", |b| {
        b.iter(|| {
            mbox_tui::index::builder::load_index(fixture_path).unwrap()
        })
    });
}

criterion_group!(benches, bench_parse_mbox, bench_index_load);
criterion_main!(benches);
```

### Criterios de aceptación Fase 1

Antes de pasar a la Fase 2, verificar TODO esto:

- [ ] `cargo build` compila sin errores ni warnings
- [ ] `cargo clippy -- -D warnings` pasa limpio
- [ ] `cargo test` — todos los tests pasan
- [ ] `mbox-tui index tests/fixtures/simple.mbox` muestra: 5 mensajes, rango de fechas, top remitentes
- [ ] `mbox-tui stats tests/fixtures/simple.mbox` muestra estadísticas
- [ ] `mbox-tui stats tests/fixtures/simple.mbox --json` produce JSON válido
- [ ] El fichero de índice se genera junto al MBOX
- [ ] Re-ejecutar `mbox-tui index` sin `--force` carga el índice existente (< 100ms)
- [ ] Re-ejecutar con `--force` re-indexa desde cero
- [ ] El parser no crashea con ficheros malformados (probado con tests)
- [ ] Los encoded-words se decodifican correctamente
- [ ] Las fechas en múltiples formatos se parsean

---

## FASE 2: TUI — Interfaz de terminal interactiva

### 2.1 Dependencias adicionales

Añadir a Cargo.toml:

```toml
# TUI
ratatui = { version = "0.29", optional = true }
crossterm = { version = "0.28", optional = true }

# Renderizado de texto
html2text = "0.12"
textwrap = "0.16"
unicode-width = "0.2"

# Abrir URLs en navegador
open = "5"

# Cache LRU (si no se añadió antes)
lru = "0.12"

[features]
default = ["tui"]
tui = ["dep:ratatui", "dep:crossterm"]
```

### 2.2 Arquitectura de la TUI

La TUI sigue el patrón **Elm Architecture** (Model-Update-View), adaptado a Rust:

```
src/tui/
├── mod.rs           # Función principal run_tui() y bucle de eventos
├── app.rs           # Estado global de la aplicación (Model)
├── event.rs         # Manejo de eventos de teclado y ratón
├── ui.rs            # Renderizado principal (View) — distribuye a los widgets
├── widgets/
│   ├── mod.rs
│   ├── mail_list.rs     # Lista de mensajes (tabla virtual scrolleable)
│   ├── mail_view.rs     # Vista del contenido de un mensaje
│   ├── sidebar.rs       # Panel lateral con archivos y carpetas
│   ├── search_bar.rs    # Barra de búsqueda interactiva
│   ├── status_bar.rs    # Barra de estado inferior
│   ├── header_bar.rs    # Barra superior con info del archivo
│   ├── help_popup.rs    # Popup de ayuda con atajos
│   ├── attachment_popup.rs  # Lista de adjuntos
│   └── export_popup.rs     # Menú de exportación
├── keymap.rs        # Mapeo de teclas a acciones
├── theme.rs         # Definición de temas de colores
└── layout.rs        # Cálculo de áreas y layouts responsive
```

### 2.3 Estado de la aplicación (src/tui/app.rs)

```rust
use crate::model::mail::{MailEntry, MailBody};
use crate::store::reader::MboxStore;

/// Estado global de la TUI
pub struct App {
    // --- Datos ---
    /// Ruta al fichero MBOX abierto actualmente
    pub mbox_path: std::path::PathBuf,
    /// Índice completo de mensajes (en memoria)
    pub entries: Vec<MailEntry>,
    /// Índices de los mensajes actualmente visibles (tras aplicar filtros/búsqueda)
    pub visible_indices: Vec<usize>,
    /// Store para leer mensajes del MBOX
    pub store: MboxStore,

    // --- Navegación ---
    /// Índice del mensaje seleccionado actualmente dentro de visible_indices
    pub selected: usize,
    /// Offset de scroll en la lista de mensajes
    pub list_scroll_offset: usize,
    /// Offset de scroll en la vista del mensaje
    pub message_scroll_offset: usize,
    /// Mensajes marcados (seleccionados con Space) — conjunto de offsets
    pub marked: std::collections::HashSet<u64>,

    // --- UI State ---
    /// Qué panel tiene el foco actualmente
    pub focus: PanelFocus,
    /// Layout activo
    pub layout: LayoutMode,
    /// ¿Está el sidebar visible?
    pub show_sidebar: bool,
    /// ¿Está el popup de ayuda abierto?
    pub show_help: bool,
    /// ¿Está el popup de adjuntos abierto?
    pub show_attachments: bool,
    /// ¿Está el popup de exportación abierto?
    pub show_export: bool,
    /// ¿Mostrar headers completos en la vista del mensaje?
    pub show_full_headers: bool,
    /// ¿Mostrar mensaje raw (código fuente)?
    pub show_raw: bool,

    // --- Búsqueda ---
    /// ¿Está la barra de búsqueda activa?
    pub search_active: bool,
    /// Texto actual de búsqueda
    pub search_query: String,
    /// Resultados de búsqueda (índices en entries)
    pub search_results: Vec<usize>,
    /// Índice actual dentro de search_results
    pub search_result_index: usize,

    // --- Ordenación ---
    pub sort_column: SortColumn,
    pub sort_ascending: bool,

    // --- Mensaje actual cargado ---
    /// Body del mensaje seleccionado actualmente (cargado on-demand)
    pub current_body: Option<MailBody>,

    // --- Estado de la app ---
    pub should_quit: bool,
    /// Mensaje de estado para la barra inferior
    pub status_message: Option<(String, std::time::Instant)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanelFocus {
    Sidebar,
    MailList,
    MailView,
    SearchBar,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LayoutMode {
    /// Solo lista de mensajes
    ListOnly,
    /// Lista arriba, mensaje abajo
    HorizontalSplit,
    /// Lista a la izquierda, mensaje a la derecha
    VerticalSplit,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortColumn {
    Date,
    From,
    Subject,
    Size,
}

impl App {
    /// Crea una nueva App cargando un fichero MBOX.
    /// Construye el índice si no existe.
    pub fn new(mbox_path: std::path::PathBuf) -> anyhow::Result<Self> { /* implementar */ }

    /// Seleccionar mensaje y cargar su body
    pub fn select_message(&mut self, index: usize) -> anyhow::Result<()> {
        // 1. Actualizar self.selected
        // 2. Cargar body desde store: self.current_body = Some(self.store.get_message(&entry)?.clone())
        // 3. Resetear message_scroll_offset a 0
        /* implementar */
    }

    /// Ordenar la lista por la columna dada
    pub fn sort_by(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_column = column;
            self.sort_ascending = match column {
                SortColumn::Date => false, // más recientes primero por defecto
                _ => true,
            };
        }
        // Reordenar visible_indices según la columna y dirección
        /* implementar */
    }

    /// Toggle marcar/desmarcar mensaje actual
    pub fn toggle_mark(&mut self) {
        let offset = self.current_entry().offset;
        if self.marked.contains(&offset) {
            self.marked.remove(&offset);
        } else {
            self.marked.insert(offset);
        }
    }

    /// Obtener el entry actualmente seleccionado
    pub fn current_entry(&self) -> &MailEntry {
        let real_index = self.visible_indices[self.selected];
        &self.entries[real_index]
    }

    /// Número de mensajes visibles
    pub fn visible_count(&self) -> usize {
        self.visible_indices.len()
    }
}
```

### 2.4 Bucle principal de la TUI (src/tui/mod.rs)

```rust
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::prelude::*;
use std::time::Duration;

/// Ejecuta la TUI completa. Bloquea hasta que el usuario sale con 'q'.
pub fn run_tui(mbox_path: std::path::PathBuf) -> anyhow::Result<()> {
    // 1. Configurar terminal
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    // 2. Crear App (carga índice, puede tardar)
    // Mostrar mensaje "Indexing..." mientras tanto
    let app = App::new(mbox_path)?;

    // 3. Bucle principal
    let tick_rate = Duration::from_millis(100);
    let result = run_event_loop(&mut terminal, app, tick_rate);

    // 4. Restaurar terminal (SIEMPRE, incluso si hubo error)
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_event_loop(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    mut app: App,
    tick_rate: Duration,
) -> anyhow::Result<()> {
    loop {
        // Renderizar
        terminal.draw(|frame| {
            crate::tui::ui::render(frame, &mut app);
        })?;

        // Esperar evento
        if crossterm::event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                crate::tui::event::handle_key_event(&mut app, key)?;
            }
        }

        // Tick: limpiar mensajes de estado expirados, etc.
        app.tick();

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
```

### 2.5 Manejo de eventos de teclado (src/tui/event.rs)

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crate::tui::app::{App, PanelFocus, LayoutMode, SortColumn};

pub fn handle_key_event(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    // Si la búsqueda está activa, redirigir input al search bar
    if app.search_active {
        return handle_search_input(app, key);
    }

    // Si hay un popup abierto, manejar sus teclas
    if app.show_help {
        if key.code == KeyCode::Esc || key.code == KeyCode::Char('q') || key.code == KeyCode::Char('?') {
            app.show_help = false;
        }
        return Ok(());
    }
    if app.show_attachments {
        return handle_attachment_popup(app, key);
    }
    if app.show_export {
        return handle_export_popup(app, key);
    }

    // Atajos globales (funcionan en cualquier panel)
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c')) |
        (_, KeyCode::Char('q')) => {
            app.should_quit = true;
            return Ok(());
        }
        (_, KeyCode::Char('?')) => {
            app.show_help = true;
            return Ok(());
        }
        (_, KeyCode::Tab) => {
            // Rotar foco entre paneles
            app.focus = match app.focus {
                PanelFocus::Sidebar => PanelFocus::MailList,
                PanelFocus::MailList => PanelFocus::MailView,
                PanelFocus::MailView => {
                    if app.show_sidebar { PanelFocus::Sidebar } else { PanelFocus::MailList }
                }
                PanelFocus::SearchBar => PanelFocus::MailList,
            };
            return Ok(());
        }
        (_, KeyCode::Char('/')) => {
            app.search_active = true;
            app.search_query.clear();
            app.focus = PanelFocus::SearchBar;
            return Ok(());
        }
        (_, KeyCode::Char('1')) => { app.layout = LayoutMode::ListOnly; return Ok(()); }
        (_, KeyCode::Char('2')) => { app.layout = LayoutMode::HorizontalSplit; return Ok(()); }
        (_, KeyCode::Char('3')) => { app.layout = LayoutMode::VerticalSplit; return Ok(()); }
        (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
            // Forzar re-render completo
            return Ok(());
        }
        _ => {}
    }

    // Atajos específicos por panel
    match app.focus {
        PanelFocus::MailList => handle_mail_list_keys(app, key),
        PanelFocus::MailView => handle_mail_view_keys(app, key),
        PanelFocus::Sidebar => handle_sidebar_keys(app, key),
        _ => Ok(()),
    }
}

fn handle_mail_list_keys(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    match key.code {
        // Navegación
        KeyCode::Char('j') | KeyCode::Down => {
            if app.selected + 1 < app.visible_count() {
                app.select_message(app.selected + 1)?;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.selected > 0 {
                app.select_message(app.selected - 1)?;
            }
        }
        KeyCode::Char('g') | KeyCode::Home => {
            app.select_message(0)?;
        }
        KeyCode::Char('G') | KeyCode::End => {
            let last = app.visible_count().saturating_sub(1);
            app.select_message(last)?;
        }
        KeyCode::PageDown => {
            // Avanzar una página (tamaño del viewport)
            let page = app.list_viewport_height();
            let new_idx = (app.selected + page).min(app.visible_count().saturating_sub(1));
            app.select_message(new_idx)?;
        }
        KeyCode::PageUp => {
            let page = app.list_viewport_height();
            let new_idx = app.selected.saturating_sub(page);
            app.select_message(new_idx)?;
        }

        // Acciones
        KeyCode::Enter => {
            // Toggle entre ListOnly y HorizontalSplit, o foco en MailView
            if app.layout == LayoutMode::ListOnly {
                app.layout = LayoutMode::HorizontalSplit;
            }
            app.focus = PanelFocus::MailView;
        }
        KeyCode::Char(' ') => app.toggle_mark(),
        KeyCode::Char('*') => {
            // Seleccionar/deseleccionar todos
            if app.marked.len() == app.visible_count() {
                app.marked.clear();
            } else {
                for &idx in &app.visible_indices {
                    app.marked.insert(app.entries[idx].offset);
                }
            }
        }

        // Ordenación
        KeyCode::Char('s') => {
            // Rotar columna de ordenación
            let next = match app.sort_column {
                SortColumn::Date => SortColumn::From,
                SortColumn::From => SortColumn::Subject,
                SortColumn::Subject => SortColumn::Size,
                SortColumn::Size => SortColumn::Date,
            };
            app.sort_by(next);
        }
        KeyCode::Char('S') => {
            // Invertir orden actual
            app.sort_ascending = !app.sort_ascending;
            app.sort_by(app.sort_column);
        }

        // Funciones
        KeyCode::Char('a') => app.show_attachments = true,
        KeyCode::Char('e') => app.show_export = true,
        KeyCode::Char('h') => app.show_full_headers = !app.show_full_headers,
        KeyCode::Char('r') => app.show_raw = !app.show_raw,
        KeyCode::Char('t') => {
            // Toggle vista de threads (Fase 5)
            app.set_status("Threading: coming in a future update");
        }
        KeyCode::Char('n') => {
            // Siguiente resultado de búsqueda
            if !app.search_results.is_empty() {
                app.search_result_index = (app.search_result_index + 1) % app.search_results.len();
                let idx = app.search_results[app.search_result_index];
                // Encontrar la posición de idx en visible_indices
                if let Some(pos) = app.visible_indices.iter().position(|&i| i == idx) {
                    app.select_message(pos)?;
                }
            }
        }
        KeyCode::Char('N') => {
            // Anterior resultado de búsqueda
            if !app.search_results.is_empty() {
                app.search_result_index = if app.search_result_index == 0 {
                    app.search_results.len() - 1
                } else {
                    app.search_result_index - 1
                };
                let idx = app.search_results[app.search_result_index];
                if let Some(pos) = app.visible_indices.iter().position(|&i| i == idx) {
                    app.select_message(pos)?;
                }
            }
        }

        _ => {}
    }
    Ok(())
}

fn handle_mail_view_keys(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            app.message_scroll_offset += 1;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.message_scroll_offset = app.message_scroll_offset.saturating_sub(1);
        }
        KeyCode::PageDown => {
            app.message_scroll_offset += 20;
        }
        KeyCode::PageUp => {
            app.message_scroll_offset = app.message_scroll_offset.saturating_sub(20);
        }
        KeyCode::Char('g') | KeyCode::Home => {
            app.message_scroll_offset = 0;
        }
        KeyCode::Char('o') => {
            // Abrir URL bajo el cursor en el navegador
            // Detectar URLs en el texto visible, abrir la primera/seleccionada
            if let Some(url) = app.detect_url_at_cursor() {
                let _ = open::that(&url);
                app.set_status(&format!("Opened: {}", url));
            }
        }
        KeyCode::Esc => {
            // Volver a la lista
            app.focus = PanelFocus::MailList;
        }
        _ => {}
    }
    Ok(())
}

fn handle_search_input(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.search_active = false;
            app.focus = PanelFocus::MailList;
        }
        KeyCode::Enter => {
            // Ejecutar búsqueda
            app.execute_search()?;
            app.search_active = false;
            app.focus = PanelFocus::MailList;
        }
        KeyCode::Backspace => {
            app.search_query.pop();
        }
        KeyCode::Char(c) => {
            app.search_query.push(c);
        }
        _ => {}
    }
    Ok(())
}
```

### 2.6 Renderizado (src/tui/ui.rs)

```rust
use ratatui::prelude::*;
use ratatui::widgets::*;

/// Función principal de renderizado. Distribuye el frame entre los widgets.
pub fn render(frame: &mut Frame, app: &mut App) {
    let size = frame.area();

    // Layout principal
    let main_areas = if app.show_sidebar {
        // Sidebar (20%) | Contenido (80%)
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Percentage(80),
            ])
            .split(size)
    } else {
        // Sin sidebar — todo el ancho para contenido
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0)])
            .split(size)
    };

    // Renderizar sidebar si está visible
    if app.show_sidebar {
        crate::tui::widgets::sidebar::render(frame, app, main_areas[0]);
    }

    let content_area = if app.show_sidebar { main_areas[1] } else { main_areas[0] };

    // Header bar (1 línea) + Content + Status bar (1 línea)
    let vertical_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // header bar
            Constraint::Min(5),     // content
            Constraint::Length(1),  // status bar / search bar
        ])
        .split(content_area);

    crate::tui::widgets::header_bar::render(frame, app, vertical_layout[0]);

    // Content area: depende del layout
    match app.layout {
        LayoutMode::ListOnly => {
            crate::tui::widgets::mail_list::render(frame, app, vertical_layout[1]);
        }
        LayoutMode::HorizontalSplit => {
            let split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                .split(vertical_layout[1]);
            crate::tui::widgets::mail_list::render(frame, app, split[0]);
            crate::tui::widgets::mail_view::render(frame, app, split[1]);
        }
        LayoutMode::VerticalSplit => {
            let split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                .split(vertical_layout[1]);
            crate::tui::widgets::mail_list::render(frame, app, split[0]);
            crate::tui::widgets::mail_view::render(frame, app, split[1]);
        }
    }

    // Status bar o search bar
    if app.search_active {
        crate::tui::widgets::search_bar::render(frame, app, vertical_layout[2]);
    } else {
        crate::tui::widgets::status_bar::render(frame, app, vertical_layout[2]);
    }

    // Popups (se renderizan encima de todo)
    if app.show_help {
        crate::tui::widgets::help_popup::render(frame, app);
    }
    if app.show_attachments {
        crate::tui::widgets::attachment_popup::render(frame, app);
    }
    if app.show_export {
        crate::tui::widgets::export_popup::render(frame, app);
    }
}
```

### 2.7 Widget de lista de mensajes (src/tui/widgets/mail_list.rs)

```rust
/// Renderiza la lista de mensajes como una tabla scrolleable virtual.
///
/// RENDIMIENTO CRÍTICO: Con 1 millón de mensajes, solo renderizar las filas visibles.
/// No crear 1M de Row objects.
///
/// Algoritmo:
/// 1. Calcular qué filas son visibles según scroll_offset y viewport height
/// 2. Solo crear Row objects para esas filas
/// 3. Usar StatefulWidget con TableState para el scroll
///
/// Columnas:
/// [ ★ ] [ Date           ] [ From              ] [ Subject                    ] [ Size ] [ 📎 ]
///
/// Formato:
/// - ★: espacio vacío o "•" si está marcado
/// - Date: formateada según config, default "2024-01-04 10:00"
/// - From: display_name si existe, sino address, truncado al ancho
/// - Subject: truncado al ancho disponible (columna flexible)
/// - Size: humanizado "1.2 KB", "3.4 MB"
/// - 📎: "📎" si has_attachments, vacío si no
///
/// Colores:
/// - Fila seleccionada: inversión de colores (bg/fg swap)
/// - Filas marcadas: color amarillo en el indicador ★
/// - Resultado de búsqueda: resaltado del texto que coincide
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    // Implementar con ratatui::widgets::Table y TableState
    // Usar virtual scrolling
    /* implementar */
}
```

### 2.8 Widget de vista de mensaje (src/tui/widgets/mail_view.rs)

```rust
/// Renderiza el contenido de un mensaje con scroll.
///
/// Layout del contenido:
/// ```
/// ┌─ Message ──────────────────────────────────────────────┐
/// │ Date:    Thu, 04 Jan 2024 10:00:00 +0000               │
/// │ From:    User One <user1@example.com>                   │
/// │ To:      User Two <user2@example.com>                   │
/// │ Subject: Hello World                                    │
/// │ ──────────────────────────────────────────────────────── │
/// │                                                         │
/// │ This is the message body text.                          │
/// │ It wraps automatically to fit the terminal width.       │
/// │                                                         │
/// │ [Attachments: 2 files]                                  │
/// │  📎 document.pdf (1.2 MB)                               │
/// │  📎 image.jpg (340 KB)                                  │
/// └─────────────────────────────────────────────────────────┘
/// ```
///
/// Comportamiento:
/// - Si show_full_headers: mostrar TODOS los headers
/// - Si show_raw: mostrar el mensaje crudo completo (headers + body codificado)
/// - El texto se wrappea al ancho del área
/// - URLs se detectan y resaltan con color cyan subrayado
/// - Texto de búsqueda se resalta en amarillo sobre negro
/// - Scroll con j/k cuando este panel tiene foco
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    /* implementar */
}

/// Detecta URLs en un texto y devuelve sus posiciones
fn find_urls(text: &str) -> Vec<(usize, usize, String)> {
    // Regex simple para http(s)://, ftp://, mailto:
    /* implementar */
}
```

### 2.9 Otros widgets

Implementar cada widget en su fichero correspondiente. Detalles clave:

**header_bar.rs**: Una línea con: nombre del fichero | N mensajes | filtro activo (si hay) | "[?] Help"

**status_bar.rs**: Una línea con: mensaje de estado temporal (si hay) O atajos rápidos: "j/k:Nav  /:Search  e:Export  ?:Help  q:Quit"

**search_bar.rs**: Prompt "/: " seguido del texto que escribe el usuario, con cursor visible

**sidebar.rs**: Lista de ficheros .mbox/.eml en el directorio, con el fichero actual resaltado y número de mensajes

**help_popup.rs**: Popup centrado (80% del ancho, 80% del alto) con tabla de todos los atajos de teclado, scrolleable

**attachment_popup.rs**: Popup con lista de adjuntos del mensaje actual, seleccionable con j/k, exportable con Enter

### 2.10 Tema de colores (src/tui/theme.rs)

```rust
use ratatui::style::{Color, Modifier, Style};

pub struct Theme {
    pub header_bar_bg: Color,
    pub header_bar_fg: Color,
    pub status_bar_bg: Color,
    pub status_bar_fg: Color,
    pub list_selected_bg: Color,
    pub list_selected_fg: Color,
    pub list_marked: Color,
    pub list_header_bg: Color,
    pub list_header_fg: Color,
    pub sidebar_bg: Color,
    pub sidebar_fg: Color,
    pub sidebar_selected: Color,
    pub message_header_fg: Color,
    pub message_body_fg: Color,
    pub url_fg: Color,
    pub search_highlight_bg: Color,
    pub search_highlight_fg: Color,
    pub attachment_fg: Color,
    pub border_fg: Color,
    pub popup_bg: Color,
    pub popup_fg: Color,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            header_bar_bg: Color::Rgb(30, 30, 40),
            header_bar_fg: Color::Rgb(200, 200, 220),
            status_bar_bg: Color::Rgb(30, 30, 40),
            status_bar_fg: Color::Rgb(150, 150, 170),
            list_selected_bg: Color::Rgb(60, 60, 100),
            list_selected_fg: Color::White,
            list_marked: Color::Yellow,
            list_header_bg: Color::Rgb(40, 40, 60),
            list_header_fg: Color::Rgb(180, 180, 200),
            sidebar_bg: Color::Reset,
            sidebar_fg: Color::Rgb(180, 180, 200),
            sidebar_selected: Color::Cyan,
            message_header_fg: Color::Rgb(130, 170, 255),
            message_body_fg: Color::Rgb(220, 220, 230),
            url_fg: Color::Cyan,
            search_highlight_bg: Color::Yellow,
            search_highlight_fg: Color::Black,
            attachment_fg: Color::Green,
            border_fg: Color::Rgb(80, 80, 100),
            popup_bg: Color::Rgb(20, 20, 35),
            popup_fg: Color::Rgb(220, 220, 230),
        }
    }

    pub fn light() -> Self {
        Self {
            header_bar_bg: Color::Rgb(220, 220, 235),
            header_bar_fg: Color::Rgb(30, 30, 50),
            status_bar_bg: Color::Rgb(220, 220, 235),
            status_bar_fg: Color::Rgb(80, 80, 100),
            list_selected_bg: Color::Rgb(180, 200, 240),
            list_selected_fg: Color::Black,
            list_marked: Color::Rgb(180, 140, 0),
            list_header_bg: Color::Rgb(200, 200, 220),
            list_header_fg: Color::Rgb(30, 30, 50),
            sidebar_bg: Color::Reset,
            sidebar_fg: Color::Rgb(40, 40, 60),
            sidebar_selected: Color::Blue,
            message_header_fg: Color::Rgb(0, 70, 180),
            message_body_fg: Color::Rgb(20, 20, 30),
            url_fg: Color::Blue,
            search_highlight_bg: Color::Rgb(255, 230, 0),
            search_highlight_fg: Color::Black,
            attachment_fg: Color::Rgb(0, 130, 0),
            border_fg: Color::Rgb(150, 150, 170),
            popup_bg: Color::Rgb(245, 245, 250),
            popup_fg: Color::Rgb(20, 20, 30),
        }
    }
}
```

### Criterios de aceptación Fase 2

- [ ] `mbox-tui tests/fixtures/simple.mbox` abre la TUI correctamente
- [ ] Navegar con j/k/↑/↓ es fluido (sin lag perceptible)
- [ ] Seleccionar un mensaje muestra su contenido en el panel inferior
- [ ] Los atajos de teclado funcionan según la especificación
- [ ] El layout se adapta al redimensionar el terminal
- [ ] Los 3 modos de layout (1/2/3) cambian correctamente
- [ ] La barra de búsqueda acepta input y filtra mensajes
- [ ] `?` muestra el popup de ayuda con todos los atajos
- [ ] `q` sale limpiamente (terminal restaurado sin corrupción)
- [ ] Los mensajes con UTF-8 y otros charsets se muestran correctamente
- [ ] Funciona en: Linux (GNOME Terminal), macOS (Terminal.app/iTerm2), Windows (Windows Terminal)

---

## FASE 3: Búsqueda avanzada

### 3.1 Motor de búsqueda (src/search/)

```
src/search/
├── mod.rs           # API pública de búsqueda
├── query.rs         # Parser de queries de búsqueda
├── metadata.rs      # Búsqueda en metadatos del índice (rápida)
├── fulltext.rs      # Búsqueda full-text streaming (lenta pero sin índice)
└── tantivy_index.rs # Índice full-text con tantivy (feature opcional)
```

### 3.2 Sintaxis de búsqueda (src/search/query.rs)

```rust
/// Sintaxis de búsqueda soportada:
///
/// Búsqueda simple:
///   "texto"  → busca en subject, from, to (metadatos)
///
/// Búsqueda por campo:
///   from:usuario@ejemplo.com    → busca en campo From
///   to:destino@ejemplo.com      → busca en campo To
///   cc:copia@ejemplo.com        → busca en campo CC
///   subject:factura              → busca en Subject
///   body:texto importante        → busca en el body del mensaje (full-text)
///   has:attachment               → mensajes con adjuntos
///   has:no-attachment             → mensajes sin adjuntos
///   label:inbox                  → mensajes con ese Gmail label
///   filename:informe.pdf         → mensajes con adjunto de ese nombre
///   id:<message-id@domain>       → búsqueda exacta por Message-ID
///
/// Filtros de fecha:
///   date:2024-01-01              → mensajes de ese día exacto
///   date:2024-01                 → mensajes de ese mes
///   date:2024                    → mensajes de ese año
///   date:2024-01-01..2024-06-30  → rango de fechas (inclusivo)
///   before:2024-06-01            → antes de esa fecha
///   after:2024-01-01             → después de esa fecha
///
/// Filtros de tamaño:
///   size:>1mb                    → mensajes de más de 1 MB
///   size:<100kb                  → mensajes de menos de 100 KB
///
/// Operadores:
///   término1 término2            → AND implícito (ambos deben coincidir)
///   término1 OR término2         → OR explícito
///   -término                     → NOT (excluir mensajes que coincidan)
///   "frase exacta"               → buscar la frase completa
///
/// Ejemplos combinados:
///   from:juan subject:presupuesto date:2024-01..2024-06
///   from:maria has:attachment -subject:spam
///   body:"número de referencia" date:2024

#[derive(Debug, Clone)]
pub enum SearchField {
    All,          // Busca en subject + from + to
    From,
    To,
    Cc,
    Subject,
    Body,
    Label,
    Filename,
    MessageId,
}

#[derive(Debug, Clone)]
pub enum SearchOperator {
    Contains(String),
    Exact(String),        // Entre comillas
    Regex(String),        // /patrón/ (avanzado)
}

#[derive(Debug, Clone)]
pub enum DateFilter {
    Exact(chrono::NaiveDate),
    Range(chrono::NaiveDate, chrono::NaiveDate),
    Before(chrono::NaiveDate),
    After(chrono::NaiveDate),
}

#[derive(Debug, Clone)]
pub enum SizeFilter {
    GreaterThan(u64),
    LessThan(u64),
}

#[derive(Debug, Clone)]
pub struct SearchTerm {
    pub field: SearchField,
    pub operator: SearchOperator,
    pub negated: bool,
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub terms: Vec<SearchTerm>,
    pub date_filter: Option<DateFilter>,
    pub size_filter: Option<SizeFilter>,
    pub has_attachment: Option<bool>,
    /// Si algún término busca en Body, necesita full-text search
    pub needs_fulltext: bool,
}

/// Parsea una query string del usuario a una SearchQuery estructurada.
/// Nunca falla: si no puede parsear algo, lo trata como búsqueda general.
pub fn parse_query(input: &str) -> SearchQuery { /* implementar */ }
```

### 3.3 Búsqueda en metadatos (src/search/metadata.rs)

```rust
/// Búsqueda rápida sobre los metadatos del índice (en memoria).
/// No accede al fichero MBOX.
///
/// Complejidad: O(n) donde n = número de mensajes.
/// Para 1M de mensajes debería completar en < 200ms.
pub fn search_metadata(
    entries: &[MailEntry],
    query: &SearchQuery,
) -> Vec<usize> {
    // Para cada entry, comprobar si coincide con todos los terms (AND)
    // Devolver los índices de los entries que coinciden
    //
    // Optimizaciones:
    // 1. Si hay date_filter, filtrar primero por fecha (más rápido que string matching)
    // 2. Usar .to_lowercase() una sola vez y cachear
    // 3. Para búsquedas "Contains", usar str::contains (que internamente usa SIMD en Rust)
    // 4. Short-circuit: si un AND term no coincide, saltar al siguiente entry
    /* implementar */
}
```

### 3.4 Búsqueda full-text streaming (src/search/fulltext.rs)

```rust
/// Búsqueda full-text que lee cada mensaje del MBOX.
///
/// Flujo:
/// 1. Filtrar primero por metadatos (si hay términos de metadatos en la query)
/// 2. Para cada candidato, leer el mensaje del MBOX por offset
/// 3. Decodificar MIME, extraer texto plano
/// 4. Buscar el texto
/// 5. Reportar progreso
///
/// CANCELABLE: el callback de progreso devuelve bool.
/// Si devuelve false, abortar la búsqueda.
pub fn search_fulltext(
    mbox_path: &std::path::Path,
    entries: &[MailEntry],
    candidates: &[usize],  // Pre-filtrados por metadatos
    query: &SearchQuery,
    progress: &dyn Fn(usize, usize) -> bool,  // (procesados, total) → continuar?
) -> crate::error::Result<Vec<usize>> {
    // Usar aho_corasick para buscar múltiples patrones a la vez
    // Leer mensajes en batches para mejorar I/O secuencial
    /* implementar */
}
```

### 3.5 Índice full-text con tantivy (src/search/tantivy_index.rs) — Feature opcional

```rust
#[cfg(feature = "fulltext")]
pub mod tantivy_search {
    /// Construye un índice tantivy para un MBOX.
    /// El índice se almacena en ~/.cache/mbox-tui/<hash>/tantivy/
    ///
    /// Campos indexados:
    /// - from (TEXT)
    /// - to (TEXT)
    /// - subject (TEXT)
    /// - body (TEXT)
    /// - date (DATE)
    /// - message_offset (u64, STORED, para mapear resultados al índice principal)
    pub fn build_fulltext_index(
        mbox_path: &std::path::Path,
        entries: &[MailEntry],
        store: &mut MboxStore,
        progress: &dyn Fn(usize, usize) -> bool,
    ) -> anyhow::Result<()> { /* implementar */ }

    /// Busca en el índice tantivy. Devuelve offsets de mensajes.
    pub fn search_tantivy(
        mbox_path: &std::path::Path,
        query_str: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<u64>> { /* implementar */ }
}
```

### Criterios de aceptación Fase 3

- [ ] `from:user1@example.com` encuentra mensajes del remitente
- [ ] `subject:Hello date:2024-01` filtra correctamente por ambos campos
- [ ] `has:attachment` muestra solo mensajes con adjuntos
- [ ] `-subject:spam` excluye mensajes con "spam" en el asunto
- [ ] `body:caracteres` busca en el cuerpo del mensaje
- [ ] La barra de progreso funciona durante búsqueda full-text
- [ ] `Esc` cancela una búsqueda en curso
- [ ] `n`/`N` navegan entre resultados de búsqueda
- [ ] Los resultados se resaltan en la lista y en la vista del mensaje
- [ ] `mbox-tui search archivo.mbox "from:user1"` funciona desde CLI
- [ ] `mbox-tui search archivo.mbox "from:user1" --json` produce JSON válido

---

## FASE 4: Exportación y operaciones batch

### 4.1 Módulo de exportación (src/export/)

```
src/export/
├── mod.rs          # API pública y tipos
├── eml.rs          # Exportar a .eml
├── mbox.rs         # Exportar a .mbox (merge, subset)
├── csv.rs          # Exportar resumen a CSV
├── html.rs         # Exportar mensaje como HTML standalone
├── text.rs         # Exportar como texto plano
├── attachment.rs   # Extraer adjuntos
└── pdf.rs          # Exportar a PDF (opcional, best-effort)
```

### 4.2 Formatos de exportación detallados

#### EML (src/export/eml.rs)
```rust
/// Exporta un mensaje como fichero .eml
/// 
/// El .eml es simplemente los bytes crudos del mensaje RFC 5322
/// (sin la línea "From " del MBOX).
/// 
/// Nombre del fichero: sanitizar subject como nombre de fichero.
/// Formato: "{date}_{from}_{subject}.eml"
/// Ejemplo: "2024-01-04_user1@example.com_Hello_World.eml"
/// Sanitización: reemplazar caracteres no válidos (/\:*?"<>|) con _
/// Truncar a 200 caracteres máximo.
pub fn export_eml(
    raw_bytes: &[u8],
    entry: &MailEntry,
    output_dir: &std::path::Path,
) -> crate::error::Result<std::path::PathBuf> { /* implementar */ }

/// Exporta múltiples mensajes a ficheros .eml en una carpeta
pub fn export_multiple_eml(
    store: &mut MboxStore,
    entries: &[&MailEntry],
    output_dir: &std::path::Path,
    progress: &dyn Fn(usize, usize),
) -> crate::error::Result<Vec<std::path::PathBuf>> { /* implementar */ }
```

#### CSV (src/export/csv.rs)
```rust
/// Exporta un resumen de mensajes a CSV.
/// 
/// Columnas: Date, From, To, CC, Subject, Size, Has_Attachments, Labels, Message_ID
/// 
/// Encoding: UTF-8 con BOM (para que Excel lo abra bien)
/// Separador: coma (configurable)
/// Escape: comillas dobles según RFC 4180
pub fn export_csv(
    entries: &[&MailEntry],
    output_path: &std::path::Path,
    include_snippet: bool,  // Si true, incluir primeros 200 chars del body
    store: Option<&mut MboxStore>,  // Necesario solo si include_snippet
) -> crate::error::Result<()> { /* implementar */ }
```

#### HTML (src/export/html.rs)
```rust
/// Exporta un mensaje como HTML standalone.
/// 
/// Si el mensaje tiene body HTML, usarlo directamente con headers inyectados.
/// Si solo tiene texto plano, wrappear en <pre>.
/// 
/// El HTML incluye:
/// - <!DOCTYPE html> con charset UTF-8
/// - CSS embebido para headers y body
/// - Headers del mensaje en una tabla
/// - Body del mensaje
/// - Lista de adjuntos (como links si se exportaron junto)
pub fn export_html(
    entry: &MailEntry,
    body: &MailBody,
    output_path: &std::path::Path,
    attachments_dir: Option<&std::path::Path>,
) -> crate::error::Result<()> { /* implementar */ }
```

#### Adjuntos (src/export/attachment.rs)
```rust
/// Extrae un adjunto individual y lo guarda en disco.
/// Decodifica base64/quoted-printable automáticamente.
pub fn export_attachment(
    store: &mut MboxStore,
    entry: &MailEntry,
    attachment: &AttachmentMeta,
    output_dir: &std::path::Path,
) -> crate::error::Result<std::path::PathBuf> { /* implementar */ }

/// Extrae todos los adjuntos de un mensaje.
pub fn export_all_attachments(
    store: &mut MboxStore,
    entry: &MailEntry,
    output_dir: &std::path::Path,
) -> crate::error::Result<Vec<std::path::PathBuf>> { /* implementar */ }

/// Extrae todos los adjuntos de múltiples mensajes.
/// Crea subcarpetas por mensaje: {output_dir}/{date}_{subject}/
pub fn export_bulk_attachments(
    store: &mut MboxStore,
    entries: &[&MailEntry],
    output_dir: &std::path::Path,
    progress: &dyn Fn(usize, usize),
) -> crate::error::Result<Vec<std::path::PathBuf>> { /* implementar */ }
```

#### Merge (src/export/mbox.rs)
```rust
/// Merge múltiples ficheros MBOX en uno solo.
///
/// Algoritmo:
/// 1. Indexar cada fichero de entrada
/// 2. Recopilar todos los Message-IDs
/// 3. Si dedup=true, eliminar duplicados (mantener la primera ocurrencia)
/// 4. Escribir el MBOX de salida secuencialmente
/// 5. Reportar estadísticas: total, duplicados eliminados, tamaño final
pub fn merge_mbox_files(
    inputs: &[std::path::PathBuf],
    output: &std::path::Path,
    dedup: bool,
    progress: &dyn Fn(usize, usize, &str),  // (fichero_actual, total_ficheros, nombre)
) -> crate::error::Result<MergeStats> { /* implementar */ }

pub struct MergeStats {
    pub total_messages: u64,
    pub duplicates_removed: u64,
    pub output_size: u64,
    pub input_files: usize,
}
```

### 4.3 Implementar comandos CLI de la Fase 4

Completar los handlers en main.rs para: `export`, `merge`, `attachments`.

Cada comando CLI debe:
- Mostrar barra de progreso con `indicatif`
- Soportar `--json` para salida máquina
- Manejar errores gracefully (no panic)
- Funcionar bien en pipes (`mbox-tui search file.mbox "query" --json | jq .`)

### Criterios de aceptación Fase 4

- [ ] `mbox-tui export tests/fixtures/simple.mbox --format eml --output /tmp/emails/` crea 5 ficheros .eml
- [ ] Los .eml exportados se abren correctamente en Thunderbird
- [ ] `mbox-tui export ... --format csv --output resumen.csv` crea un CSV válido con BOM UTF-8
- [ ] El CSV se abre correctamente en Excel y LibreOffice Calc
- [ ] `mbox-tui attachments tests/fixtures/multipart.mbox --output /tmp/att/` extrae adjuntos decodificados
- [ ] `mbox-tui merge file1.mbox file2.mbox -o merged.mbox` produce un MBOX válido
- [ ] El merge con `--dedup` elimina duplicados por Message-ID
- [ ] Exportar desde la TUI con `e` funciona (EML, TXT)
- [ ] `E` con mensajes marcados exporta múltiples
- [ ] `a` muestra adjuntos y permite exportarlos individualmente

---

## FASE 5: Threading, configuración y polish final

### 5.1 Algoritmo de threading JWZ (src/tui/threading.rs)

```rust
/// Implementación del algoritmo de Jamie Zawinski para agrupar mensajes en conversaciones.
/// Referencia: https://www.jwz.org/doc/threading.html
///
/// Pasos del algoritmo:
///
/// 1. CONSTRUIR TABLA DE ID → CONTAINER
///    Para cada mensaje, crear un Container con su Message-ID.
///    Si ya existe un Container con ese ID (referenciado por otro mensaje), asociarlo.
///
/// 2. ENLAZAR REFERENCES
///    Para cada mensaje con References: A B C D
///    - A es padre de B, B es padre de C, C es padre de D
///    - D es padre del mensaje actual
///    - Evitar ciclos: antes de establecer parentesco, verificar que no crea un loop
///
/// 3. ENCONTRAR RAÍCES
///    Los Containers sin padre son las raíces de los threads.
///
/// 4. PODAR CONTAINERS VACÍOS
///    Si un Container no tiene mensaje y solo tiene un hijo, promover el hijo.
///
/// 5. AGRUPAR POR SUBJECT (fallback)
///    Si dos raíces tienen el mismo Subject (normalizado: sin Re:/Fwd:), fusionarlas.
///
/// Resultado: Vec<Thread> donde cada Thread es un árbol de mensajes.

#[derive(Debug)]
pub struct Thread {
    pub root_message_id: String,
    pub subject: String,
    pub messages: Vec<ThreadNode>,
    pub total_count: usize,
    pub date_range: (DateTime<Utc>, DateTime<Utc>),
}

#[derive(Debug)]
pub struct ThreadNode {
    pub entry_index: usize,      // Índice en Vec<MailEntry>
    pub depth: usize,            // Profundidad en el thread (0 = raíz)
    pub children: Vec<ThreadNode>,
}

pub fn build_threads(entries: &[MailEntry]) -> Vec<Thread> { /* implementar */ }

/// Aplana un thread en una lista ordenada por fecha con indicación de profundidad.
/// Para mostrar en la TUI con indentación.
pub fn flatten_thread(thread: &Thread) -> Vec<(usize, usize)> {
    // Devuelve: Vec<(entry_index, depth)>
    /* implementar */
}
```

### 5.2 Fichero de configuración (src/config.rs)

```rust
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// Ubicaciones del config file, en orden de prioridad:
/// 1. $MBOX_TUI_CONFIG (variable de entorno)
/// 2. ~/.config/mbox-tui/config.toml (Linux/macOS)
///    %APPDATA%\mbox-tui\config.toml (Windows)
/// 3. Defaults

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub display: DisplayConfig,
    pub columns: ColumnsConfig,
    pub colors: ColorsConfig,
    pub export: ExportConfig,
    pub keybindings: KeybindingsConfig,
    pub performance: PerformanceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Columna de ordenación por defecto
    pub default_sort: String,        // "date", "from", "subject", "size"
    /// Dirección de ordenación por defecto
    pub sort_order: String,          // "asc", "desc"
    /// Formato de fecha para la lista
    pub date_format: String,         // strftime format
    /// Directorio para ficheros de caché (índices, logs)
    pub cache_dir: Option<PathBuf>,
    /// Nivel de log: "error", "warn", "info", "debug", "trace"
    pub log_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// Tema de colores: "dark", "light", "auto"
    pub theme: String,
    /// Layout inicial: "horizontal", "vertical", "list-only"
    pub layout: String,
    /// Mostrar sidebar al inicio
    pub show_sidebar: bool,
    /// Headers a mostrar en la vista del mensaje
    pub message_header_fields: Vec<String>,
    /// Máximo de mensajes en caché LRU
    pub max_cached_messages: usize,
    /// Ancho preferido para el texto del mensaje (0 = ancho del terminal)
    pub message_text_width: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnsConfig {
    pub visible: Vec<String>,
    pub date_width: u16,
    pub from_width: u16,
    pub subject_width: u16,    // 0 = flexible
    pub size_width: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorsConfig {
    pub unread: String,
    pub selected: String,
    pub marked: String,
    pub search_highlight: String,
    pub url: String,
    pub attachment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportConfig {
    pub default_format: String,
    pub default_output_dir: Option<PathBuf>,
    pub csv_separator: char,
    pub csv_include_snippet: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingsConfig {
    pub next_message: String,
    pub prev_message: String,
    pub search: String,
    pub quit: String,
    pub help: String,
    pub export: String,
    pub mark: String,
    pub toggle_threads: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformanceConfig {
    /// Tamaño del buffer de lectura en bytes
    pub read_buffer_size: usize,      // default: 131072 (128KB)
    /// Tamaño máximo de un mensaje individual en bytes
    pub max_message_size: usize,       // default: 268435456 (256MB)
    /// Número de mensajes en el LRU cache
    pub lru_cache_size: usize,         // default: 50
}

/// Implementar Default para todas las structs con valores sensatos
impl Default for Config { /* ... */ }
impl Default for GeneralConfig { /* ... */ }
// ... etc

/// Cargar configuración. Busca en las ubicaciones en orden.
pub fn load_config() -> Config { /* implementar */ }

/// Guardar configuración (para cuando el usuario cambie algo desde la TUI)
pub fn save_config(config: &Config) -> anyhow::Result<()> { /* implementar */ }
```

Añadir `toml = "0.8"` al Cargo.toml.

### 5.3 Logging a fichero

```rust
/// Configurar logging a fichero + stderr.
/// Fichero: ~/.cache/mbox-tui/mbox-tui.log
/// Rotación: máximo 10MB, mantener últimos 3 ficheros
pub fn setup_logging(level: &str) -> anyhow::Result<()> {
    // Usar tracing_subscriber con:
    // - fmt layer a stderr (para mensajes de error visibles)
    // - fmt layer a fichero (para debug)
    // - EnvFilter configurable
    /* implementar */
}
```

### 5.4 Generación de manpage y completions

```rust
/// En build.rs o como subcomando:
/// mbox-tui generate-completions --shell bash > mbox-tui.bash
/// mbox-tui generate-completions --shell zsh > _mbox-tui
/// mbox-tui generate-completions --shell fish > mbox-tui.fish
/// mbox-tui generate-completions --shell powershell > _mbox-tui.ps1
/// mbox-tui generate-manpage > mbox-tui.1
```

### 5.5 CI/CD (.github/workflows/ci.yml)

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always

jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
        rust: [stable, "1.75"]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ matrix.rust }}
          components: clippy, rustfmt
      - name: Check formatting
        run: cargo fmt --all -- --check
      - name: Clippy
        run: cargo clippy -- -D warnings
      - name: Build
        run: cargo build --all-features
      - name: Test
        run: cargo test --all-features
      - name: Build (no default features)
        run: cargo build --no-default-features

  release:
    needs: test
    if: startsWith(github.ref, 'refs/tags/v')
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact: mbox-tui-linux-x86_64
          - os: ubuntu-latest
            target: aarch64-unknown-linux-gnu
            artifact: mbox-tui-linux-aarch64
          - os: macos-latest
            target: x86_64-apple-darwin
            artifact: mbox-tui-macos-x86_64
          - os: macos-latest
            target: aarch64-apple-darwin
            artifact: mbox-tui-macos-aarch64
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            artifact: mbox-tui-windows-x86_64.exe
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - name: Build release
        run: cargo build --release --target ${{ matrix.target }} --all-features
      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: target/${{ matrix.target }}/release/mbox-tui*
```

### Criterios de aceptación Fase 5

- [ ] `t` en la TUI alterna entre vista plana y threaded
- [ ] Los threads se muestran correctamente con indentación
- [ ] Gmail labels se muestran como tags en la lista
- [ ] `label:inbox` filtra por label
- [ ] `~/.config/mbox-tui/config.toml` se carga al iniciar
- [ ] Cambiar tema entre dark/light funciona
- [ ] Los keybindings personalizados funcionan
- [ ] El log se escribe a fichero correctamente
- [ ] Shell completions se generan para bash, zsh, fish, powershell
- [ ] CI pasa en las 3 plataformas con Rust stable y MSRV 1.75
- [ ] `cargo install mbox-tui` funciona desde crates.io (cuando se publique)

---

## Resumen de dependencias finales

```toml
[dependencies]
# Core
mail-parser = "0.9"
encoding_rs = "0.8"
chrono = { version = "0.4", features = ["serde"] }
serde = { version = "1.0", features = ["derive"] }
bincode = "1.3"
sha2 = "0.10"
memmap2 = "0.9"
byteorder = "1"
lru = "0.12"

# CLI
clap = { version = "4", features = ["derive", "env", "wrap_help"] }
clap_complete = "4"
clap_mangen = "0.2"
indicatif = "0.17"
dialoguer = "0.11"

# TUI (optional)
ratatui = { version = "0.29", optional = true }
crossterm = { version = "0.28", optional = true }
html2text = { version = "0.12", optional = true }
textwrap = { version = "0.16", optional = true }
unicode-width = { version = "0.2", optional = true }
open = { version = "5", optional = true }

# Search
aho-corasick = "1"
regex = "1"
tantivy = { version = "0.22", optional = true }

# Export
csv = "1"
serde_json = "1"

# Config
toml = "0.8"
dirs = "5"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tracing-appender = "0.2"

# Errors
thiserror = "2"
anyhow = "1"

# Utilities
humansize = "2"

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
tempfile = "3"
assert_fs = "1"
predicates = "3"
assert_cmd = "2"

[features]
default = ["tui"]
tui = ["dep:ratatui", "dep:crossterm", "dep:html2text", "dep:textwrap", "dep:unicode-width", "dep:open"]
fulltext = ["dep:tantivy"]

[[bench]]
name = "parsing"
harness = false
```

---

## Guía de uso de este prompt con Claude Code

### Para empezar (Fase 1):

```
Estamos construyendo mbox-tui, un lector de MBOX de terminal en Rust.

Comienza con la FASE 1 del plan maestro. Necesito que:

1. Crees la estructura completa del proyecto (Cargo.toml, directorios, módulos)
2. Implementes el parser MBOX por streaming que maneje ficheros de 50GB+ sin cargar en memoria
3. Implementes el parsing de headers con soporte de encoded-words y múltiples charsets
4. Implementes el índice binario persistente con verificación de integridad
5. Implementes el store/reader para acceder a mensajes por offset
6. Crees los ficheros de test fixture
7. Implementes todos los tests de la sección 1.12
8. Implementes los comandos CLI: index y stats
9. Verifiques que cargo test, cargo clippy y cargo build pasan limpiamente

El foco principal es la eficiencia: el parser NUNCA debe cargar el fichero entero en memoria. 
Usa BufReader con buffer de 128KB y streaming puro.

Sigue las especificaciones exactas del prompt para tipos de error, modelo de datos, 
formato del índice y algoritmo del parser.
```

### Para cada fase siguiente:

```
Continuamos con mbox-tui. La Fase [N-1] está completa y todos los tests pasan.

Implementa la FASE [N] siguiendo las especificaciones del plan maestro.
Asegúrate de que todos los criterios de aceptación se cumplen antes de reportar que has terminado.
Ejecuta cargo test, cargo clippy -- -D warnings, y verifica manualmente las funcionalidades descritas.
```

### Si algo falla:

```
El test [nombre_del_test] falla con este error: [error]
Revisa la especificación de la Fase [N] sección [X.Y] del plan maestro 
y corrige la implementación para que cumpla con los requisitos.
```
