# Almacenamiento en la nube {#cloud}

Manta muestra iCloud Drive, Google Drive, OneDrive, Dropbox y otros almacenamientos en la nube que tu Mac ya sincroniza. Puedes ver qué archivos están solo en la nube, descargarlos o volver a eliminar la descarga local.

![Sección Nube en la barra lateral e insignias de sincronización en la lista](images/cloud-sidebar-badges.png)

## Cómo accede Manta al almacenamiento en la nube {#prinzip}

Manta **no** inicia sesión por sí mismo en Google, Microsoft ni Dropbox. Usa las carpetas que las apps oficiales de los proveedores crean en tu Mac:

- **iCloud Drive** está integrado en macOS.
- **Google Drive**, **OneDrive**, **Dropbox**, **Box** y otros crean sus carpetas en `~/Library/CloudStorage` en cuanto instalas su app e inicias sesión en ella.

Para Manta, son carpetas normales de tu Mac. Hagas lo que hagas en Manta (copiar, renombrar, eliminar), la app del proveedor lo sincroniza después con la nube.

## Configurar Google Drive {#google-drive}

1. Descarga **Google Drive para ordenadores** de Google e instálalo.
2. Inicia sesión en la app de Google Drive con tu cuenta de Google.
3. **Google Drive** aparece solo en **Nube**, en la barra lateral: Manta detecta las carpetas nuevas de `~/Library/CloudStorage`. Si no aparece, haz clic en la flecha circular junto a **Nube**.

OneDrive y Dropbox se configuran igual: instala la app del proveedor, inicia sesión y listo.

> [!NOTE]
> Manta **no tiene un inicio de sesión de Google propio**. Sin Google Drive para ordenadores no verás Google Drive en Manta.

