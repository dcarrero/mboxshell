# Changelog

Todos los cambios relevantes de mboxshell se documentan en este fichero.

El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el proyecto se ajusta a [Semantic Versioning](https://semver.org/lang/es/).

## v0.4.7

- Corregido: **artefactos residuales en pantalla en la TUI causados por caracteres de control en el texto de los mensajes.** Los caracteres de control en bruto de un mensaje (los tabuladores eran el culpable típico, en cuerpos e incluso cabeceras) se escribían al terminal tal cual; el terminal movía el cursor por su cuenta, desincronizando el diffing de celdas de ratatui respecto a lo que había realmente en pantalla, así que fragmentos de mensajes vistos antes sobrevivían en zonas que la interfaz creía vacías. Todo el texto renderizado — cuerpo, vista raw, cabeceras y columnas de la lista — se sanea ahora antes de dibujar: los tabuladores se expanden a tab stops de 8 columnas y el resto de caracteres de control se muestran como un `�` visible. Los offsets de la búsqueda dentro del cuerpo se ajustaron en consecuencia, así que el resaltado de coincidencias sigue alineado. Gracias a @jpetrina por el reporte y por verificar el fix (#17).

## v0.4.6

- Corregido: **los correos citados verbatim dentro del cuerpo de un mensaje ya no parten el mensaje que los contiene.** Una línea que empieza por `From ` ahora solo separa mensajes cuando es estructuralmente un separador mbox real — `From <remitente> <fecha asctime>` terminando justo después de la fecha (con zona horaria opcional), o una línea `From ` desnuda sin nada detrás (como las escribe Thunderbird al exportar una cuenta de Gmail). Los correos de `git format-patch` citados — cuyo pseudo-separador lleva la fecha mágica fija de git `Mon Sep 17 00:00:00 2001` — se tratan como contenido dentro de un buzón normal, mientras que un fichero que *empieza* por uno (una serie de parches, que es mbox válido en sí misma) sigue separándose en cada parche. Se ha subido la versión del formato de índice, así que los índices existentes se reconstruyen automáticamente con los límites corregidos. Gracias a @jpetrina por el reporte, la depuración al momento y el fix de los separadores desnudos (#16).
- Cambiado: **la búsqueda por metadatos y la ordenación por columnas reservan mucha menos memoria.** La comparación sin distinción de mayúsculas ya no pasa a minúsculas cada campo de cada entrada por término de búsqueda (los campos ASCII se comparan in situ; los no-ASCII conservan el folding Unicode completo), y ordenar por Remitente/Asunto calcula la clave de cada fila una sola vez en lugar de dos veces por comparación — ambas cosas se notan en buzones con cientos de miles de mensajes.

## v0.4.5

- Seguridad: **un fichero de índice corrupto o manipulado ya no puede provocar una reserva de memoria gigante.** Al cargar un `.mboxshell.idx`, ahora se valida que el `offset + length` de cada entrada (con aritmética segura frente a desbordamientos) cae dentro del tamaño real del MBOX; un índice con entradas fuera de rango se trata como inválido y se reconstruye, igual que cualquier otro índice obsoleto. Las longitudes de mensaje también se convierten con una conversión comprobada en el store, en vez de un cast silencioso que podía truncar en sistemas de 32 bits.
- Seguridad: **las exportaciones a CSV ahora protegen contra la inyección de fórmulas de hoja de cálculo.** Los valores que empiezan por `=`, `+`, `-`, `@`, tabulador o CR reciben un `'` inicial para que Excel/LibreOffice los muestren como texto en vez de evaluarlos como fórmulas — un asunto tipo `=cmd|...` en un mensaje de spam era el vector típico al abrir el CSV exportado en una hoja de cálculo.
- Cambiado: **los binarios de release ahora son más pequeños y algo más rápidos** — compilados con LTO thin, una sola unidad de codegen y símbolos eliminados mediante una nueva sección `[profile.release]`.
- Cambiado: mantenimiento de CI/build — caché de cargo en CI con cancelación automática de runs supersedidos, fmt/clippy se ejecutan una sola vez en Linux en vez de en cada SO, token del workflow restringido a solo lectura, `cross` se instala como binario precompilado y pinneado en vez de compilarse desde el HEAD de git, builds con `--locked`, y eliminadas las dependencias sin uso `memmap2` y `byteorder`.

## v0.4.4

- Corregido: **las líneas de cabecera que caen justo en el límite del búfer de lectura de 1 MB ya no se parten**, algo que antes truncaba las cabeceras de un mensaje y descartaba todo lo que venía después del punto de corte (`Subject`, `Date`, …). Los mensajes afectados aparecían con fecha `1970-01-01` y asunto `unknown`, e inflaban el recuento de mensajes. El lector de líneas ahora acumula una línea física completa a través de las recargas del búfer en vez de tratar un fragmento parcial como una línea completa. Gracias a @jpetrina por el diagnóstico preciso y la propuesta de corrección (#15).

## v0.4.3

- Añadido: el comando `stats` ahora muestra una **línea `Duplicados`** que cuenta los mensajes que repiten un `Message-ID` ya visto, junto al número de IDs distintos — p. ej. `Duplicados  185 (42 IDs únicos)`. Los mensajes sin `Message-ID` no se cuentan como duplicados. Las mismas cifras `duplicates` / `unique_ids` se incluyen en `stats --json`. Gracias a @jpetrina (#14).
- Cambiado: el **cuerpo del mensaje y los valores de las cabeceras ahora se muestran en un casi-blanco más brillante** (`rgb(235,235,245)` en vez de `rgb(220,220,230)`), para mejor contraste sobre fondos negros — especialmente en terminales translúcidos donde el tono anterior podía verse apagado (#13).

## v0.4.2

- Corregido: la búsqueda en el cuerpo con `n` / `N` ahora **desplaza de forma fiable la coincidencia enfocada hasta la vista**. El auto-scroll medía la posición en líneas *sin envolver*, mientras que el cuerpo se desplaza sobre filas *envueltas*, así que en mensajes con líneas largas la coincidencia podía quedar fuera de pantalla y `n`/`N` parecían no hacer nada. Ahora el desplazamiento tiene en cuenta el ajuste de línea (usa el propio word-wrap de ratatui para mapear una coincidencia a su fila en pantalla), lo que además permite desplazar el cuerpo limpiamente hasta el final (#12).
- Cambiado: el prompt de búsqueda en el cuerpo ahora aparece en la **parte superior del panel del mensaje**, justo al lado del cuerpo que se busca, en vez de en la barra inferior global (#12).
- Añadido: **navegación vertical con teclado en el popup de Filtros de búsqueda.** `↑` / `↓` se mueven entre campos (junto a `Tab` / `Shift-Tab`) y `RePág` / `AvPág` (o `Inicio` / `Fin`) saltan al primer / último campo. Los selectores de Tamaño y Etiqueta ahora cambian su valor con `←` / `→` (manteniendo `j` / `k` como alias), ya que las flechas ahora se usan para moverse entre campos (#13).

## v0.4.1

- Añadido: **búsqueda interactiva dentro del cuerpo del mensaje abierto**, al estilo less/vim. Con el panel de mensaje enfocado, `/` abre un prompt que resalta todas las coincidencias en vivo según escribes; `Enter` confirma y las mantiene navegables; `n` / `N` saltan a la coincidencia siguiente/anterior con auto-scroll que la centra en pantalla; un contador `[ actual/total ]` aparece en el borde del cuerpo junto al indicador de scroll; `Esc` primero limpia las coincidencias y luego vuelve a la lista. La búsqueda no distingue mayúsculas y es compatible con Unicode. La búsqueda global con `/` no cambia en el resto de paneles (#12).

## v0.4.0

- Añadido: la vista previa del mensaje ahora muestra un **indicador de posición de scroll** en la esquina inferior derecha de su borde, para saber de un vistazo si el cuerpo se puede desplazar y por dónde vas — `[ Todo ]` cuando cabe entero, `[ ↓ Inicio ]` al principio, `[ ↕ NN% ]` en medio y `[ ↑ Fin ]` al final (#10).
- Corregido: **«buscar dentro de resultados previos» ahora funciona de forma fiable en todo el flujo.** El interruptor se reiniciaba en silencio cada vez que se reabría el popup de filtros, así que refinar un conjunto de resultados con un segundo campo (p. ej. una búsqueda de Texto/cuerpo tras una de Asunto) volvía a escanear el índice completo. Ahora es un modo de ámbito persistente respetado por todos los puntos de búsqueda, y solo se descarta cuando el ámbito se reinicia de verdad — al cambiar el filtro de etiqueta o salir de la búsqueda con `Esc` (#11).
- Añadido: `build_index_cancelable()` — variante cancelable de la construcción del índice que consulta un callback `should_cancel` por cada mensaje y aborta sin escribir un índice parcial, para integraciones que necesitan interrumpir un indexado largo (p. ej. la app de macOS). `build_index()` no cambia y es totalmente retrocompatible.

## v0.3.8

- Añadido: `Shift+↑` / `Shift+↓` (y `Shift+RePág` / `Shift+AvPág`) ahora hacen scroll del cuerpo del mensaje seleccionado en el panel de previsualización sin salir de la lista de mensajes, de modo que puedes leer un correo largo manteniendo la navegación de la lista con las flechas normales (#8).
- Cambiado: los atajos sueltos de la barra de estado ahora son consistentemente en minúscula — `F:Filtros` → `f` y `L:Etiquetas` → `l`. Las teclas `F`/`L` en mayúscula siguen funcionando como alias ocultos, así que la memoria muscular existente se mantiene (#9). Los atajos emparejados con Shift (`s`/`S`, `h`/`H`, `n`/`N`, `a`/`A`, `g`/`G`) no cambian.

## v0.3.7

- Corregido: las búsquedas lanzadas desde la barra de búsqueda ahora respetan el filtro de etiqueta activo en la barra lateral. Cuando había una etiqueta seleccionada, escribir una consulta y pulsar `Enter` perdía el scope y buscaba contra todos los mensajes del índice; la barra ahora deriva un conjunto de restricción a partir de la etiqueta activa e intersecta los resultados con él (#7). El camino de consulta vacía honra el mismo scope, así que limpiar la consulta ya no escapa de la etiqueta.

## v0.3.6

- Corregido: las búsquedas de texto libre y `body:`/`filename:` ya no congelan la interfaz. v0.3.5 hizo que el campo `Texto` escaneara el cuerpo de los mensajes, pero ese escaneo se ejecutaba de forma síncrona en el hilo de la UI, así que en un buzón grande la app entera se bloqueaba hasta terminar, sin progreso ni forma de cancelar (#6). El escaneo del cuerpo ahora corre en un **hilo en segundo plano**: la interfaz sigue respondiendo, muestra el progreso en vivo (`Buscando en los cuerpos N/M`) y se puede cancelar con **Esc**. Las búsquedas solo de metadatos (`from:`, `subject:`, …) siguen resolviéndose al instante en línea.
- Cambio: un valor de varias palabras en el campo `Texto` ahora coincide con los mensajes que contienen **todas** las palabras (AND), buscadas en asunto/remitente/destinatario **y** en el cuerpo, en vez de buscar esa frase contigua exacta. Los valores por campo (`subject:`, `from:`, …) se siguen tratando como frases entrecomilladas.

## v0.3.5

- Corregido: el campo `Texto` del popup de Filtros de Búsqueda (y cualquier búsqueda de texto libre / palabra suelta) ahora busca también en el **cuerpo del mensaje**, además de en asunto/remitente/destinatario. Antes solo miraba los metadatos de las cabeceras, así que una palabra que solo estaba en el cuerpo no devolvía resultados — lo que hacía que combinar `Texto` + `Asunto` "no siempre encontrara la coincidencia" (#4, #6) y que `Buscar en los resultados anteriores` pareciera roto, porque su búsqueda base no devolvía nada (#5). El filtrado mientras escribes sigue siendo solo de metadatos e instantáneo; el escaneo del cuerpo se ejecuta al pulsar `Enter`, con el mismo coste que una consulta `body:` explícita. Las consultas OR y los términos por campo no cambian.

## v0.3.4

- Nuevo: casilla `Buscar en los resultados anteriores` en el popup de Filtros de Búsqueda (`F`). Al activarla, la nueva consulta se intersecta con lo que estuviera visible al abrir el popup, permitiendo refinar progresivamente un conjunto de resultados (#5).

## v0.3.3

- Fix: el popup de Filtros de Búsqueda (`F`) ahora entrecomilla los valores con espacios al construir la query interna, de modo que combinar `Text` + `Subject` (o cualquier par de filtros cuando uno contiene espacios) ya no rompe la consulta partiéndola en términos AND independientes (#4).
- Fix: las frases entrecomilladas en la búsqueda por metadatos ahora usan coincidencia por substring en vez de igualdad estricta, igual que ya hacía la búsqueda fulltext y como cabe esperar de queries como `subject:"informe mensual"` (#4).
- Nuevo: pista `F: Filtros` en el pie de la lista de mensajes para que el popup visual de filtros sea descubrible sin abrir la ayuda (#3).

## v0.3.2

- Renderizado HTML: la vista interna del mensaje ahora usa el crate `html2text`, por lo que tablas, listas, encabezados y enlaces se ven correctamente (#1).
- Nuevo atajo `H`: abre el cuerpo HTML del mensaje actual en un visor externo (configurable con `MBOXSHELL_HTML_VIEWER`, por defecto `w3m`; funciona con `chawan`, `lynx -dump`, `pandoc`, etc.). La TUI suspende la pantalla alternativa mientras corre el visor y la restaura al salir (#1).
- Nuevo formato de exportación `html`: `mbox-tui export ... --format html` y una nueva opción HTML en el popup de exportación. Produce una página HTML autocontenida con los headers en una tabla y el cuerpo HTML original (o texto envuelto en `<pre>`). **Los cuerpos HTML se sanitizan por defecto** (scripts, manejadores `on*`, iframes y URLs `javascript:` se eliminan vía el crate `ammonia`); usa `--raw-html` para conservar el markup original (solo recomendado para archivado local) (#1).
- La barra de búsqueda ahora muestra una chuleta de sintaxis en línea (`from: to: subject: body: date:` …) cuando está vacía, para que el lenguaje de búsqueda sea descubrible sin leer la documentación (#1).
- Nuevo flag `--qp` en `export ... --format eml`: re-codifica los cuerpos de texto de 8 bits como quoted-printable, de modo que el EML resultante es ASCII puro de 7 bits. Ayuda a herramientas estrictas con UTF-8 como `eml-extractor` y `emlAnalyzer`. **Funciona tanto para mensajes single-part como multipart** — el árbol MIME se recorre recursivamente y cada hoja text/* se re-codifica en su sitio (#1).
- CI: bump de `actions/checkout`, `actions/upload-artifact` y `actions/download-artifact` a v5 (Node 24 nativo) antes del fin de Node 20 en GitHub (sep 2026).

## v0.3.1

- Fix: la barra de búsqueda registraba dos veces cada tecla y cada carácter pegado en Windows Terminal y en terminales con el protocolo kitty (#2). Ahora los eventos se filtran por `KeyEventKind::Press`.
- Fix: en el layout pantalla completa (`1`), pulsar `Tab`/`Enter` sobre un mensaje ahora muestra la vista del mensaje a pantalla completa y `Tab`/`Esc` vuelve a la lista (#1). Antes el foco cambiaba pero no se veía nada nuevo.
- Fix: la exportación a `.eml` ahora revierte el escapado mboxrd `>From ` y recorta el salto de línea separador del MBOX, produciendo ficheros conformes a RFC 5322 que aceptan los parsers estándar (#1).

## v0.3.0

- Popup de filtros de búsqueda (`F`): formulario visual para construir queries sin recordar la sintaxis (from, to, subject, rango de fechas, tamaño, adjuntos, etiqueta).
- Contador de resultados en la barra de búsqueda: muestra `(N / total)` mientras escribes.
- Historial de búsqueda: las teclas Arriba/Abajo en la barra de búsqueda navegan por consultas anteriores, con indicador `[history]`.
- Nuevas entradas en la ayuda para el atajo `F` e historial de búsqueda.
- Internacionalización completa EN/ES: todos los textos de la TUI y CLI (~150 claves de traducción), detección automática del idioma del sistema o selección manual con `--lang en|es`.

## v0.2.0

- Búsqueda incremental: la lista de mensajes se filtra mientras escribes (solo campos de metadatos; búsqueda full-text se ejecuta al pulsar Enter).
- Título dinámico en la vista de mensaje muestra el modo actual: `[RAW]` o `[HEADERS]`.
- Scroll proporcional con PageDown/Up en la vista de mensaje (se adapta a la altura real del viewport).
- Indentación mejorada en vista de hilos con conectores verticales (`│└`) y profundidad limitada a 4 niveles.
- Referencia completa de comandos CLI añadida a la documentación.

## v0.1.2

- Borde del panel activo resaltado en cyan para indicar claramente el foco.
- Barra de estado contextual: los atajos cambian según el panel enfocado.
- Número de versión visible en la esquina inferior derecha.
- Popup de ayuda reorganizado en columnas múltiples (se adapta al ancho del terminal).
- Popup de ayuda muestra nombre de la app, versión, licencia y autor.

## v0.1.0

- Versión inicial.
- Parser MBOX streaming (maneja archivos de 50 GB+ sin cargar en memoria).
- Índice binario persistente para re-aperturas instantáneas.
- Interfaz de terminal completa con navegación estilo vi y tres modos de layout.
- Soporte de etiquetas Gmail (X-Gmail-Labels) con filtrado en panel lateral.
- Búsqueda avanzada: `from:`, `to:`, `subject:`, `body:`, `date:`, `size:`, `has:attachment`, `label:`.
- Agrupación en conversaciones (algoritmo JWZ).
- Exportación a EML, TXT, CSV con extracción de adjuntos.
- Interfaz bilingüe (Inglés / Español).
