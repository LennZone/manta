# Favoritos y barra lateral {#seitenleiste}

La barra lateral de la izquierda te lleva con un clic a tus ubicaciones más importantes: favoritos, carpetas estándar, unidades, servidores conectados, contenedores y almacenamiento en la nube. Un clic abre la ubicación en el panel activo.

![Barra lateral con todas las secciones](images/sidebar.png)

## Las secciones {#abschnitte}

| Sección | Contenido |
|---|---|
| **Favoritos** | Carpetas y conexiones a servidores que has guardado. |
| **Ubicaciones** | Inicio, Aplicaciones, Escritorio, Documentos, Descargas y, al final, la **Papelera** (consulta más abajo). |
| **Nube** | iCloud Drive y las carpetas de las apps de nube (Google Drive para ordenadores, OneDrive, Dropbox, Box…); consulta [Almacenamiento en la nube](09-cloud.md). La sección solo aparece cuando Manta encuentra al menos un almacenamiento en la nube. |
| **Dispositivos** | Macintosh HD y todos los volúmenes montados (unidades USB, discos, recursos compartidos de red, imágenes de disco). |
| **Servidores** | Los servidores a los que estás conectado. Sin conexión, muestra **Conectarse al servidor…**. |
| **Contenedores** | Contenedores de Docker en ejecución y detenidos, y pods de Kubernetes (solo si Docker está instalado o hay un contexto de Kubernetes definido). |

El botón de flecha circular junto a **Nube** y **Dispositivos** vuelve a buscar carpetas en la nube y volúmenes. Manta también detecta por sí mismo las apps de nube nuevas.

Pasa el puntero por encima de una ubicación para ver su ruta completa en una descripción emergente.

## Añadir una carpeta a Favoritos {#favorit-hinzufuegen}

Hay tres maneras:

- **Teclado**: abre la carpeta en el panel activo y pulsa **⌃⌘T** (**Añadir carpeta a Favoritos**).
- **Ratón**: haz clic en el **+** junto al encabezado **Favoritos** (“Añadir carpeta actual”), o elige **Añadir carpeta a Favoritos** en el menú contextual de un espacio vacío del panel.
- **Arrastrar**: arrastra una o varias carpetas de un panel a la sección **Favoritos** (consulta más abajo).

Las carpetas **dentro de archivos comprimidos** no pueden ser favoritos (“Las carpetas dentro de archivos comprimidos no se pueden añadir a Favoritos”).

### Arrastrar carpetas a Favoritos {#favoriten-ziehen}

1. Selecciona una o varias carpetas en un panel, en cualquier visualización, locales o en un servidor.
2. Arrástralas a la sección **Favoritos**. La sección se resalta, una línea muestra dónde se colocarán las carpetas y el puntero indica, por ejemplo, “Añadir 2 carpetas a Favoritos”.
3. Suéltalas. Manta informa: *Se ha añadido “Proyectos” a Favoritos* o “Se han añadido 2 carpetas a Favoritos”.

Conviene saber:

- **Solo carpetas**: si arrastras solo archivos, el puntero indica “Solo las carpetas pueden ser favoritos” y no ocurre nada. Con una selección mixta, Manta toma solo las carpetas.
- **Sin duplicados**: si una carpeta ya es favorita, la entrada existente parpadea brevemente y Manta indica *“nombre” ya está en Favoritos*. Solo cuenta la ubicación, no el nombre del favorito.
- **Carpetas de servidor**: al arrastrar una carpeta desde un servidor se crea un favorito de servidor con esa carpeta como carpeta inicial, por ejemplo “files.example.com – www”. No se guarda ninguna contraseña nueva; si el Llavero ya tiene una para esa cuenta de servidor (de otro favorito), el nuevo favorito también la usa. Si no, Manta la pide al abrirlo (salvo que inicies sesión con ssh-agent o con un archivo de clave sin frase de contraseña).

![Una carpeta arrastrada a Favoritos](images/favorites-drag.png)

## Reordenar y eliminar favoritos {#favoriten-ordnen}

- **Reordenar**: arrastra un favorito hacia arriba o hacia abajo en la lista. La fila arrastrada se atenúa y una línea muestra la nueva posición. El orden se guarda al instante.
- **Eliminar**: haz clic derecho en el favorito → **Eliminar de Favoritos**, o arrastra el favorito a la **Papelera** de la barra lateral. Por supuesto, la carpeta en sí se conserva.

## Papelera {#papierkorb}

La **Papelera** es la última fila de **Ubicaciones**, como en Finder. Su icono indica si está vacía o llena (una papelera con contenido = hay algo dentro); la descripción emergente indica el número de ítems.

