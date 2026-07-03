# mboxShell — Manual de usuario

> Guía completa de todas las funciones de `mboxShell`, el visor rápido de MBOX para terminal.
> **Válido para mboxShell v0.6.0.**
> Versión en inglés: [MANUAL.md](MANUAL.md) · Resumen breve: [../README-ES.md](../README-ES.md) · Cambios: [../CHANGELOG-ES.md](../CHANGELOG-ES.md)

`mboxShell` abre, busca y exporta ficheros `.mbox` de cualquier tamaño (más de 50 GB) desde la terminal, sin cargar nunca el fichero entero en memoria y **sin modificar jamás el fichero original** (es estrictamente de solo lectura).

---

## Índice

1. [Conceptos básicos](#1-conceptos-básicos)
2. [Instalación](#2-instalación)
3. [Inicio rápido](#3-inicio-rápido)
4. [Referencia de línea de comandos](#4-referencia-de-línea-de-comandos)
5. [La interfaz de terminal (TUI)](#5-la-interfaz-de-terminal-tui)
6. [Atajos de teclado](#6-atajos-de-teclado)
7. [Búsqueda](#7-búsqueda)
8. [Exportación y extracción](#8-exportación-y-extracción)
9. [Fichero de configuración](#9-fichero-de-configuración)
10. [Variables de entorno](#10-variables-de-entorno)
11. [Idioma / internacionalización](#11-idioma--internacionalización)
12. [Autocompletado y página de manual](#12-autocompletado-y-página-de-manual)
13. [Rendimiento y límites](#13-rendimiento-y-límites)
14. [Resolución de problemas y FAQ](#14-resolución-de-problemas-y-faq)

---

## 1. Conceptos básicos

| Concepto | Qué significa |
|----------|---------------|
| **Solo lectura** | mboxShell nunca escribe en tu `.mbox`. Las exportaciones y fusiones siempre van a ficheros nuevos que tú indicas. |
| **E/S por streaming** | El fichero se lee por bloques (búfer de 128 KB por defecto). Un buzón de 100 GB usa aproximadamente la misma RAM que uno de 1 GB. |
| **Índice binario** | En la primera apertura se crea un fichero de índice `<nombre>.mboxshell.idx` junto al MBOX. Guarda metadatos compactos (remitente, asunto, fecha, offsets) para que las siguientes aperturas tarden menos de un segundo. |
| **Validación del índice** | El índice se vincula al origen mediante tamaño, fecha de modificación y un SHA-256 de los primeros bytes. Si el MBOX cambia, el índice se reconstruye automáticamente. |
| **Cuerpos bajo demanda** | Los cuerpos se decodifican solo al abrir el mensaje y se mantienen en una pequeña caché LRU (50 mensajes por defecto). |
| **Etiquetas de Gmail** | Las cabeceras `X-Gmail-Labels` (de Google Takeout) aparecen como carpetas virtuales en una barra lateral. |

### Formatos de entrada soportados

| Formato | Ruta | Notas |
|---------|------|-------|
| MBOX (mboxrd / mboxo) | `fichero.mbox` | Google Takeout, Thunderbird, servidores Unix |
| EML | `mensaje.eml` | Un único mensaje RFC 5322 |
| Carpeta de EML | `carpeta/` | Una carpeta con varios ficheros `.eml` |

---

## 2. Instalación

### Binarios precompilados (recomendado)

Descarga el binario de tu plataforma desde la página de [Releases](https://github.com/dcarrero/mboxshell/releases):

| Plataforma | Binario |
|------------|---------|
| Linux x86_64 | `mboxshell-linux-x86_64` |
| Linux ARM64 | `mboxshell-linux-aarch64` |
| Linux RISC-V 64 | `mboxshell-linux-riscv64` |
| FreeBSD x86_64 | `mboxshell-freebsd-x86_64` |
| macOS Intel | `mboxshell-macos-x86_64` |
| macOS Apple Silicon | `mboxshell-macos-aarch64` |
| Windows x86_64 | `mboxshell-windows-x86_64.exe` |
| Windows ARM64 | `mboxshell-windows-arm64.exe` |

```bash
# Linux / macOS
chmod +x mboxshell-*
sudo mv mboxshell-* /usr/local/bin/mboxshell      # para todo el sistema
# o bien:
mv mboxshell-* ~/.local/bin/mboxshell              # solo para tu usuario
```

En Windows, mueve el `.exe` a una carpeta de tu `PATH`, o ejecútalo directamente.

### Compilar desde el código fuente

Requiere [Rust](https://www.rust-lang.org/tools/install) 1.85 o posterior.

```bash
git clone https://github.com/dcarrero/mboxshell.git
cd mboxshell
cargo build --release
sudo cp target/release/mboxshell /usr/local/bin/
```

### Instalar con Cargo

```bash
cargo install --git https://github.com/dcarrero/mboxshell.git
```

> **Alternativa gráfica en macOS:** si prefieres una app nativa, consulta [mboxViewer](https://mboxviewer.net) — el mismo motor de parseo con interfaz de escritorio.

---

## 3. Inicio rápido

```bash
# Abrir un buzón en el visor interactivo (acción por defecto)
mboxshell correo.mbox

# Construir el índice y mostrar estadísticas
mboxshell index correo.mbox
mboxshell stats correo.mbox

# Buscar desde la línea de comandos
mboxshell search correo.mbox "from:user@gmail.com date:2024"

# Exportar a ficheros .eml individuales
mboxshell export correo.mbox --format eml --output ./emails/

# Extraer todos los adjuntos
mboxshell attachments correo.mbox --output ./adjuntos/

# Fusionar varios buzones en uno, descartando duplicados
mboxshell merge a.mbox b.mbox -o fusionado.mbox --dedup
```

---

## 4. Referencia de línea de comandos

Forma general:

```
mboxshell [FLAGS GLOBALES] [COMANDO] [ARGS]
mboxshell [FLAGS GLOBALES] <FICHERO>     # sin comando = abrir <FICHERO> en la TUI
```

### Flags globales

| Flag | Descripción |
|------|-------------|
| `-f`, `--force` | Forzar la reconstrucción completa del índice aunque exista uno válido |
| `-v`, `-vv`, `-vvv` | Aumentar el detalle del log (`info`, `debug`, `trace`) |
| `--lang <en\|es>` | Forzar el idioma de la interfaz (por defecto se autodetecta del locale) |
| `-h`, `--help` | Mostrar ayuda |
| `-V`, `--version` | Mostrar versión |

### Comandos

| Comando | Propósito |
|---------|-----------|
| `mboxshell <FICHERO>` | Abrir un fichero/carpeta en la TUI (por defecto, sin subcomando) |
| `open <ruta>` | Abrir un fichero o carpeta en la TUI |
| `index <ruta>` | Construir o reconstruir el índice binario (usa `--force` para reconstruir) |
| `stats <ruta> [--json]` | Mostrar estadísticas (nº de mensajes, rango de fechas, remitentes top, …) |
| `search <ruta> <consulta> [--json]` | Buscar y mostrar los mensajes coincidentes |
| `export <ruta> -o <salida> [opciones]` | Exportar mensajes (ver abajo) |
| `merge <entradas...> -o <salida> [--dedup]` | Fusionar varios ficheros MBOX en uno |
| `attachments <ruta> -o <salida>` | Extraer todos los adjuntos a una carpeta |
| `completions <shell>` | Imprimir el script de autocompletado (`bash`, `zsh`, `fish`, `powershell`, `elvish`) |
| `manpage` | Imprimir una página de manual por stdout |

#### Opciones de `export`

| Opción | Descripción |
|--------|-------------|
| `-f`, `--format <fmt>` | `eml` (por defecto), `csv`, `txt` (o `text`), `html` |
| `-o`, `--output <ruta>` | Carpeta de salida (formatos por mensaje) o fichero (csv) — **obligatorio** |
| `--query <q>` | Exportar solo los mensajes que coincidan con esta [consulta](#7-búsqueda) |
| `--qp` | Recodificar el texto de 8 bits como quoted-printable para que el `.eml` sea ASCII de 7 bits puro (ayuda a herramientas estrictas como `eml-extractor`). **Solo EML.** |
| `--raw-html` | Mantener el cuerpo HTML original **sin sanear** (se conservan scripts, `on*`, iframes). Solo para archivado local — nunca sirvas estos ficheros. **Solo HTML.** |

#### Salida de `stats`

`stats` informa de: ruta y tamaño del fichero, nº de mensajes, rango de fechas (más antiguo/más reciente), tamaño del índice, tiempo de indexado, nº y porcentaje de mensajes con adjuntos, el número de `Message-ID` duplicados (con el recuento de IDs únicos), y los 10 remitentes principales. Añade `--json` para una salida legible por máquina.

#### Ejemplos

```bash
mboxshell stats correo.mbox --json
mboxshell search correo.mbox "has:attachment subject:factura" --json
mboxshell export correo.mbox -f csv -o resumen.csv
mboxshell export correo.mbox -f eml -o ./salida/ --query "from:jefe after:2024-01-01" --qp
mboxshell export correo.mbox -f html -o ./html/ --raw-html
mboxshell completions zsh > ~/.zfunc/_mboxshell
```

---

## 5. La interfaz de terminal (TUI)

Al ejecutar `mboxshell <fichero>` (u `open`) arranca el visor interactivo.

### Paneles

- **Barra de cabecera** (arriba): nombre del fichero y contexto global.
- **Lista de mensajes**: tabla con scroll virtual (Fecha, De, Asunto, Tamaño). Solo se renderizan las filas visibles, así que la navegación es instantánea incluso con 500 000 mensajes.
- **Vista del mensaje**: el mensaje decodificado. En la esquina inferior derecha de su borde aparece un indicador de posición de scroll — `[ Todo ]` cuando cabe entero, `[ ↓ Inicio ]` al principio, `[ ↕ NN% ]` en medio, `[ ↑ Fin ]` al final.
- **Barra lateral de etiquetas** (opcional): etiquetas/carpetas de Gmail; selecciona una para filtrar la lista.
- **Barra de estado / barra de búsqueda** (abajo): pistas, progreso de búsqueda o la consulta activa.

### Modos de diseño

Cámbialos en cualquier momento con las teclas numéricas:

| Tecla | Diseño |
|-------|--------|
| `1` | Solo lista (lista a pantalla completa; `Enter` muestra el mensaje a pantalla completa) |
| `2` | División horizontal (lista arriba, mensaje abajo) — por defecto |
| `3` | División vertical (lista a la izquierda, mensaje a la derecha) |

### Modos de la vista del mensaje

- **Por defecto**: cabeceras compactas (Fecha, De, Para, Cc, Asunto) + cuerpo decodificado, con las URL resaltadas.
- `h` — alternar **cabeceras completas** (todas las líneas de cabecera en bruto).
- `r` — alternar **fuente en bruto** (los bytes originales del mensaje).
- `H` — abrir el **cuerpo HTML en un visor externo** (ver [`MBOXSHELL_HTML_VIEWER`](#10-variables-de-entorno)).

### Leer mensajes largos

Desplaza el cuerpo **sin salir de la lista** con `Shift-↑` / `Shift-↓` (y `Shift-RePág` / `Shift-AvPág`). Las flechas normales siguen navegando la lista. El indicador de posición en el borde del mensaje te dice de un vistazo si queda más por leer.

### Buscar dentro de un mensaje

Con un mensaje abierto y la **vista de mensaje enfocada** (pulsa `Enter` para cambiar a ella), pulsa `/` para buscar dentro del cuerpo, al estilo *less/vim*:

- Escribe el término — cada coincidencia se **resalta en vivo** según escribes, y la vista salta a la primera.
- `Enter` confirma la búsqueda: el prompt se cierra pero los resaltados se mantienen y las coincidencias siguen siendo navegables.
- `n` / `N` van a la coincidencia **siguiente / anterior**, con auto-scroll que la centra; la coincidencia activa se resalta de forma destacada.
- En el borde del mensaje aparece un contador `[ actual/total ]`, junto al indicador de scroll (p. ej. `[ 3/12 ]`).
- `Esc` limpia las coincidencias; un segundo `Esc` vuelve a la lista.

La búsqueda **no distingue mayúsculas** y es compatible con Unicode. Es independiente de la búsqueda global: `/` desde la lista o la barra lateral sigue buscando *entre mensajes*; `/` con la vista de mensaje enfocada busca *dentro del cuerpo abierto*.

### Barra lateral de etiquetas

Pulsa `l` para mostrar / enfocar / ocultar la barra lateral (solo se rellena si el buzón tiene `X-Gmail-Labels`). Seleccionar una etiqueta limita la lista a esa etiqueta; las búsquedas posteriores se mantienen dentro de ella.

### Hilos de conversación

Pulsa `t` para alternar la **vista por hilos**, que agrupa los mensajes en conversaciones con el algoritmo JWZ (el mismo que usaban Netscape/Mozilla). Pulsa `t` de nuevo para volver a la lista plana.

### Marcar mensajes

- `Espacio` — marcar / desmarcar el mensaje actual.
- `*` — marcar / desmarcar todos los mensajes visibles.

Las marcas te permiten actuar sobre una selección (p. ej. exportar).

### Adjuntos

Pulsa `a` para abrir el popup de adjuntos del mensaje actual:

- `j` / `k` — moverse entre adjuntos
- `Enter` — guardar el adjunto resaltado
- `A` — guardar todos los adjuntos
- `Esc` / `a` — cerrar

### Ordenación

- `s` — rotar la columna de orden: Fecha → De → Asunto → Tamaño.
- `S` — alternar ascendente / descendente.

---

## 6. Atajos de teclado

### Global / lista de mensajes

| Tecla | Acción |
|-------|--------|
| `j` / `k` (o `↓` / `↑`) | Mensaje siguiente / anterior |
| `g` / `G` (o `Inicio` / `Fin`) | Primer / último mensaje |
| `AvPág` / `RePág` | Página abajo / arriba |
| `Enter` | Abrir mensaje / cambiar a la vista de mensaje |
| `Shift-↑` / `Shift-↓` | Scroll del cuerpo del mensaje seleccionado (mantiene el foco en la lista) |
| `Shift-RePág` / `Shift-AvPág` | Scroll por páginas del cuerpo |
| `Tab` / `Shift-Tab` | Rotar el foco entre paneles |
| `/` | Abrir la barra de búsqueda |
| `f` | Abrir el popup de filtros de búsqueda (`F` es un alias oculto) |
| `n` / `N` | Resultado de búsqueda siguiente / anterior |
| `Espacio` | Marcar / desmarcar mensaje |
| `*` | Marcar / desmarcar todos |
| `s` / `S` | Rotar columna de orden / alternar dirección |
| `e` | Exportar el mensaje actual (EML, TXT, CSV, adjuntos) |
| `a` | Mostrar adjuntos |
| `t` | Alternar vista por hilos (conversación) |
| `l` | Mostrar / enfocar / ocultar la barra lateral de etiquetas (alias `L`) |
| `h` | Alternar cabeceras completas |
| `H` | Abrir el cuerpo HTML en un visor externo |
| `r` | Alternar fuente en bruto del mensaje |
| `1` / `2` / `3` | Diseño: solo lista / división horizontal / división vertical |
| `?` | Ayuda |
| `Esc` | Volver a la lista / cerrar popup |
| `q` o `Ctrl-C` | Salir |

### Vista de mensaje — búsqueda en el cuerpo (pulsa `/` con la vista de mensaje enfocada)

El prompt de búsqueda se abre en la **parte superior del panel del mensaje**, junto al cuerpo.

| Tecla | Acción |
|-------|--------|
| `/` | Abrir el prompt de búsqueda en el cuerpo |
| *(escribir)* | Refinar la consulta; las coincidencias se resaltan en vivo y la vista salta a la primera |
| `Enter` | Confirmar — cierra el prompt, mantiene resaltados y navegación con `n`/`N` |
| `n` / `N` | Ir a la coincidencia siguiente / anterior (con auto-scroll) |
| `Esc` | Limpiar las coincidencias; púlsalo de nuevo para volver a la lista |

### Barra de búsqueda (tras pulsar `/`)

| Tecla | Acción |
|-------|--------|
| *(escribir)* | Editar la consulta; la lista se filtra en vivo para consultas de metadatos |
| `Enter` | Ejecutar la búsqueda (las de cuerpo/texto completo corren en un hilo en segundo plano) |
| `↑` / `↓` | Navegar por el historial de búsquedas |
| `Esc` | Cancelar y restaurar la vista anterior |

### Popup de filtros de búsqueda (tras pulsar `f`)

| Tecla | Acción |
|-------|--------|
| `Tab` / `↑` / `↓` | Moverse entre campos (`Shift-Tab` retrocede) |
| `RePág` / `AvPág` (o `Inicio` / `Fin`) | Saltar al primer / último campo |
| *(escribir)* | Rellenar el campo enfocado (Texto, De, Para, Asunto, fechas…) |
| `Espacio` | Alternar la casilla enfocada (`has:attachment`, *Buscar dentro de resultados previos*) |
| `←` / `→` (o `j` / `k`) | Cambiar el valor del selector de Tamaño / Etiqueta |
| `Enter` | Construir la consulta y ejecutarla |
| `Esc` | Cerrar el popup |

### Popup de adjuntos (tras pulsar `a`)

| Tecla | Acción |
|-------|--------|
| `j` / `k` | Moverse entre adjuntos |
| `Enter` | Guardar el adjunto resaltado |
| `A` | Guardar todos |
| `Esc` / `a` | Cerrar |

---

## 7. Búsqueda

mboxShell tiene un único lenguaje de consulta usado tanto por el comando `search` de la CLI como por la barra de búsqueda / popup de filtros de la app.

### Dos motores

- **Búsqueda de metadatos** — coincide con asunto, de, para, cc, etiquetas, fechas, tamaño y adjuntos. Se ejecuta contra el índice en memoria, así que es instantánea (menos de ~200 ms incluso con un millón de mensajes) y filtra la lista *mientras escribes*.
- **Búsqueda de texto completo** — se activa con `body:` o un término libre suelto. Hace streaming de los cuerpos desde el disco, así que corre **en un hilo en segundo plano**: la interfaz sigue respondiendo, muestra el progreso en vivo (`Searching message bodies N/M`) y se puede cancelar con `Esc`.

### Sintaxis de consulta

| Operador | Significado | Ejemplo |
|----------|-------------|---------|
| *(palabra suelta)* | Busca en asunto + de + para; una palabra suelta también lanza un escaneo del cuerpo al pulsar `Enter` | `factura` |
| `from:` | Remitente | `from:user@gmail.com` |
| `to:` | Destinatario | `to:equipo@empresa.com` |
| `cc:` | Copia | `cc:jefe@empresa.com` |
| `subject:` | Línea de asunto | `subject:presupuesto` |
| `body:` | Búsqueda de texto completo en el cuerpo | `body:contrato firmado` |
| `label:` | Etiqueta de Gmail | `label:Recibidos` |
| `filename:` | Nombre de fichero adjunto | `filename:informe.pdf` |
| `id:` | Message-ID | `id:<abc@dominio>` |
| `has:attachment` | Solo mensajes con adjuntos | `has:attachment` |
| `has:no-attachment` | Solo mensajes sin adjuntos | `has:no-attachment` |
| `date:` | Día / mes / año exacto, o un rango | `date:2024-01-15`, `date:2024-01`, `date:2024`, `date:2024-01-01..2024-06-30` |
| `before:` / `after:` | Límites de fecha abiertos | `before:2024-06-01`, `after:2024-01-01` |
| `size:` | Comparación de tamaño | `size:>1mb`, `size:<100kb` |
| `"…"` | Frase exacta entrecomillada | `subject:"informe mensual"` |
| *(espacio)* | **AND** implícito — todos los términos deben coincidir | `from:juan subject:presupuesto` |
| `OR` | **OR** explícito — coincide cualquier término | `from:ana OR from:luis` |
| `-` | **NOT** — excluir | `-subject:spam` |

### Texto libre de varias palabras

Un valor de varias palabras en el campo **Texto** del popup (o una consulta libre de varias palabras) coincide con los mensajes que contienen **todas** las palabras (AND), buscando en asunto/de/para **y** en el cuerpo — no la frase contigua exacta. Usa comillas (`"…"`) cuando necesites la frase literal.

### Buscar dentro de resultados previos

En el popup de filtros, la casilla **Buscar dentro de resultados previos** limita la siguiente consulta a los mensajes visibles en ese momento. Es un **modo de ámbito persistente**: una vez activado, se mantiene entre reaperturas del popup, así que puedes refinar de forma iterativa (p. ej. acotar por Asunto, luego por una palabra del cuerpo, luego por remitente). El modo se descarta automáticamente solo cuando el ámbito se reinicia — al cambiar el filtro de etiqueta de la barra lateral, o al salir de la búsqueda con `Esc`.

### Búsqueda por CLI

```bash
mboxshell search correo.mbox "from:user@gmail.com date:2024"
mboxshell search correo.mbox "has:attachment subject:factura" --json
```

`--json` imprime resultados estructurados para scripts.

---

## 8. Exportación y extracción

### Formatos de exportación

| Formato | `--format` | Salida | Notas |
|---------|-----------|--------|-------|
| EML | `eml` (por defecto) | un `.eml` por mensaje en la carpeta de salida | Añade `--qp` para cuerpos ASCII de 7 bits puro |
| CSV | `csv` | un único fichero `.csv` resumen | UTF-8 con BOM (compatible con Excel); separador configurable |
| Texto plano | `txt` / `text` | un `.txt` por mensaje | Cuerpo de texto decodificado |
| HTML | `html` | un `.html` independiente por mensaje | Cuerpo saneado por defecto; `--raw-html` lo deja intacto (solo archivado local) |

Combínalo con `--query` para exportar solo los mensajes coincidentes:

```bash
mboxshell export correo.mbox -f eml -o ./salida/ --query "label:Importante after:2023-01-01"
```

En la TUI, pulsa `e` sobre un mensaje para abrir el popup de exportación y elegir un formato de forma interactiva.

### Fusionar buzones

```bash
mboxshell merge recibidos.mbox archivo.mbox -o todo.mbox --dedup
```

`merge` concatena varios ficheros MBOX en uno. `--dedup` (activado por defecto) elimina mensajes duplicados (por Message-ID / contenido), así que fusionar exportaciones de Takeout solapadas es seguro.

### Extraer adjuntos

```bash
mboxshell attachments correo.mbox -o ./adjuntos/
```

Decodifica y escribe todos los adjuntos de todo el buzón en la carpeta de salida. Para un único mensaje, usa el popup `a` de la TUI.

---

## 9. Fichero de configuración

La configuración es opcional — mboxShell funciona sin ella. Cuando existe, el fichero se lee de:

1. `$MBOXSHELL_CONFIG` (si está definida), si no
2. `~/.config/mboxshell/config.toml` (Linux/macOS) · `%APPDATA%\mboxshell\config.toml` (Windows)

Un fichero inválido o ausente recurre a los valores por defecto en silencio. Fichero completo con los **valores por defecto reales**:

```toml
[general]
default_sort = "date"          # date | from | subject | size
sort_order   = "desc"          # desc | asc
date_format  = "%Y-%m-%d %H:%M"
# cache_dir  = "/ruta/propia"  # por defecto: dir. de caché del SO + /mboxshell
log_level    = "warn"          # error | warn | info | debug | trace

[display]
theme               = "dark"        # dark | light
layout              = "horizontal"  # horizontal | vertical | list-only
show_sidebar        = false         # mostrar la barra de etiquetas al arrancar
max_cached_messages = 50
message_text_width  = 0             # 0 = usar todo el ancho del panel

[columns]
date_width = 17
from_width = 20
size_width = 8

[export]
default_format = "eml"          # eml | csv | txt | html
# default_output_dir = "./salida"
csv_separator  = ","

[performance]
read_buffer_size = 131072       # búfer de streaming de 128 KB
max_message_size = 268435456    # tope de 256 MB por mensaje
lru_cache_size   = 50           # mensajes decodificados en memoria
```

Rutas relacionadas:

- **Índice**: `<buzón>.mboxshell.idx`, junto al fichero de origen.
- **Carpeta de caché**: `cache_dir`, o el dir. de caché del SO + `/mboxshell`.
- **Fichero de log**: `<carpeta de caché>/mboxshell.log`.

---

## 10. Variables de entorno

| Variable | Efecto |
|----------|--------|
| `MBOXSHELL_CONFIG` | Ruta absoluta a un fichero de configuración, anulando la ubicación estándar |
| `MBOXSHELL_HTML_VIEWER` | Comando externo usado por `H` para renderizar cuerpos HTML. Por defecto `w3m`. Funciona con `chawan`, `lynx -dump`, `pandoc`, etc. La TUI se suspende mientras el visor corre y se restaura limpiamente al salir. |
| `MBOXSHELL_LANG` | Forzar el idioma de la interfaz (`en` / `es`). Tiene prioridad sobre `LC_MESSAGES` y `LANG`. |

```bash
MBOXSHELL_HTML_VIEWER="lynx -dump" mboxshell correo.mbox
MBOXSHELL_LANG=es mboxshell correo.mbox
```

---

## 11. Idioma / internacionalización

La interfaz y la salida de la CLI están disponibles en **inglés** y **español**. El idioma se resuelve en este orden:

1. flag `--lang en|es`
2. `MBOXSHELL_LANG`
3. `LC_MESSAGES`
4. `LANG`
5. inglés por defecto

```bash
mboxshell --lang es correo.mbox
```

---

## 12. Autocompletado y página de manual

```bash
# Bash
mboxshell completions bash | sudo tee /etc/bash_completion.d/mboxshell

# Zsh (una carpeta de tu $fpath)
mboxshell completions zsh > ~/.zfunc/_mboxshell

# Fish
mboxshell completions fish > ~/.config/fish/completions/mboxshell.fish

# PowerShell / Elvish también están soportados
mboxshell completions powershell > mboxshell.ps1

# Página de manual
mboxshell manpage > mboxshell.1
```

---

## 13. Rendimiento y límites

Medido con exportaciones reales de Google Takeout:

| Tamaño | Mensajes | Indexado inicial | Reapertura |
|--------|----------|------------------|------------|
| 500 MB | ~5 000 | ~3 s | < 1 s |
| 5 GB | ~50 000 | ~30 s | < 1 s |
| 50 GB | ~500 000 | ~5 min | < 1 s |

- La RAM se mantiene prácticamente plana sin importar el tamaño del fichero — solo el índice de metadatos vive en memoria.
- Un mensaje individual mayor que `max_message_size` (256 MB por defecto) se omite para proteger la memoria.
- La navegación de la lista es O(1) gracias al scroll virtual.

---

## 14. Resolución de problemas y FAQ

**¿mboxShell modifica mi buzón?**
No. Es estrictamente de solo lectura. Cada exportación/fusión escribe en una ruta nueva que tú eliges.

**La primera apertura es lenta.**
Es la pasada de indexado de una sola vez. Las siguientes aperturas leen el `.mboxshell.idx` y son casi instantáneas. Fuerza una reconstrucción con `mboxshell index <fichero> --force` si el índice alguna vez parece desactualizado (normalmente se reconstruye solo cuando el origen cambia).

**Una búsqueda de cuerpo parece colgarse.**
El texto completo (`body:` / palabras sueltas) escanea el fichero en un hilo en segundo plano. Observa el progreso `Searching message bodies N/M` en la barra de estado y pulsa `Esc` para cancelar.

**`H` no hace nada / da error.**
Necesita un visor HTML externo en modo texto. Instala `w3m` (por defecto) o define `MBOXSHELL_HTML_VIEWER` con uno que tengas (`chawan`, `lynx -dump`, `pandoc`, …).

**Los acentos se ven mal.**
mboxShell decodifica las encoded-words RFC 2047 y la mayoría de juegos de caracteres vía `encoding_rs`. Si algo aún se ve raro, mira la fuente en bruto con `r` para confirmar la codificación original.

**¿Dónde están los logs?**
En `<carpeta de caché>/mboxshell.log`. Aumenta el detalle con `-v` / `-vv` / `-vvv` y con `log_level` en la configuración.

---

*Ver también: [README-ES.md](../README-ES.md) · [CHANGELOG-ES.md](../CHANGELOG-ES.md) · [MANUAL.md](MANUAL.md)*

---

## Licencia

MIT - Copyright (c) 2026 David Carrero Fernandez-Baillo - [https://carrero.es](https://carrero.es)

Source Code: [https://github.com/dcarrero/mboxshell](https://github.com/dcarrero/mboxshell)
