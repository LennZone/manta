# Conexiones {#verbindungen}

Manta abre carpetas de servidores (SFTP, FTP, FTPS, WebDAV, SMB), de almacenamiento S3 y de contenedores de Docker y Kubernetes como si fueran carpetas locales. Ahí copias, mueves y renombras igual que en tu Mac.

## Conectarse a un servidor {#verbinden}

1. Pulsa **⌘K** (**Conectarse al servidor…**), haz clic en **Conectar** en la barra de herramientas o haz clic en el **+** junto a **Servidores** en la barra lateral.
2. Elige el **Protocolo** arriba: **SFTP**, **FTP**, **FTPS**, **WebDAV**, **SMB**, **S3** o **Contenedor**.
3. Rellena los campos (consulta más abajo).
4. Opcional: haz clic en **Probar conexión**. Manta informa, por ejemplo: “Sesión iniciada. Carpeta de inicio: /home/anna”.
5. Haz clic en **Conectar**.

El servidor se abre en el panel activo (el diálogo te indica cuál). La conexión aparece en **Servidores**, en la barra lateral.

![El diálogo Conectarse al servidor para SFTP](images/connect-sftp.png)

> [!TIP]
> Puedes escribirlo todo de una vez en el campo **Servidor**: `usuario@host:puerto`. Manta lo reparte entre los campos.

## SFTP {#sftp}

SFTP es el estándar seguro para servidores Linux y muchos planes de alojamiento.

| Campo | Significado |
|---|---|
| **Servidor**, **Puerto** | La dirección del servidor; el puerto 22 si no lo cambias. |
| **Usuario** | Tu nombre de usuario en el servidor. |
| **Iniciar sesión con** | **Contraseña**, **Archivo de clave** (con una **Frase de contraseña** opcional, “solo si la clave está cifrada”) o **Agente SSH** (“Usa las claves cargadas en tu ssh-agent en ejecución.”). |
| **Carpeta** | Opcional; por omisión, tu carpeta de inicio en el servidor. |

### Comprobación de la clave del servidor {#server-schluessel}

> [!WARNING]
> Manta comprueba la clave del servidor con tu archivo `~/.ssh/known_hosts`, el mismo que usa `ssh` en Terminal.
>
> - **Servidor conocido, clave coincidente:** Manta se conecta sin preguntar.
> - **Servidor desconocido** (no está en `known_hosts`): Manta muestra el diálogo **Servidor desconocido** con el servidor, el tipo de clave y la huella (`SHA256:…`, tal como la muestra `ssh-keygen -lf`). Compara la huella con la que te facilite tu administrador o tu proveedor de alojamiento. **Conectar y recordar** añade la clave a `known_hosts` (`host`, o `[host]:puerto` para un puerto distinto del 22) y se conecta; **Cancelar** no se conecta.
> - **Clave cambiada:** Manta **no** se conecta y muestra una advertencia en rojo (posible ataque de intermediario). No hay a propósito ningún botón de “conectar igualmente”. Esto también ocurre cuando se reinstala un servidor: pide al administrador la huella correcta. Si coincide, elimina la entrada antigua en Terminal con el comando del diálogo (por ejemplo, `ssh-keygen -R host`, **Copiar comando**) y vuelve a conectarte.
>
> Las transferencias que se reconectan en segundo plano tras un reinicio nunca preguntan: con una clave desconocida esperan a **Reconectar**; con una clave cambiada, fallan.

## FTP y FTPS {#ftp}

**FTP** lo envía todo sin cifrar, incluida tu contraseña. Úsalo solo en tu red local o con servidores públicos. **Anónimo** inicia sesión sin contraseña, si el servidor lo permite.

**FTPS** es FTP con cifrado (TLS explícito, normalmente en el puerto 21). Manta no admite FTPS implícito en el puerto 990.

### El canal de datos de FTPS {#datenkanal}

FTP usa dos conexiones: una para iniciar sesión y enviar comandos, y otra (el “canal de datos”) para los listados de carpetas y los archivos. Algunos cortafuegos de empresa bloquean las conexiones de datos cifradas. Por eso puedes elegir un **Canal de datos**:

