# Navegación {#navigation}

Este capítulo explica cómo moverte por las carpetas: con la barra de ruta, con **Ir a la ruta…**, con el historial y con el teclado. Al final verás cómo filtrar, seleccionar escribiendo y mostrar archivos ocultos.

## Abrir carpetas y archivos {#oeffnen}

- **Doble clic** o **↩** abre una carpeta en el panel, o un archivo con su app predeterminada.
- **⌘↓** hace lo mismo que ↩.
- **⌥↩** (**Abrir con la app predeterminada**) abre siempre con la app predeterminada, también los archivos comprimidos, que Manta abre si no como carpetas (consulta [Archivos comprimidos](10-archive.md)).
- **⌘↑** o **⌫** (tecla de borrar) va a la **carpeta contenedora**.

Para abrir un archivo de un servidor, Manta lo descarga en una carpeta temporal y abre esa copia (*Descargando “nombre” para abrirlo…*).

> [!WARNING]
> Los cambios en un archivo del servidor abierto de esta forma solo se guardan en la copia temporal, no en el servidor. Para editar un archivo, cópialo a una carpeta local, edítalo ahí y vuelve a copiarlo.

## La barra de ruta {#pfadleiste}

La barra de ruta, en la parte superior de cada panel, muestra dónde estás, por ejemplo `Inicio › Proyectos › manta`.

![Barra de ruta con la etiqueta de ubicación, la ruta y las etiquetas de plugins](images/path-bar.png)

