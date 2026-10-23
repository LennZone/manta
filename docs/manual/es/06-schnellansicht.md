# Vista Rápida y vista previa {#schnellansicht}

Pulsa la **barra espaciadora** para ver un archivo sin abrirlo. Para los archivos de tu Mac, Manta usa **Vista Rápida** de Apple, como Finder; para los archivos de servidores y de archivos comprimidos hay una **vista previa integrada**.

## Ver archivos rápidamente {#leertaste}

1. Selecciona un archivo.
2. Pulsa la **barra espaciadora** (o **⌘Y**, como en Finder).
3. Usa **↑** y **↓** para ir al archivo anterior o siguiente; el panel de previsualización lo sigue.
4. La **barra espaciadora** o **esc** cierran la vista previa.

Con varios archivos seleccionados, las flechas **‹** y **›** de Vista Rápida recorren la selección.

El teclado sigue funcionando en Manta: puedes seguir moviéndote por la lista con las teclas de flecha mientras el panel de Vista Rápida está abierto.

![Vista Rápida sobre un panel](images/quick-look.png)

## Vista Rápida (macOS) {#quick-look}

Vista Rápida es el panel de previsualización de Apple. Muestra todo lo que muestra Finder: PDF, imágenes, vídeos, documentos de Office, archivos de texto y mucho más. Funciona con archivos y carpetas **locales**.

## Vista previa integrada {#integrierte-vorschau}

Los archivos de **servidores** (SFTP, FTP, S3, contenedores) y **de archivos comprimidos** se abren en la ventana de vista previa propia de Manta, porque Vista Rápida de Apple solo conoce archivos locales.

- La vista previa integrada muestra **archivos de texto** e **imágenes**.
- En los archivos de texto grandes muestra el principio (“Se muestran los primeros … del archivo.”).
- Para otros tipos de archivo (PDF o vídeos, por ejemplo) indica qué es el archivo y propone abrirlo con ↩. Manta lo descarga entonces y lo abre con la app predeterminada.
- No muestra carpetas; pulsa ↩ para abrir una. (Vista Rápida de Apple sí muestra carpetas locales).
- Para cerrarla: **barra espaciadora** o **⌘W** (es una ventana propia con los botones de ventana habituales).

![Vista previa integrada de un archivo de texto de un servidor](images/preview-builtin.png)

## Cambiar lo que hace la barra espaciadora {#einstellung}

En **Ajustes** → **General** → **Vista previa** → **La barra espaciadora abre**, elige:

- **Vista Rápida (macOS)**: la opción por omisión.
- **Vista previa integrada**: también para archivos locales.

Los archivos de servidores y de archivos comprimidos siempre usan la vista previa integrada.

## Más vistas previas con plugins {#plugins}

- **Vista previa de Markdown** (**⌥⌘M**): muestra los archivos `.md` bien maquetados en una barra lateral; consulta [Vista previa de Markdown](14-plugins.md#markdown).
- **Herramientas de imagen**: muestran el tamaño, el perfil de color y los datos EXIF de una imagen; consulta [Herramientas de imagen](14-plugins.md#bildwerkzeuge).

## Limitaciones {#grenzen}

- Vista Rápida solo funciona con archivos locales. Manta (todavía) no descarga los archivos del servidor para mostrarlos en Vista Rápida.
- La vista previa integrada solo muestra texto e imágenes.