- **Abrir**: haz clic en **Papelera**. Se abre en el panel activo como cualquier carpeta. **Eliminar inmediatamente…** (o **⌘⌫**) elimina definitivamente ítems sueltos de su interior; Manta pregunta antes.
- **Vaciar**: haz clic derecho en **Papelera** → **Vaciar papelera…**, pulsa **⇧⌘⌫** o elige **Vaciar papelera…** en la paleta de comandos. Manta pregunta (“¿Seguro que quieres eliminar permanentemente los ítems de la papelera?”) y muestra el número de ítems y su tamaño. Esta acción no se puede deshacer. Los ítems que no se pueden eliminar (los bloqueados, por ejemplo) se omiten y se notifican.
- **Arrastrar a ella**: arrastra ítems de un panel a **Papelera**. La fila se resalta y el puntero indica, por ejemplo, “Trasladar 3 ítems a la papelera”. Los ítems locales van a la papelera sin preguntar (como con **⌘⌫**). Los ítems de un **servidor**, de **S3** o de un **contenedor** no pueden ir a una papelera: Manta pregunta si quieres eliminarlos definitivamente. Los ítems de un **archivo comprimido** no se pueden eliminar; el puntero lo indica. También puedes arrastrar archivos de otras apps (de Finder, por ejemplo) a la papelera.
- **Devolver**: solo en Finder (clic derecho → **Abrir en Finder** y después **Devolver**). Finder guarda la ubicación original en sus propios datos, que Manta no puede leer.

### Acceso total al disco {#papierkorb-zugriff}

macOS protege la papelera: para mostrar lo que contiene, Manta necesita **Acceso total al disco**. Sin él, el panel indica “Manta necesita acceso total al disco para mostrar la papelera” y ofrece dos botones:

1. **Abrir Acceso total al disco** abre **Ajustes del Sistema › Privacidad y seguridad › Acceso total al disco**. Activa ahí **Manta** y vuelve a abrir la papelera (**⌘R**). A veces hay que reiniciar Manta para que surta efecto.
2. **Abrir en Finder** muestra la papelera en Finder, aunque Manta sea tu gestor de archivos predeterminado.

Sin Acceso total al disco, arrastrar a la papelera y **⌘⌫** siguen funcionando, y la fila sigue indicando si la papelera está vacía o llena. **Vaciar papelera…** lo hace entonces Finder (macOS pregunta una vez si Manta puede controlar Finder); Finder también vacía la papelera de otros volúmenes.

## Servidores como favoritos {#server-favoriten}

Una conexión a un servidor se convierte en favorito cuando activas **Añadir a Favoritos** en el diálogo **Conectarse al servidor**. Puedes darle un nombre propio (“Nombre en la barra lateral (opcional)”) y una carpeta inicial.

Haz clic derecho en un favorito de servidor:

- **Abrir**: se conecta y abre la carpeta inicial.
- **Editar conexión…**: abre el diálogo de conexión con los datos guardados.
- **Eliminar de Favoritos**.

Pulsar **⌃⌘T** en una carpeta de un servidor crea un favorito de servidor con esa carpeta como carpeta inicial. La contraseña no se copia.

Consulta [Conexiones](08-verbindungen.md#favoriten).

## Unidades {#geraete}

Las unidades que conectas y las imágenes de disco que abres (por ejemplo, un DMG desde el que instalas una app) aparecen solas en **Dispositivos**. Si alguna no aparece, haz clic en el icono de flecha junto a **Dispositivos** (**Volver a buscar volúmenes**).

Como en Finder, las imágenes de disco, memorias USB y unidades externas aparecen en su propia sección, **Expulsables**, con el icono propio del volumen (o el de la app que contiene) si lo tiene. El icono de expulsión a su lado expulsa la unidad. El menú contextual ofrece además **Abrir**, **Abrir en una pestaña nueva**, **Mostrar en la carpeta contenedora** (en una imagen de disco, el archivo DMG) y **Expulsar “Nombre”**. Los paneles que muestran la unidad pasan a **Inicio** al expulsarla. Si una app todavía usa la unidad, Manta indica que no se ha podido expulsar.

## Servidores conectados {#server}

Cada conexión abierta aparece en **Servidores** con un punto verde. Un clic la abre en el panel activo. El icono de expulsión que aparece al pasar el puntero cierra la conexión (**Desconectar**).

## Contenedores {#container}

La sección **Contenedores** enumera tus contenedores de Docker, agrupados por proyecto de Compose como en Docker Desktop. Los contenedores en ejecución tienen un punto de color; los detenidos aparecen atenuados y plegados en “Detenidos (N)”. Para el contexto actual de Kubernetes hay un selector de namespace con los pods y sus contenedores.

**Iniciar, reiniciar, detener**: haz clic con el botón derecho en un contenedor de Docker. Uno detenido ofrece **Iniciar**; uno en ejecución, **Reiniciar** y **Detener**. Manta ejecuta `docker start`, `docker restart` o `docker stop`; mientras tanto la fila indica “Iniciando…” (etc.) y un mensaje confirma el resultado. Si un contenedor se detiene mientras está abierto en un panel, el panel ofrece **Iniciar contenedor** en lugar de **Reintentar** y vuelve a mostrar la carpeta cuando se está ejecutando.

La lista se actualiza en segundo plano cada 5 segundos. El icono de flecha (**Actualizar contenedores**) la vuelve a leer al instante. Consulta [Conexiones](08-verbindungen.md#container).

## Dónde se guardan los favoritos {#speicherort}

En `~/Library/Application Support/Manta/favorites.json`. Las contraseñas no están ahí: con **Recordar contraseña** se guardan en el Llavero de macOS; consulta [Conexiones](08-verbindungen.md#favoriten).
