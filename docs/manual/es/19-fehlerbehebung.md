# Solución de problemas {#fehlerbehebung}

Soluciones a problemas habituales y dónde registra Manta los errores. Si informas de un error, el archivo de registro ayuda mucho.

## El archivo de registro {#protokoll}

Manta escribe los errores y los eventos importantes en:

`~/Library/Logs/Manta/manta.log`

Para encontrarlo:

1. Abre la paleta de comandos con **⇧⌘P**.
2. Elige **Mostrar archivo de registro en Finder**. Finder abre la carpeta de registros (`~/Library/Logs/Manta`) con el archivo `manta.log`, en el propio Finder, aunque Manta sea tu gestor de archivos predeterminado.

Cada mensaje de error que Manta te muestra está ahí con el error técnico, la ruta y la ubicación (local, servidor…). El archivo rota al llegar a 5 MB; se conserva una copia anterior.

> [!TIP]
> Para obtener registros más detallados, abre Manta desde Terminal con `MANTA_LOG=debug /Applications/Manta.app/Contents/MacOS/manta`.

> [!NOTE]
> El archivo de registro puede contener nombres de archivos y de servidores. Revísalo antes de compartirlo. Las contraseñas, frases de contraseña y claves secretas no aparecen en él, tampoco para SFTP, FTP y S3.

Los plugins tienen sus propios registros: **Ajustes** → **Plugins** → **Mostrar registro** en la tarjeta del plugin.

## Manta no se abre (Gatekeeper) {#gatekeeper}

**Problema:** En el primer arranque, macOS indica *No se ha abierto “Manta”* o *No se puede abrir “Manta” porque Apple no puede comprobar si contiene software malicioso*.

**Causa:** Esta copia no está notarizada por Apple. macOS bloquea estas apps en el primer arranque.

**Solución:**

1. Haz clic en **OK**.
2. Abre **Ajustes del Sistema** → **Privacidad y seguridad**.
3. Abajo, haz clic en **Abrir igualmente** junto a Manta y confirma.

Antes de macOS 15: haz clic derecho (o Control + clic) en Manta en la carpeta **Aplicaciones** → **Abrir** → **Abrir**.

## “macOS ha bloqueado el acceso” {#berechtigungen}

**Problema:** Una carpeta no se abre, o Manta indica *macOS ha bloqueado el acceso a “…” (operación no permitida)*.

**Causa:** No has permitido que Manta acceda a esta carpeta (o has hecho clic en “No permitir” por error), o la carpeta necesita Acceso total al disco.

**Solución:**

1. Abre **Ajustes del Sistema** → **Privacidad y seguridad** → **Archivos y carpetas**.
2. Busca **Manta** y activa la carpeta (Escritorio, Documentos, Descargas, volúmenes extraíbles, volúmenes de red).
3. Para carpetas como `~/Library/Mail`: **Acceso total al disco** → activa **Manta**.
4. Cierra Manta y vuelve a abrirlo.

Si Manta no aparece en la lista, macOS todavía no ha preguntado. Abre la carpeta una vez en Manta y aparecerá la pregunta.

“No tienes permiso para abrir esta carpeta” es otra cosa: faltan los permisos Unix normales. Pide acceso al propietario de la carpeta o cambia los permisos en Finder (**Obtener información**).

## Funciones que controlan Finder o Terminal {#automation}

**Problema:** “Mostrar en Finder”, “Abrir en Terminal.app” o trasladar duplicados a la papelera no funciona.

**Solución:** **Ajustes del Sistema** → **Privacidad y seguridad** → **Automatización** → en **Manta**, activa **Finder** y **Terminal**.

## FTP: las carpetas no cargan, errores de “TLS” {#ftp-datenkanal}

**Problema:** Iniciar sesión en un servidor FTPS funciona, pero las carpetas se quedan vacías o no terminan de cargar, o Manta indica “la red bloquea las conexiones de datos FTP cifradas”.

**Causa:** Algunas redes de empresa y cortafuegos interrumpen las conexiones de datos FTP cifradas.

**Solución:**

1. Haz clic derecho en el favorito → **Editar conexión…** (o **⌘K** para una conexión nueva).
2. Ajusta **Canal de datos** a **Automático**.
3. Vuelve a conectarte.

Manta envía entonces los listados y los archivos sin cifrar; el inicio de sesión sigue cifrado. Un escudo de advertencia en la etiqueta de ubicación lo indica.

> [!WARNING]
> No es una buena solución para datos confidenciales. En ese caso, usa SFTP, otra red o una VPN, y ajusta el canal de datos a **Obligatorio**.

Más indicaciones sobre FTP:

