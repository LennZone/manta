# Tus propios plugins {#eigene-plugins}

Puedes ampliar Manta con tus propios plugins, en cualquier lenguaje de programación. Este capítulo explica qué puede hacer un plugin, cómo instalarlo, actualizarlo y encontrarlo, y qué debes tener en cuenta. La guía técnica está en la documentación para desarrolladores.

## Qué pueden hacer los plugins {#moeglichkeiten}

- Mostrar **insignias** junto a los nombres de archivo (como las insignias de Git).
- Mostrar **etiquetas** en la barra de ruta (como la rama o el proyecto).
- Ofrecer **comandos** para la paleta de comandos y el menú contextual, con un atajo si quieres.
- Mostrar **paneles** bajo un panel de archivos, en la barra lateral derecha o como diálogo.
- Mostrar **elementos en la barra de estado**, con icono, una pequeña barra y color de aviso (como [Carga del sistema](14-plugins.md#systemauslastung)).
- Mostrar **avisos**.

Un plugin es una carpeta con un archivo `plugin.toml` y un programa (un script de Python, por ejemplo). Manta inicia el programa e intercambia mensajes con él (líneas JSON a través de stdin/stdout). Un archivo `.mantaplugin` es simplemente esa carpeta comprimida en ZIP.

## Instalar un plugin {#installieren}

1. Abre **Ajustes** (**⌘,**) → **Plugins**.
2. Haz clic en **Instalar plugin…** y elige **Desde un archivo (.mantaplugin, .zip)…** o **Desde una carpeta…** (la que contiene `plugin.toml`). Más rápido: haz **doble clic** en un archivo `.mantaplugin` en el Finder o **arrástralo** a la página Plugins. Los plugins del directorio de la comunidad se instalan desde [Descubrir](#entdecken).
3. Antes de instalar nada, Manta muestra un resumen: nombre, versión, autor, descripción, **Lo que puede hacer** (por ejemplo “Lee tus archivos”, “Modifica o elimina archivos”, “Usa la red”, “Ejecuta otros programas”, “Ejecuta comandos en el Terminal”), el **Comando** que ejecuta, el **Origen**, un enlace al **Código fuente**, la **Licencia** y el tamaño. Debajo, un aviso de que Manta no revisa los plugins.
4. Haz clic en **Instalar**. El plugin aparece entonces en **Instalados**, todavía desactivado.

![Resumen antes de instalar un plugin](images/plugin-install.png)

> [!NOTE]
> Los permisos los declara el autor del plugin, y Manta los **impone**: el plugin se ejecuta en un entorno aislado de macOS y no puede hacer nada más (consulta [Seguridad](#sicherheit)).

### Activarlo {#einschalten}

Haz clic en **Activar** en la tarjeta del plugin. La primera vez, Manta te pide que lo confirmes y vuelve a mostrar el autor, la versión, los permisos, el comando y la carpeta en la que se ejecuta.

![Confirmación al activar un plugin externo](images/plugin-confirm.png)

Si descargaste el plugin de internet, macOS mantiene sus archivos en cuarentena y Gatekeeper bloquearía los programas que contiene. La confirmación lo indica: solo cuando activas el plugin, Manta quita la marca de cuarentena de sus archivos para que sus programas puedan ejecutarse; nunca lo hace sin avisar.

Más botones en la tarjeta de un plugin: **Recargar**, **Mostrar registro**, **Mostrar carpeta**, **Desinstalar**. Arriba: **Abrir carpeta de plugins** y **Recargar** (busca plugins añadidos, modificados o eliminados).

## Actualizar y desinstalar {#aktualisieren}

- **Actualizar**: cuando el directorio de plugins tiene una versión más reciente, la tarjeta del plugin muestra “La versión … está disponible en el directorio de plugins” con un botón **Actualizar**. También puedes instalar la nueva versión del mismo modo, desde un archivo o una carpeta. Si tiene el mismo id que un plugin instalado, el resumen indica que es una actualización y muestra en **Instalado** la versión que se reemplazará. Tras confirmar, Manta reemplaza el plugin y conserva si estaba activado o desactivado.
- **Desinstalar**: haz clic en **Desinstalar** en la tarjeta del plugin. Manta elimina la carpeta del plugin; su carpeta de datos se conserva.

## Encontrar plugins {#entdecken}

En **Ajustes** → **Plugins**, haz clic en **Descubrir** (o elige **Explorar plugins…** en la paleta de comandos). Manta muestra los plugins del directorio de la comunidad: nombre y descripción en tu idioma, autor, licencia, la versión de Manta que necesita, sus permisos y un enlace a su código fuente.

1. Haz clic en **Instalar** (o en **Actualizar** si hay una versión más reciente de un plugin instalado).
2. Manta descarga el plugin y compara su suma de comprobación SHA-256 con la del directorio. Si no coinciden, no se instala nada.
3. Aparece el resumen habitual (“El SHA-256 coincide con el directorio de plugins”); continúa como se describe [arriba](#installieren).

Los plugins que necesitan una versión más reciente de Manta o que no funcionan en tu Mac se muestran, pero no se pueden instalar. **En la web** abre el directorio en [manta-filemanager.com/plugins](https://manta-filemanager.com/plugins/).

> [!NOTE]
> **Privacidad:** Manta carga la lista desde GitHub solo cuando abres **Descubrir** (una vez por sesión) o haces clic en **Actualizar**, nunca en segundo plano. Guarda la última lista, así que Descubrir también funciona sin conexión y los plugins instalados pueden ofrecer actualizaciones sin conectarse a internet.

## Seguridad {#sicherheit}

Los plugins externos se ejecutan en un **entorno aislado (sandbox) de macOS**: solo pueden hacer lo que permiten sus permisos. Sin permisos, un plugin solo puede leer su propia carpeta y el sistema, y escribir en su propia carpeta de datos.

| Permiso | Permite al plugin |
|---|---|
| Lee tus archivos | leer cualquier archivo |
| Modifica o elimina archivos | escribir, crear y eliminar archivos en cualquier lugar |
| Usa la red | conectarse a internet y a tu red |
| Ejecuta otros programas | iniciar otros programas (que siguen en el entorno aislado) |
| Ejecuta comandos en el Terminal | escribir comandos en el Terminal de Manta |

Las mismas reglas se aplican a lo que un plugin pide a Manta que haga por él (leer un archivo, listar una carpeta, renombrar archivos). La tarjeta del plugin muestra **Aislado**; su descripción emergente enumera los permisos. Los plugins integrados forman parte de Manta y no están aislados.

Si el entorno aislado no funciona en tu Mac, la tarjeta muestra **Sin aislar** y el motivo, y el resumen indica que los permisos no se pueden imponer. En ese caso, el plugin se ejecuta con los permisos de tu usuario.

> [!WARNING]
> El entorno aislado no hace inofensivo a un plugin: un plugin que puede leer tus archivos y usar la red puede enviar tus archivos a otro lugar. Manta no revisa los plugins, tampoco los del directorio de plugins. Instala solo plugins de confianza y lee el código de los plugins de internet antes de activarlos.

Por eso Manta muestra un resumen antes de instalar, los plugins nuevos empiezan desactivados y Manta pregunta la primera vez que activas uno.

## Cuando un plugin no funciona {#probleme}

- La tarjeta muestra **Error** y el motivo, por ejemplo cuando falta `python3`.
- **Mostrar registro** muestra los últimos mensajes del plugin.
- Si un plugin falla, Manta lo reinicia, como máximo tres veces por minuto. Después se queda en estado de error hasta que haces clic en **Recargar**.
- Si la carpeta de plugins contiene dos carpetas con el mismo id de plugin, la segunda muestra **No se puede cargar** y un mensaje. Elimina una de las dos.

## Dónde están los plugins {#speicherorte}

| Qué | Dónde |
|---|---|
| Plugins instalados | `~/Library/Application Support/Manta/plugins/<id>/` |
| Datos y registro de un plugin | `~/Library/Application Support/Manta/plugin-data/<id>/` (registro: `plugin.log`) |
| Estado activado/desactivado | `~/Library/Application Support/Manta/plugins.json` |
| Última lista del directorio de plugins | `~/Library/Application Support/Manta/plugin-registry.json` |
| Reglas del entorno aislado de un plugin | `~/Library/Application Support/Manta/plugin-data/<id>/sandbox.sb` |

Al desinstalar se elimina la carpeta del plugin; su carpeta de datos se conserva.

## Escribir un plugin tú mismo {#schreiben}

La forma más rápida de empezar es el ejemplo **Hello Python** (`examples/plugins/hello-python` en el código fuente, comentado línea a línea, solo con la biblioteca estándar de Python). También puedes instalarlo desde **Ajustes** → **Plugins** → **Disponibles**. También hay plantillas para Node.js, shell (bash + jq) y Rust (`examples/plugins/`); para Rust existe además el crate `manta-plugin-protocol`.

Guía, referencia del protocolo y ejemplos: [Escribir plugins para Manta (docs/plugins.md)](../../plugins.md) (en inglés).

Los plugins pueden hablar todos los idiomas de Manta: Manta les comunica el idioma actual, y `plugin.toml` puede incluir un nombre y una descripción traducidos (`[localized.es]`, `[localized.de]`, etc.).

## Desarrollar y probar {#entwickeln}

- **Recargar plugins** (paleta de comandos) reinicia todos los plugins externos en ejecución y vuelve a examinar la carpeta de plugins.
- **Modo de desarrollo**: inicia Manta desde el Terminal con
  `/Applications/Manta.app/Contents/MacOS/manta --plugin-dev ~/code/my-plugin`
  (se puede repetir, o usa la variable de entorno `MANTA_PLUGIN_DEV=dir1:dir2`). Durante esta sesión el plugin se ejecuta directamente desde esa carpeta: de confianza, activado y con la etiqueta **Desarrollo**. Mientras tanto, una copia instalada con el mismo id se aparta; no se guarda nada.
- **Probar sin Manta**: `python3 scripts/plugin-test.py ruta/al/plugin`.
- **Empaquetar**: `python3 scripts/plugin-pack.py ruta/al/plugin` crea el archivo `.mantaplugin` y muestra su suma de comprobación SHA-256 para el directorio.
- **Entorno aislado**: declara los permisos que necesita tu plugin; sin ellos no puede hacerlo. Si algo se rechaza, el registro lo indica (`refused …`).
- **Registro**: **Mostrar registro** en la tarjeta, o el archivo `~/Library/Application Support/Manta/plugin-data/<id>/plugin.log`.

Más información: [Develop and debug (docs/plugins.md)](../../plugins.md#develop-and-debug) (en inglés).

## Publicar un plugin {#veroeffentlichen}

Pon el plugin en un repositorio público con una licencia aprobada por la OSI, adjunta un archivo `.mantaplugin` a una release y abre un pull request que lo añada a `plugins/registry.json`. Después aparecerá en **Descubrir**. Detalles: [Publishing a plugin (docs/plugins.md)](../../plugins.md#publishing-a-plugin) (en inglés).