- **Haz clic en el nombre de una carpeta** para ir a ella.
- **Haz clic en “…”** al principio para ver las carpetas contenedoras (“Mostrar carpetas contenedoras”) cuando la ruta aparece acortada.
- **Haz doble clic en un espacio vacío** de la barra de ruta (o pulsa **⌘L**, **Editar ruta**) para convertirla en un campo de texto. Escribe una ruta y pulsa ↩. **esc** cancela.
- **Etiqueta de ubicación**: la etiqueta del extremo izquierdo muestra dónde estás: **Local** en tu Mac o, por ejemplo, **Docker · shop-web**, **Kubernetes · pod**, **SFTP · deploy@web-01**, **S3 · bucket**, **FTP · host**, **Archivo comprimido · backup.zip**; y en las carpetas en la nube, el proveedor (**Google Drive**, **iCloud Drive**, **OneDrive**, **Dropbox**; la cuenta aparece en la descripción emergente). Los nombres largos se acortan con “…” y los paneles estrechos solo muestran el icono; el texto completo está en la descripción emergente. Al hacer clic, vas a la carpeta superior de esa ubicación.
- **Visualización**: a la derecha, antes del campo de filtro, cambias entre Iconos, Lista, Columnas y Galería; al lado está **Opciones de visualización**. Consulta [Visualizaciones](03-ansichten.md#wechseln).
- **Las etiquetas de plugins** de la derecha muestran, por ejemplo, la rama de Git o el proyecto detectado. Cuando el panel se estrecha, se acortan y al final quedan en un icono; el texto completo está en la descripción emergente.

### Volver a Local {#lokal}

Cuando no estás en una carpeta normal de tu Mac (un servidor, contenedor, bucket, archivo comprimido o carpeta en la nube), junto a la etiqueta de ubicación aparece una flecha **Volver a Local**. Te lleva a la última carpeta de tu Mac en el historial de la pestaña (saltándose las carpetas en la nube y los archivos comprimidos) o, si no hay ninguna, a tu carpeta de inicio. La conexión sigue abierta.

## Ir a la ruta {#gehe-zum-pfad}

**Ir a la ruta…** (**⇧⌘G**) abre una carpeta cuya ruta conoces o tienes en el portapapeles.

1. Pulsa **⇧⌘G**.
2. Escribe o pega una ruta. Manta entiende `~/Downloads`, rutas relativas, rutas entre comillas y URL `file://`.
3. **⇥** completa los nombres de carpeta. Debajo del campo ves **Carpetas coincidentes** y **Recientes**.
4. Pulsa **↩** para abrir la carpeta en una **pestaña nueva**, o **⌘↩** para abrirla **en la pestaña actual**.

Manta comprueba la ruta en segundo plano e informa, por ejemplo, de “No se ha encontrado la carpeta” o “Permiso denegado”. Si la ruta es un **archivo**, Manta abre su carpeta y selecciona el archivo.

Si el panel activo muestra un servidor, la ruta se refiere a ese servidor (“Se abre en una pestaña nueva en…”).

**⇧⌘G** funciona en todas partes, incluso mientras escribes en un campo de texto.

![Ir a la ruta con sugerencias](images/go-to-path.png)

## Atrás y adelante {#verlauf}

Cada pestaña tiene su propio historial.

- **⌘[** o **⌘←**: **Atrás**
- **⌘]** o **⌘→**: **Adelante**

El historial entra en los archivos comprimidos y vuelve a salir de ellos.

## Seleccionar con el teclado {#auswaehlen}

| Tecla | Efecto |
|---|---|
| ↑ / ↓ | Ítem anterior / siguiente |
| ⇧↑ / ⇧↓ | Ampliar la selección |
| Re Pág / Av Pág | 20 filas más allá |
| Inicio / Fin | Primer / último ítem |
| ⌘A | Seleccionar todo |
| esc | Anular la selección |

Con el ratón: un clic selecciona un ítem, **⌘ + clic** añade o quita uno y **⇧ + clic** selecciona un intervalo. Un clic en un espacio vacío anula la selección.

## Seleccionar escribiendo {#tippen}

Como en Finder: escribe las primeras letras de un nombre y Manta salta al primer ítem que coincida. No distingue entre mayúsculas y minúsculas. Si ningún nombre empieza por esas letras, Manta salta al siguiente nombre según el orden.

Tras aproximadamente un segundo sin escribir, la búsqueda vuelve a empezar. Mientras escribes, la barra espaciadora forma parte del nombre (“mi ca”) y no abre Vista Rápida.

## Filtrar archivos {#filtern}

El filtro muestra solo los ítems cuyo nombre contiene un texto.

1. Pulsa **⌘F** (**Filtrar archivos**) o haz clic en el campo **Filtrar** de la barra de ruta.
2. Escribe parte del nombre, por ejemplo `.pdf` o `factura`.

El filtro se aplica solo a la carpeta actual y a esta pestaña. La barra de estado muestra *que coinciden con “…”*. Si nada coincide, ves *Ningún ítem coincide con “…”* con un botón **Borrar filtro**. **esc** en el campo borra el filtro. En paneles más estrechos (por ejemplo, dos paneles en una ventana pequeña), el campo es una lupa: haz clic en ella o pulsa **⌘F** para abrirlo; permanece abierto mientras contiene texto y se pliega de nuevo cuando está vacío y haces clic en otro sitio o pulsas **esc**. Así queda espacio para la etiqueta de ubicación y la ruta; mientras el campo está abierto, los botones de visualización se agrupan en un único botón de menú.

## Archivos ocultos {#unsichtbare-dateien}

Los archivos cuyo nombre empieza por un punto (como `.git` o `.env`) y los archivos marcados como ocultos no se muestran normalmente.

- **⇧⌘.** (**Mostrar archivos ocultos**) los muestra u oculta en la pestaña actual. El botón de la barra de herramientas con el archivo de contorno discontinuo hace lo mismo (aparece resaltado mientras se muestran los archivos ocultos). El ojo que hay al lado es Vista Rápida.
- **Ajustes** → **General** → **Mostrar archivos ocultos** determina si las pestañas nuevas y Manta al arrancar los muestran.

Si una carpeta parece vacía pero contiene ítems ocultos, Manta te lo indica: “3 ítems ocultos — pulsa ⇧⌘. para mostrarlos.”

## Actualizar {#aktualisieren}

Las carpetas locales se mantienen actualizadas solas, como en Finder: cuando otra app crea, elimina, renombra o modifica un archivo, el cambio aparece en el panel al cabo de un momento. La selección, el cursor, la posición de desplazamiento y la selección escribiendo se mantienen. Manta solo vigila las carpetas visibles (en la visualización Columnas, también las columnas de la izquierda). Muchos cambios a la vez (al copiar miles de archivos, por ejemplo) se agrupan; la carpeta se vuelve a leer como mucho una vez por segundo. Mientras escribes un nombre (Renombrar, Nueva carpeta), la actualización espera a que termines.

Las carpetas de **servidores**, **contenedores**, **S3** y **archivos comprimidos** no se vigilan. Si otra persona cambia algo ahí, pulsa **⌘R** (**Actualizar**) para volver a leer la carpeta. Por supuesto, ⌘R también funciona en local.

## Consulta también

- [Favoritos y barra lateral](07-favoriten-seitenleiste.md)
- [Atajos de teclado](18-tastenkuerzel.md#navigation)