- “No se ha podido establecer una conexión segura”: el servidor no admite FTPS en este puerto (prueba **FTP** si es aceptable en tu red) o su certificado no es de confianza.
- Manta no admite FTPS implícito (puerto 990).

## Un contenedor sin shell {#container-ohne-shell}

**Problema:** Un contenedor se abre en modo de solo lectura, o Manta indica “El contenedor no tiene shell”.

**Causa:** Manta necesita `sh` en el contenedor para listar y modificar archivos. Las imágenes distroless y scratch no tienen shell.

**Solución:**

- Leer y copiar hacia fuera (**F5**, arrastrar) sigue funcionando, mediante `docker cp`.
- Para hacer cambios necesitas una imagen con shell (una variante de depuración de la imagen, por ejemplo).
- Los pods de Kubernetes necesitan `sh` incluso para leer.
- Los contenedores detenidos también son de solo lectura: inícialos para hacer cambios.

## Docker o Kubernetes no aparecen {#container-fehlen}

- **“Docker no se está ejecutando”**: abre Docker Desktop.
- **“Docker no está instalado”**: instala Docker Desktop o asegúrate de que el comando `docker` está en tu PATH. Manta también busca en `/opt/homebrew/bin`, `/usr/local/bin` y dentro de la app Docker Desktop.
- **“kubectl no está instalado”**: `brew install kubectl`.
- **“No se puede acceder al clúster de Kubernetes”**: comprueba el contexto y tu VPN, o inicia tu clúster local.

## Conexiones a servidores {#server}

| Mensaje | Solución |
|---|---|
| “La clave del servidor ha cambiado” | La clave no coincide con `~/.ssh/known_hosts`; Manta no se conecta (posible ataque de intermediario). Pide la huella al administrador. Si de verdad se ha reinstalado el servidor, elimina la entrada antigua con el comando del diálogo (`ssh-keygen -R host`). |
| “Servidor desconocido” | Primera conexión con este servidor. Compara la huella y haz clic en **Conectar y recordar**; consulta [Conexiones](08-verbindungen.md). |
| “No se ha encontrado la contraseña guardada en el Llavero” | Se ha eliminado el ítem, por ejemplo en Acceso a Llaveros. Introduce la contraseña y deja activado **Recordar contraseña**. |
| “No hay ninguna clave utilizable en ssh-agent” | `ssh-add ~/.ssh/id_ed25519` |
| “El reloj de tu Mac no está en hora” | **Ajustes del Sistema** → **General** → **Fecha y hora** → ajustar automáticamente. |
| “El bucket está en otra región” | Ajusta la región del diálogo a la indicada. |
| “Se ha perdido la conexión con el servidor” | Comprueba la red y después haz clic en **Reintentar**. |

## Una transferencia no avanza {#uebertragungen}

| Lo que ves | Solución |
|---|---|
| “Esperando conexión… (intento 5, siguiente en 16 s)” | La red o el servidor no están disponibles. Manta sigue intentándolo y continúa en el mismo punto. Comprueba la Wi-Fi o la VPN; **Reintentar ahora** inicia el siguiente intento al instante. |
| **No hay conexión a internet** en la parte superior de **Transferencias** | Tu Mac no tiene conexión. En cuanto vuelva a tenerla, las transferencias continuarán solas. |
| “Requiere una conexión con…” | Tras un reinicio falta la conexión con el servidor. Haz clic en **Reconectar** e introduce la contraseña. |
| “En pausa · …” | Tú (o **Pausar todo**) has pausado la tarea. Haz clic en **Reanudar**. |
| Archivos ocultos `.NOMBRE.manta-part` en el destino | Archivos parciales de una tarea interrumpida. Déjalos mientras quieras reanudar la tarea; si no, puedes eliminarlos. |

Consulta [Pausar y reanudar transferencias](05-dateien.md#pausieren).

## Una actualización falla {#updates}

Consulta [Actualizaciones](17-updates.md#blockiert). Normalmente, Manta no se está ejecutando desde la carpeta **Aplicaciones**.

## Nube: “Eliminar descarga” no funciona {#cloud}

Con Google Drive, OneDrive, Dropbox y otros, decide la app del proveedor. Usa Finder: clic derecho → **Eliminar descarga**. Consulta [Almacenamiento en la nube](09-cloud.md#download-entfernen).

## Una carpeta muestra contenido desactualizado {#aktualisieren}

Pulsa **⌘R**. Manta solo ve los cambios hechos por otras apps o por el servidor después de actualizar.

## Restablecer Manta {#zuruecksetzen}

Como último recurso:

1. Cierra Manta.
2. Renombra `~/Library/Application Support/Manta` (por ejemplo, a `Manta antiguo`).
3. Abre Manta. Se iniciará como si fuera la primera vez.

Tus favoritos y tus acciones rápidas siguen en la carpeta renombrada.
