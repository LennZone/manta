# Plugins integrados {#plugins}

Muchas funciones de Manta están en plugins integrados. Vienen activados desde el principio y puedes desactivar cada uno en **Ajustes** → **Plugins**. Git y Terminal tienen capítulos propios: [Git](12-git.md), [Terminal](11-terminal.md).

## Activar y desactivar plugins {#verwalten}

1. Abre **Ajustes** con **⌘,** y elige **Plugins**.
2. Los plugins integrados están en **Integrados**. Haz clic en **Desactivar** o **Activar**.
3. Si un plugin tiene opciones, ábrelas con **Ajustes** en su tarjeta.

Para añadir plugins de otras personas, abre **Descubrir** en la parte superior de la página Plugins, haz doble clic en un archivo `.mantaplugin`, suéltalo en la página Plugins o usa **Instalar plugin…**, desde un archivo `.mantaplugin` o una carpeta. Los plugins de otras personas se ejecutan en un entorno aislado (sandbox) de macOS y solo pueden hacer lo que permiten sus permisos; los plugins integrados forman parte de Manta y no están aislados. Más información: [Tus propios plugins](15-eigene-plugins.md#installieren) y [Seguridad](15-eigene-plugins.md#sicherheit).

Muchos plugins muestran su contenido en una **barra lateral a la derecha** o en un **panel bajo el panel de archivos**. Ábrelos con atajos, con las etiquetas de la barra de ruta o con la paleta de comandos (*Mostrar el panel “…”*).

![Ajustes → Plugins](images/settings-plugins.png)

## Tamaños de carpeta {#ordnergroessen}

**Tamaños de carpeta** muestra también el tamaño real de las carpetas en la columna **Tamaño**, medido en segundo plano y guardado en caché. Los tamaños se conservan tras reiniciar: Manta los muestra al instante y vuelve a medir en segundo plano por si algo ha cambiado.

- Mientras Manta sigue contando, muestra por ejemplo “≥ 1,2 GB” con un pequeño indicador de actividad.
- Pasa el puntero sobre un tamaño para ver el número de archivos y el tamaño en disco.
- Puedes ordenar por tamaño de carpeta, y este cuenta para el tamaño de la selección.

El tamaño es la suma de los tamaños de los archivos (igual que en los archivos). Manta no sigue los enlaces simbólicos, cuenta los enlaces físicos una sola vez y no sale del volumen de la carpeta.

**Solo cuando se solicite**: si cambias **Ajustes** → **Plugins** → **Tamaños de carpeta** a **Solo cuando se solicite**, las carpetas muestran “—” hasta que haces clic derecho en ellas y eliges **Calcular tamaño**.

### Uso del disco {#speicherbelegung}

**⌥⌘U** (**Mostrar uso del disco**) muestra qué ocupa espacio en la carpeta activa:

- un **mapa de árbol** (treemap): haz clic para ampliar y doble clic para abrir,
- una lista de los **Ítems más grandes**.

![Uso del disco con un mapa de árbol](images/disk-usage.png)

Los tamaños de carpeta y el uso del disco solo funcionan con carpetas locales.

## Renombrar en lote {#umbenennen}

Renombra muchos archivos a la vez, con reglas, vista previa en directo y opción de deshacer.

1. Selecciona los archivos. Si no hay nada seleccionado, se aplica a todos los archivos de la carpeta.
2. Pulsa **⇧⌘R** (**Renombrar en lote…**) o **F2** con varios ítems seleccionados.
3. Añade reglas con **Añadir regla**. Se aplican de arriba abajo.
4. Revisa la **Vista previa**: nombre original → nombre nuevo, con los cambios resaltados carácter a carácter.
5. Haz clic en **Renombrar** (o pulsa ↩).

![Renombrar en lote con reglas y vista previa](images/batch-rename.png)

Las reglas:

| Regla | Qué hace |
|---|---|
| **Buscar y reemplazar** | Texto o expresión regular, con grupos `$1`; distinguir mayúsculas; primera aparición o todas. |
| **Insertar texto** | Al principio, al final o en una posición. |
| **Eliminar caracteres** | Los primeros, los últimos o un rango de caracteres. |
| **Mayúsculas/minúsculas** | minúsculas, MAYÚSCULAS, Título, Frase. |
| **Numeración** | 1, 2, 3… con inicio, incremento y dígitos (ceros a la izquierda), como prefijo, sufijo o como nombre. |
| **Fecha** | Fecha de modificación o de hoy, en el formato que elijas. |
| **Extensión** | Cambiar la extensión, pasarla a minúsculas/mayúsculas o eliminarla. |
| **Limpiar** | Recortar espacios, unificar espacios, transliterar diéresis y acentos (ä → ae, é → e). |

Cada regla puede aplicarse a **Nombre**, **Extensión** o **Ambos**, activarse y desactivarse y cambiarse de posición.

Seguridad:

- La vista previa muestra al instante los conflictos (nombres duplicados, nombres existentes, nombres vacíos o no válidos). Solo se puede renombrar cuando están todos resueltos.
- Nunca se sobrescribe nada. Intercambiar nombres (a ↔ b) y los cambios que solo afectan a mayúsculas y minúsculas funcionan.
- Después, un aviso ofrece **Deshacer**. También está en la paleta de comandos: **Deshacer renombrado en lote**.

**Preajustes**: en **Preajustes** guardas un conjunto de reglas con un nombre y lo vuelves a cargar más tarde.

Renombrar en lote funciona en tu Mac y en servidores.

## Acciones rápidas {#schnellaktionen}

Las **Acciones rápidas** son tus propios comandos para los archivos seleccionados: ajustar el tamaño de imágenes, comprimir en ZIP, abrir en VS Code, sumas de verificación y cualquier otra cosa que pueda hacer un comando de shell.

### Ejecutar una acción rápida

- **Clic derecho** en archivos → submenú **Acciones rápidas**. Solo enumera las acciones adecuadas para la selección (“Reducir imágenes al 50 %” solo para imágenes, por ejemplo).
- **Paleta de comandos** (**⇧⌘P**): escribe el nombre de la acción.
- **Panel**: **Mostrar panel de acciones rápidas** en la paleta muestra las acciones aplicables y, en **Últimas ejecuciones**, el estado, la duración y la salida.

Se ejecutan como máximo dos acciones en segundo plano a la vez. Si una tarda más, aparece un aviso con **Cancelar**; al final recibes un aviso con el resultado. Las acciones de terminal se ejecutan a la vista en el [Terminal](11-terminal.md) bajo el panel.

### Acciones incluidas con Manta

Reducir imágenes al 50 % · Convertir a JPEG · Convertir a PNG · Comprimir en ZIP · Extraer archivo comprimido · Abrir en VS Code · Abrir en Terminal.app · Copiar ruta · Mostrar en Finder · Hacer ejecutable · Contar líneas · Calcular SHA-256 · Copiar al otro panel (rsync)

### Tus propias acciones en actions.json {#actions-json}

Las acciones están en `actions.json`, en la carpeta de datos del plugin (`~/Library/Application Support/Manta/plugin-data/manta.quickactions/`).

1. Elige **Editar acciones rápidas…** en la paleta de comandos. El archivo se abre en tu editor de texto.
2. Cambia o añade acciones y guarda el archivo.
3. Manta lo vuelve a cargar en menos de un segundo e informa de los problemas (“Acciones rápidas: …”). A mano: **Volver a cargar acciones rápidas**.

Un ejemplo:

```jsonc
{
  "actions": [
    {
      "id": "optimize",
      "title": "Optimize PNG",
      "title_de": "PNG optimieren",
      "applies_to": ["png"],
      "command": "optipng {file}",
      "message": "Optimized {count} image{s}",
      "message_de": "{count} Bild(er) optimiert",
      "refresh": true
    }
  ]
}
```

Los campos más importantes:

| Campo | Significado |
|---|---|
| `id` | Identificador único (letras, dígitos, `.`, `-`, `_`). |
| `title`, `title_de` | Nombre en el menú; `title_de` se usa cuando Manta está en alemán. |
| `command` | Comando de shell, ejecutado con `/bin/sh -c` en la carpeta de la selección. |
| `applies_to` | `"any"`, `"files"`, `"folders"` y/o extensiones como `"jpg"`. |
| `min`, `max` | Cuántos ítems deben estar seleccionados. |
| `mode` | `"background"` (por omisión), `"terminal"` (visible en el terminal) o `"silent"`. |
| `output` | `"none"`, `"toast"` (la salida como aviso) o `"panel"`. |
| `confirm` | Pregunta antes de ejecutar, por ejemplo `"Delete {count} file{s}?"`. |
| `refresh` | Volver a leer el panel después. |
| `shortcut` | Un atajo propio, por ejemplo `"⌃⌥Z"`. |

Marcadores de posición en el comando: `{file}` (el comando se ejecuta entonces una vez por ítem), `{files}` (todos a la vez), `{name}`, `{stem}`, `{ext}`, `{dir}`, `{dest}` (la carpeta del otro panel) y `{count}`. Manta se encarga de las comillas: escribe `{file}`, no `'{file}'`.

La referencia completa está en [docs/quick-actions.md](../../quick-actions.md) (en inglés).

> [!TIP]
> Para obtener las acciones por omisión actuales después de una actualización, renombra `actions.json`. Manta vuelve a crear el archivo.

Las acciones rápidas solo funcionan en carpetas locales.

## Info del proyecto {#projektinfo}

**Info del proyecto** detecta el proyecto en el que estás y permite ejecutar sus scripts con un clic.

Detecta: Laravel (`artisan`), Composer, Node (npm, pnpm, yarn, bun), Docker Compose, Rust (Cargo), Python, Make, just y Task.

- **Etiqueta en la barra de ruta**: por ejemplo “Laravel · 28 scripts”. Haz clic para mostrar u ocultar el panel del proyecto.
- **Panel del proyecto**: nombre, raíz, rama de Git y versiones (“PHP ^8.2”, por ejemplo), los botones **Abrir en VS Code**, **Finder**, **.env** e **Ir a la raíz**, tus scripts recientes y todos los scripts agrupados por herramienta.
- **Al hacer clic en un script**, se ejecuta en el [Terminal](11-terminal.md) bajo el panel, en la raíz del proyecto, como si lo hubieras escrito tú.
- **Paleta de comandos**: **Ejecutar script del proyecto…**, **Mostrar panel del proyecto**, **Volver a ejecutar el último script** (**⌥⌘R**).
- **Clic derecho** en `package.json`, `composer.json`, `Makefile` y similares: **Mostrar scripts**.

![Panel del proyecto de un proyecto Laravel](images/project-info.png)

> [!NOTE]
> Los scripts que pueden borrar datos (como `migrate:fresh`, `docker compose down -v`, `rm -rf` o los nombres que contienen “clean”, “reset”, “deploy” o “publish”) preguntan antes de ejecutarse. Están marcados con un signo de advertencia.

Info del proyecto necesita el plugin Terminal y solo funciona en carpetas locales. Detalles: [docs/project-info.md](../../project-info.md) (en inglés).

## Vista previa de Markdown {#markdown}

**Vista previa de Markdown** muestra el archivo `.md`, `.markdown` o `.mdx` seleccionado bien maquetado en una barra lateral a la derecha. Si no hay nada seleccionado, muestra el README de la carpeta.

- Mostrar y ocultar: **⌥⌘M** (**Mostrar/ocultar vista previa de Markdown**).
- Admite Markdown al estilo de GitHub: tablas, listas de tareas, alertas, notas al pie, código resaltado con botón **Copiar** e imágenes locales.
- **Contenido** muestra un índice; los enlaces a otros archivos `.md` los abren y **Atrás** vuelve.
- **Abrir en el editor** y **Mostrar en el panel**.
- En directo: cuando cambias el archivo, la vista previa se actualiza y mantiene la posición de desplazamiento.

Para proteger tu privacidad, la vista previa no carga imágenes de internet (su servidor sabría que has abierto el archivo). Para activarlas: **Ajustes** → **Plugins** → **Vista previa de Markdown** → **Cargar imágenes de internet (http/https)**.

Los archivos de más de 2 MB solo se muestran en parte y a petición (**Mostrar los primeros 2 MB**). No se admite HTML (excepto imágenes y saltos de línea), ni imágenes SVG, ni fórmulas matemáticas.

## Herramientas de imagen {#bildwerkzeuge}

**Herramientas de imagen** muestra detalles sobre las imágenes y edita muchas imágenes a la vez.

**Insignias en la lista:**

- **lo**: baja resolución; el lado más largo mide menos de 800 px.
- **XL**: grande para la web; más de 2 MB.
- La descripción emergente muestra, por ejemplo, “1920×1080 · sRGB · 340 KB”.

**Panel de imagen** (**Mostrar panel de imagen** en la paleta): vista previa, dimensiones, proporción, tamaño del archivo, perfil de color, PPP, canal alfa, animación y datos EXIF (cámara, objetivo, exposición, fecha de captura). Si una foto tiene ubicación GPS, el panel te avisa y ofrece **Abrir en Mapas**.

**Clic derecho → Herramientas de imagen:**

| Comando | Qué hace |
|---|---|
| **Ajustar tamaño…** | 50 %, 1920/1280/800/400 px o un tamaño personalizado; mantiene la proporción. |
| **Convertir a…** | JPEG, PNG, HEIC, TIFF; WebP con `cwebp`. |
| **Girar 90° a la izquierda/derecha**, **Girar 180°**, **Voltear horizontalmente/verticalmente** | |
| **Eliminar metadatos…** | Elimina EXIF (incluido el GPS), XMP, IPTC y comentarios, sin pérdida en JPEG, PNG y WebP. HEIC y TIFF requieren `exiftool`. |
| **Optimizar para la web…** | Lado más largo de 2560 px como máximo, sRGB, calidad JPEG 82 (PNG se mantiene si hay transparencia). |
| **Crear conjunto @2x/@1x…** | El original como @2x y una versión @1x a la mitad de tamaño. |
| **Copiar como URI de datos** | Para una sola imagen, para incrustarla en HTML/CSS. |

En **Guardar como** eliges **Junto al original**, **En una subcarpeta** o **Reemplazar originales** (los originales van a la papelera). Manta nunca sobrescribe archivos existentes sin preguntar; a los nombres ya ocupados se les añade `-2`, `-3`…

Herramientas de imagen usa `sips` de macOS. Para WebP, instala `cwebp` (`brew install webp`); para los metadatos de HEIC/TIFF, `exiftool` (`brew install exiftool`). Solo funciona en carpetas locales.

![Panel de imagen con datos EXIF](images/image-tools.png)

## Duplicados {#duplikate}

**Duplicados** busca archivos idénticos en una carpeta y sus subcarpetas, y traslada las copias sobrantes a la papelera.

1. Abre la carpeta y elige **Buscar duplicados en esta carpeta** en la paleta de comandos, o haz clic derecho en una carpeta → **Buscar duplicados aquí**.
2. Manta analiza la carpeta en segundo plano (puedes cancelar) y compara el tamaño, luego un hash rápido y después SHA-256.
3. El panel **Duplicados** muestra los grupos, el número de archivos duplicados y cuánto espacio recuperarías. Los grupos con mayor ahorro aparecen primero.
4. En cada grupo se conserva una copia (“Se conserva”). Cuál se conserva lo decide la **Regla de conservación**: **Más antiguo**, **Más reciente**, **Ruta más corta** o **En carpeta…**. Haz clic en otra copia para conservar esa en su lugar.
5. **Eliminar copias** (por grupo) o **Trasladar copias a la papelera** (todas), tras una confirmación con el número y el tamaño.

![Panel Duplicados con grupos](images/duplicates.png)

Conviene saber:

- Duplicados usa siempre la **papelera** de macOS, nunca una eliminación definitiva. Puedes devolverlo todo.
- Los archivos que han cambiado desde el análisis no se tocan.
- Se omiten `node_modules`, `vendor`, `.git`, los paquetes (como `.app`), los enlaces simbólicos y los enlaces físicos adicionales.
- **Mostrar en Finder** y **Seleccionar en el panel** te ayudan a comprobarlo.
- Opciones: en el panel, en **Ajustes** → **Plugins** → **Duplicados** (Omitir paquetes, Incluir archivos ocultos) y en `settings.json` (exclusiones, tamaño mínimo, un comando de papelera personalizado).

Duplicados solo funciona en carpetas locales.

## Carga del sistema {#systemauslastung}

**Carga del sistema** muestra a la derecha de la barra de estado cuánto trabaja tu Mac: “CPU 12 %  GPU 3 %  RAM 12,4 / 16 GB”, cada valor con una pequeña barra. Los valores se leen cada 2 segundos en segundo plano, sin permisos de administrador y sin programas auxiliares. Mientras la ventana está minimizada o tapada, la medición se detiene.

- Por encima del 85 % de carga o con presión de memoria elevada, el valor y la barra se vuelven ámbar; con presión de memoria crítica, rojos.
- La etiqueta de ayuda muestra detalles con los términos de Monitor de Actividad: los núcleos (de rendimiento y de eficiencia), el núcleo más cargado, los núcleos de GPU, la memoria de apps, la memoria fija, la comprimida, el intercambio usado y la presión de memoria.
- **RAM** es la “Memoria usada” de Monitor de Actividad (memoria de apps + fija + comprimida), sin cachés.
- Un clic abre **Monitor de Actividad**.
- En ventanas estrechas desaparece primero la indicación de la paleta de comandos y después la GPU; luego CPU y RAM muestran solo su icono y su barra.

Puedes desactivarla en **Ajustes** → **Plugins** → **Carga del sistema**; allí también puedes desmarcar **Mostrar la carga de la GPU**. En los Mac sin estadísticas de GPU, la GPU se omite sola.

## Plugins incluidos {#mitgeliefert}

**Ajustes** → **Plugins** → **Disponibles** enumera los plugins que vienen con Manta pero no están instalados (el ejemplo “Hello Python”, por ejemplo). **Instalar** los copia en tu carpeta de plugins; empiezan desactivados. Consulta [Tus propios plugins](15-eigene-plugins.md).