| Ajuste | Comportamiento |
|---|---|
| **Automático** (por omisión) | Cifrado. Si la red bloquea las conexiones de datos cifradas, las transferencias continúan sin cifrar; el inicio de sesión sigue cifrado. |
| **Obligatorio** | Siempre cifrado. Falla si la red bloquea las conexiones de datos cifradas. |
| **Desactivado** | Los listados y los archivos viajan sin cifrar; el inicio de sesión sigue cifrado. |

Cuando **Automático** recurre a un canal de datos sin cifrar, lo notarás:

- La etiqueta de ubicación de la barra de ruta muestra un escudo de advertencia.
- Un aviso único indica “Conexión de datos sin cifrar (la red bloquea TLS) – el inicio de sesión sigue cifrado”.

> [!WARNING]
> En ese modo alternativo, **el contenido y los nombres de los archivos viajan sin cifrar**; solo el nombre de usuario y la contraseña siguen protegidos. Para datos confidenciales, ajusta el canal de datos a **Obligatorio** y usa otra red, o SFTP.

El ajuste se guarda con el favorito. Consulta también [Solución de problemas: canal de datos FTP](19-fehlerbehebung.md#ftp-datenkanal).

![Diálogo FTPS con la opción del canal de datos](images/connect-ftps-data-channel.png)

## S3 y almacenamiento compatible con S3 {#s3}

**S3** se conecta a Amazon S3 y a servicios compatibles. Los buckets y prefijos aparecen como carpetas.

| Campo | Significado |
|---|---|
| **Proveedor** | Amazon S3, Cloudflare R2, MinIO, Backblaze B2, Wasabi, DigitalOcean Spaces, Hetzner Object Storage u **Otro compatible con S3**. El preajuste rellena el endpoint y la región. |
| **ID de cuenta** | Solo Cloudflare R2 (panel de Cloudflare › R2 › Account ID). |
| **Región**, **Endpoint** | El endpoint suele depender de la región. Si un bucket está en otra región, Manta lo corrige por sí mismo. |
| **Iniciar sesión con** | **Perfil de AWS** (claves de `~/.aws/credentials`), **Clave de acceso** (ID de clave de acceso y clave secreta) o **Público** (para buckets públicos). |
| **Bucket** | Opcional; si no, ves todos los buckets. |
| **Direccionamiento por ruta** | Lo necesitan MinIO y la mayoría de los servidores autoalojados. |

![Diálogo S3 con un preajuste de proveedor](images/connect-s3.png)

Conviene saber:

- Renombrar y mover dentro de un bucket es, internamente, “copiar y eliminar”. Los objetos de más de 5 GB no se pueden renombrar.
- En S3 no se pueden definir fechas de modificación ni permisos.
- Al eliminar el último archivo de una “carpeta”, la carpeta desaparece si no tiene un marcador de carpeta propio.
- Los perfiles de AWS que usan SSO, `credential_process` o assume-role no funcionan; solo las claves estáticas.
- Si el reloj de tu Mac tiene un desfase de más de 15 minutos, S3 rechaza las solicitudes (“El reloj de tu Mac no está en hora”).

## WebDAV {#webdav}

**WebDAV** se conecta a Nextcloud, ownCloud, sistemas NAS (Synology, QNAP), servidores Apache o nginx y otros servicios WebDAV.

| Campo | Significado |
|---|---|
| **Servidor**, **Puerto** | La dirección del servidor. Puedes pegar una URL completa como `https://cloud.example.com/remote.php/dav/files/anna`; Manta rellena los campos. |
| **Seguridad** | **HTTPS** (puerto 443) o **HTTP (sin cifrar)** (puerto 80). |
| **Usuario**, **Iniciar sesión con** | **Contraseña** o **Anónimo**. Manta admite la autenticación Basic y Digest. |
| **Tipo de servidor** | **Servidor WebDAV**, **Nextcloud** u **ownCloud**. Nextcloud y ownCloud rellenan la **Ruta WebDAV** `/remote.php/dav/files/<nombre de usuario>`. |
| **Ruta WebDAV** | Dónde empieza la carpeta WebDAV en el servidor, por ejemplo `/dav`. Vacío: la raíz del servidor. |
| **Carpeta** | Opcional: la carpeta que se abre dentro. |

Conviene saber:

- macOS comprueba los certificados, como en Safari. Un certificado autofirmado funciona en cuanto confías en él en Acceso a Llaveros.
- Las descargas interrumpidas continúan donde se detuvieron. Las subidas interrumpidas empiezan el archivo de nuevo (WebDAV no puede añadir datos a un archivo).
- Copiar dentro del mismo servidor usa la función de copia del propio servidor (sin descargar y volver a subir).
- Las fechas de modificación se pueden establecer cuando el servidor lo permite (Nextcloud, ownCloud); si no, las copias reciben la hora de la subida.

> [!TIP]
> Con la autenticación de doble factor en Nextcloud u ownCloud, inicia sesión con una **contraseña de app** (Ajustes → Seguridad en la interfaz web), no con tu contraseña normal.

> [!WARNING]
> Con **HTTP (sin cifrar)**, la contraseña y todos los archivos se transmiten sin cifrar. Usa HTTP solo en una red de confianza.

![Diálogo de conexión para WebDAV con el ajuste de Nextcloud](images/connect-webdav.png)

## SMB {#smb}

**SMB** abre recursos compartidos de PC con Windows, sistemas NAS y otros Mac. La conexión la establece el propio macOS (igual que **Conectarse al servidor** en el Finder) y monta el recurso compartido en `/Volumes`; después Manta lo muestra como una carpeta local.

| Campo | Significado |
|---|---|
| **Servidor** | Nombre o dirección del servidor, por ejemplo `nas.local`. `smb://servidor/recurso/carpeta` rellena todos los campos. |
| **Usuario**, **Iniciar sesión con** | **Contraseña** o **Invitado** (sin contraseña, o con tu tíquet de Kerberos o la contraseña guardada en tu Llavero). |
| **Recurso compartido** | El recurso compartido que se abre. **Explorar…** muestra los recursos compartidos del servidor; haz clic en uno para elegirlo. |
| **Carpeta** | Opcional: una carpeta dentro del recurso compartido. |

Conviene saber:

- Un recurso compartido que ya está montado (por ejemplo, por el Finder) se usa tal cual y sigue montado al desconectarte. Un recurso compartido que montó Manta se expulsa de nuevo al desconectarte o salir de Manta.
- El chip de ubicación muestra, por ejemplo, “SMB · nas.local/Team”.
- Las copias interrumpidas continúan donde se detuvieron. Si el recurso compartido desaparece (cambio de red, reposo), Manta lo vuelve a montar antes de reintentar.
- La lista de recursos compartidos viene de `smbutil`. Si el servidor pide una contraseña para ella, Manta pasa brevemente a `smbutil` la contraseña que introdujiste.

![Diálogo de conexión para SMB con la lista de recursos compartidos](images/connect-smb.png)

### Volúmenes de red en la barra lateral {#netzwerk-volumes}

Los recursos compartidos que conectaste en el Finder (SMB, AFP, NFS, WebDAV) aparecen en **Dispositivos** con un icono de red. Pasa el puntero por encima y haz clic en el icono de expulsión (**Expulsar**) para desmontarlo. Si has montado algo nuevo, haz clic en la flecha junto a **Dispositivos**.

## Docker y Kubernetes {#container}

Manta abre los archivos **dentro de contenedores** como si fueran una carpeta. Usa los comandos `docker` y `kubectl`, que deben estar instalados en tu Mac.

### Desde la barra lateral

La forma más rápida: haz clic en un contenedor de la sección **Contenedores** de la barra lateral. Se abre en su directorio de trabajo.

### Desde el diálogo

1. Pulsa **⌘K** y elige **Contenedor**.
2. Elige el **Motor**: **Docker** o **Kubernetes**.
3. Docker: elige el contenedor de la lista o escribe su nombre o ID. Kubernetes: elige el **Contexto**, el **Namespace**, el **Pod** y, si hace falta, el contenedor.
4. Haz clic en **Conectar**.

Los favoritos guardan el contenedor **por nombre**, así que un favorito sigue funcionando después de volver a crear el contenedor.

![Selector de contenedores en el diálogo de conexión](images/connect-container.png)

### Contenedores detenidos e imágenes sin shell

- **Los contenedores detenidos** se abren en modo de **solo lectura** (etiqueta **Detenido**). Puedes copiar archivos hacia fuera; inicia el contenedor para hacer cambios.
- Los contenedores **sin shell** (imágenes distroless o scratch) también son de solo lectura.
- Manta lee de una vez cada carpeta de un contenedor detenido; si ocupa más de 512 MB, indica “demasiado grande”. En ese caso, inicia el contenedor.

### Una shell en el contenedor

Clic derecho → **Abrir shell en el contenedor** (también en la paleta de comandos) abre una shell dentro del contenedor en el terminal situado bajo el panel, justo en la carpeta que estás viendo. Consulta [Terminal](11-terminal.md#container-ssh).

> [!NOTE]
> Manta solo modifica los archivos sobre los que actúas tú. Nunca inicia, detiene ni elimina contenedores.

## Conexiones como favoritos {#favoriten}

1. Activa **Añadir a Favoritos** en el diálogo de conexión.
2. Si quieres, introduce un nombre para la barra lateral.
3. Opcional: **Recordar contraseña** (o **Recordar frase de contraseña**, **Recordar clave secreta**).

> [!NOTE]
> Las contraseñas, frases de contraseña y claves secretas de S3 recordadas se guardan en el **Llavero de macOS** (llavero de inicio de sesión, ítem “com.manta-filemanager.manta.server”, cuenta como `sftp://user@host:22`), no en `favorites.json`. El archivo solo anota que existe un ítem. Manta lee la contraseña al conectarse; puede que macOS pregunte una vez si Manta puede usar el Llavero.
>
> Las versiones anteriores guardaban las contraseñas recordadas sin cifrar en `favorites.json`. En el primer arranque, Manta las traslada al Llavero y reescribe el archivo sin ellas. Si eso falla (Llavero bloqueado o acceso denegado), el archivo no se modifica y Manta lo vuelve a intentar en el siguiente arranque.
>
> Al eliminar un favorito también se elimina su ítem del Llavero (salvo que otro favorito de la misma cuenta de servidor lo siga usando). Al editar un favorito, el campo de contraseña indica “guardada en el Llavero”; déjalo vacío para conservar la contraseña guardada.

Para cambiar un favorito, haz clic derecho → **Editar conexión…** (el diálogo “Editar conexión”).

## Desconectar {#trennen}

Pasa el puntero sobre la conexión en **Servidores**, en la barra lateral, y haz clic en el icono de expulsión (**Desconectar**). Los paneles que mostraban el servidor indican “Desconectado”. En un recurso compartido SMB, desconectarte también lo expulsa si lo montó Manta.

## Cuando algo no funciona {#fehler}

Manta convierte los errores técnicos en mensajes sencillos con una indicación. Los más habituales:

| Mensaje | Qué puedes hacer |
|---|---|
| *No se ha encontrado “host”* | Comprueba si la dirección del servidor tiene erratas. |
| “host ha rechazado la conexión en el puerto …” | ¿Son correctos el puerto y el protocolo? ¿Está en marcha el servidor? |
| “host no responde” | Comprueba tu red, la VPN y el cortafuegos. |
| “El servidor no ha aceptado estas credenciales” | Comprueba el nombre de usuario y la contraseña, o prueba otro método de inicio de sesión. |
| “No hay ninguna clave utilizable en ssh-agent” | Añade una clave con `ssh-add`. |
| “No se ha podido establecer una conexión segura” | El servidor no admite FTPS en este puerto o su certificado no es de confianza. |
| “Docker no se está ejecutando” | Abre Docker Desktop. |
| “kubectl no está instalado” | `brew install kubectl` |

Más ayuda: [Solución de problemas](19-fehlerbehebung.md).

## Limitaciones {#grenzen}

- Sin FTPS implícito (puerto 990).
- Kubernetes: sin redirección de puertos ni contenedores de depuración efímeros; los pods necesitan `sh`.
- La escritura en contenedores se hace mediante `cat`; los archivos nuevos pertenecen al usuario con el que se ejecuta `exec`.
- Google Drive y similares no se conectan aquí, sino mediante las carpetas en la nube de tu Mac; consulta [Almacenamiento en la nube](09-cloud.md). Manta no incluye un inicio de sesión de Google propio; solo si introduces tu **propio** cliente OAuth en Ajustes aparece también aquí “Google Drive”; consulta [Google Drive para expertos](09-cloud.md#experte).
- WebDAV: sin bloqueos (`LOCK`); las subidas interrumpidas empiezan de nuevo.
- SMB necesita la compatibilidad con SMB de macOS; Manta no tiene un cliente SMB propio. Con un puerto distinto del predeterminado, puede que la lista de recursos compartidos no funcione.