Mientras no haya ninguna carpeta de Google Drive, **Nube** muestra una fila **Conectar Google Drive…**. Haz clic en ella para ver los pasos de configuración y el enlace **Descargar Google Drive para ordenadores** (<https://www.google.com/drive/download/>). **No volver a mostrar** oculta la indicación; **Ajustes › General › Nube** la recupera.

## La sección Nube de la barra lateral {#seitenleiste}

La sección **Nube** está entre **Ubicaciones** y **Dispositivos**. Cada almacenamiento en la nube encontrado tiene su propia fila con el icono del proveedor. Si un proveedor aparece más de una vez (dos cuentas de Google, el OneDrive de tu empresa y bibliotecas compartidas), la cuenta aparece a continuación en gris; si no, la descripción emergente la muestra junto con la ruta:

- iCloud Drive (siempre el primero cuando está activado)
- Google Drive (con varias cuentas: Google Drive *nombre@gmail.com*)
- OneDrive *Personal* u OneDrive *nombre de tu empresa*, OneDrive *Bibliotecas compartidas*
- Dropbox, Box y otros (Nextcloud, Proton Drive o Synology Drive, por ejemplo)

Un clic abre el almacenamiento en el panel activo.

## iCloud Drive como en el Finder {#icloud-apps}

En Manta, iCloud Drive se ve como en el Finder: la carpeta superior muestra tus carpetas y archivos **y** las carpetas de tus apps, por ejemplo **Obsidian**, **Pages**, **Numbers** o **Atajos**. Manta muestra las mismas carpetas de apps que el Finder (las apps que comparten sus documentos en iCloud Drive y tienen alguno allí).

La barra de ruta empieza con la etiqueta **iCloud Drive**; en una carpeta de app le sigue el nombre de la app, por ejemplo **iCloud Drive › Obsidian › Vault**. Los nombres de carpeta con los que macOS lo guarda en el disco (`~/Library/Mobile Documents/com~apple~CloudDocs`, `iCloud~md~obsidian/Documents`) solo los ves cuando editas la ruta.

Una carpeta de app pertenece a su app: no puedes renombrarla, moverla ni eliminarla; si lo intentas, Manta te lo indica. Lo que hay dentro lo cambias con normalidad.

## Espacio libre {#freier-speicher}

Abajo, en la barra de estado, después del número de ítems aparece cuánto espacio queda libre, por ejemplo “12 ítems · 18 GB disponibles”. La descripción emergente muestra también el tamaño total, si se conoce.

| Dónde | Qué significa “disponibles” |
|---|---|
| Carpetas en tu Mac | espacio libre del disco, como en el Finder, es decir, incluido el espacio que macOS libera cuando hace falta |
| iCloud Drive | lo que aún queda libre en tu almacenamiento de iCloud |
| Google Drive (conexión propia) | lo que aún queda libre en tu almacenamiento de Google; en las unidades compartidas, nada |
| SFTP, SMB | espacio libre en el servidor |
| WebDAV (por ejemplo, Nextcloud) | tu cuota en el servidor, si este la indica |

Google Drive para ordenadores, OneDrive y Dropbox guardan sus archivos en tu Mac; ahí Manta muestra el espacio libre del disco. S3, FTP y los contenedores no indican el espacio libre; en ese caso no aparece el dato. Después de copiar, mover o eliminar, el valor se vuelve a leer.


## Qué significan las insignias {#abzeichen}

En las carpetas en la nube, Manta muestra el estado de sincronización: en la lista, como icono en el extremo derecho de la columna Nombre; en la visualización Iconos, como insignia redonda en la esquina superior derecha del icono. La descripción emergente lo explica con palabras (por ejemplo, “Solo en la nube – no descargado”).

| Insignia | Significado | Acciones posibles |
|---|---|---|
| (ninguna) | Descargado y actualizado | Eliminar descarga |
| Nube con flecha hacia abajo | Solo en la nube, no en este Mac | Descargar ahora |
| Nube con signo de exclamación | Descargado, pero hay una versión más reciente en la nube | Descargar ahora, Eliminar descarga |
| Círculo con flecha hacia abajo | Descargando | – |
| Nube con flecha hacia arriba (azul, parpadeante) | Subiendo o esperando para subir; por ejemplo, justo después de copiar a Google Drive | – |
| Signo de exclamación rojo en un círculo | Error de subida: el proveedor no ha podido subir el ítem; la descripción emergente indica el motivo (por ejemplo, “Error de subida: almacenamiento lleno”) | – |

Después de copiar a una carpeta en la nube, la app del proveedor sube los archivos en segundo plano. Mientras tanto, la barra de estado también indica “Google Drive está subiendo 2 ítems” (en rojo si hay errores). iCloud Drive puede informar además del progreso (“Subiendo… 42 %” en la descripción emergente); Google Drive, OneDrive y Dropbox solo indican que están subiendo. Los archivos que los proveedores nunca suben (`.DS_Store`, archivos de bloqueo de Office `~$…`) no reciben insignia.

Leer el estado no descarga nada, así que puedes explorar carpetas grandes en la nube sin que tu Mac lo descargue todo.

Manta lee el estado de los atributos de los archivos (el indicador `SF_DATALESS` y los valores `NSURLUbiquitousItem…` de macOS, que comunican iCloud Drive y las apps de File Provider) y lo actualiza cada pocos segundos mientras la carpeta está visible.

## Descargar archivos de la nube {#jetzt-laden}

1. Selecciona archivos o carpetas que estén solo en la nube.
2. Clic derecho → **Descargar ahora**.

Las carpetas se descargan con todo su contenido. La insignia cambia a “descargando” y desaparece cuando todo está en tu Mac.

Si abres o copias un archivo que solo está en la nube, macOS lo descarga automáticamente de todos modos.

## Liberar espacio {#download-entfernen}

1. Selecciona archivos o carpetas descargados.
2. Clic derecho → **Eliminar descarga**.

Los archivos siguen en la nube y visibles en Manta, pero ya no ocupan espacio en tu Mac.

> [!IMPORTANT]
> **Eliminar descarga** funciona de forma fiable con **iCloud Drive**. Con Google Drive, OneDrive, Dropbox y otros, es la app del proveedor la que decide si lo permite. Si se niega, Manta te lo indica. En ese caso, usa **Finder**: clic derecho → **Eliminar descarga**; Finder puede hacerlo, las demás apps no.

## Google Drive para expertos: tu propio cliente OAuth {#experte}

Manta también incluye una conexión propia con Google Drive (Drive API) que está **oculta**: Manta no incluye ningún cliente OAuth para ella. Solo si no puedes usar Google Drive para ordenadores y tienes un cliente propio:

1. En Google Cloud Console, crea un cliente OAuth del tipo **Desktop app** (app de escritorio) y activa la API de Google Drive.
2. Abre **Ajustes › General › Nube › Experto: tu propio cliente OAuth de Google**, introduce el ID de cliente y el secreto de cliente y haz clic en **Guardar**.
3. El diálogo de conexión tiene ahora un tipo **Google Drive**. Si dejas **Cuenta** vacío, Manta abre el navegador para iniciar sesión al conectarte.

**Eliminar** vuelve a quitar Google Drive del diálogo de conexión. El cliente se guarda en `~/Library/Application Support/Manta/gdrive-client.json`.

## Eliminar en carpetas en la nube {#loeschen}

> [!IMPORTANT]
> Las carpetas en la nube son carpetas de tu Mac. **⌘⌫** traslada los elementos que contienen a la **Papelera**, como en cualquier otro sitio (consulta [Eliminar](05-dateien.md#loeschen) y [Papelera](07-favoriten-seitenleiste.md#papierkorb)); **⌥⌘⌫** elimina definitivamente tras confirmar. Después, la app del proveedor sincroniza la eliminación **en la nube y en todos tus dispositivos**. Según el proveedor, el archivo se conserva un tiempo en la papelera de su sitio web.

Si solo quieres ahorrar espacio, usa **Eliminar descarga** en lugar de eliminar.

## Limitaciones {#grenzen}

- Manta solo muestra el almacenamiento en la nube que tu Mac ya sincroniza. No funciona sin la app del proveedor.
- **Eliminar descarga** en proveedores de terceros solo funciona si su app lo permite (si no, hazlo en Finder).
- La rapidez con la que los cambios llegan a la nube depende de la app del proveedor, no de Manta.
