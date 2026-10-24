# Primeros pasos {#erste-schritte}

Manta es un gestor de archivos para Mac con varios paneles uno al lado del otro. Este capítulo explica cómo instalar Manta, qué ocurre al abrirlo por primera vez y por qué macOS te pide permisos.

![Manta con dos paneles y la barra lateral](images/overview-two-panes.png)

## Requisitos del sistema {#voraussetzungen}

- macOS 11 (Big Sur) o posterior.
- Un Mac con chip de Apple (M1 o posterior).

## Instalar Manta {#installieren}

1. Haz doble clic en `Manta-<version>.dmg`.
2. En la ventana que se abre, arrastra **Manta** a la carpeta **Aplicaciones**.
3. Expulsa la imagen de disco (haz clic en el icono de expulsión en Finder).
4. Abre Manta desde la carpeta **Aplicaciones**, por ejemplo con Launchpad o Spotlight.

> [!IMPORTANT]
> Abre Manta siempre desde la carpeta **Aplicaciones**, no directamente desde la imagen de disco. Solo así Manta puede actualizarse ([Actualizaciones](17-updates.md)) y convertirse en tu gestor de archivos predeterminado.

![La ventana de instalación de la imagen de disco](images/dmg-window.png)

## El primer arranque: Gatekeeper {#gatekeeper}

Si tu copia de Manta no está notarizada por Apple, macOS bloquea el primer arranque con un mensaje como *No se ha abierto “Manta”*. Es una función de seguridad de macOS (Gatekeeper), no un error de Manta.

Para abrir Manta de todos modos:

1. Haz clic en **OK** en el mensaje (no en “Trasladar a la papelera”).
2. Abre **Ajustes del Sistema** → **Privacidad y seguridad**.
3. Desplázate hasta la sección **Seguridad**. Ahí aparece un aviso sobre Manta.
4. Haz clic en **Abrir igualmente** y confirma con tu contraseña o con Touch ID.

En versiones anteriores a macOS 15 basta con hacer Control + clic en Manta en Finder y elegir **Abrir**.

Solo tienes que hacerlo una vez. Consulta también [Solución de problemas](19-fehlerbehebung.md#gatekeeper).

## Por qué macOS pide permisos {#berechtigungen}

macOS protege ciertas carpetas y funciones. Todas las apps tienen que pedir permiso una vez antes de usarlas, también un gestor de archivos. Las preguntas no aparecen todas al abrir la app: cada una surge la primera vez que abres esa carpeta o usas esa función.

| macOS pregunta por… | Cuándo | Por qué lo necesita Manta |
|---|---|---|
| Las carpetas Escritorio, Documentos y Descargas | La primera vez que abres la carpeta | Para mostrar y gestionar los archivos que contiene. |
| Volúmenes extraíbles y de red | La primera vez que abres una unidad USB o un recurso compartido de red | Lo mismo para unidades externas y recursos compartidos de servidores. |
| Red local | La primera vez que te conectas a un servidor de tu red | Para servidores SFTP, FTP y S3 de tu red local (un NAS, por ejemplo). |
| Automatización (Finder, Terminal) | Cuando una función controla Finder o Terminal | Por ejemplo, para trasladar ítems a la papelera (Duplicados), mostrar archivos en Finder o ejecutar acciones rápidas como “Abrir en Terminal.app”. |

Si haces clic en “No permitir”, Manta muestra un aviso al abrir la carpeta. Puedes conceder el acceso más tarde: **Ajustes del Sistema** → **Privacidad y seguridad** → **Archivos y carpetas** (o **Automatización**, **Red local**).

> [!TIP]
> Algunas carpetas (como `~/Library/Mail` o los datos de Time Machine) solo se abren con **Acceso total al disco**. Concédelo en **Ajustes del Sistema** → **Privacidad y seguridad** → **Acceso total al disco**. Es opcional; la mayoría de la gente no lo necesita.

> [!NOTE]
> Manta no recopila datos de uso. Solo se conecta a internet para las conexiones que abres tú y para buscar actualizaciones (consulta [Actualizaciones y privacidad](17-updates.md#datenschutz)).

## Gestor de archivos predeterminado {#standard-dateimanager}

En los primeros arranques, Manta pregunta: **¿Usar Manta como gestor de archivos predeterminado?**

- **Establecer por omisión**: “Mostrar en Finder” en otras apps abrirá entonces Manta, igual que las carpetas que abren otras apps o el Terminal (por ejemplo, `open ~/Downloads`).
- **Ahora no**: Manta volverá a preguntar en un arranque posterior (como máximo tres veces).
- **No volver a preguntar**: Manta no vuelve a preguntar nunca.

Puedes cambiarlo en cualquier momento en **Ajustes** → **General** → **Gestor de archivos predeterminado**, o con **Gestor de archivos predeterminado…** en la paleta de comandos.

![La pregunta sobre el gestor de archivos predeterminado](images/default-file-manager-prompt.png)

Lo que macOS permite y lo que no:

- El escritorio, el icono de Finder en el Dock y los dobles clics dentro de ventanas de Finder se quedan **siempre** con Finder. macOS no permite que ninguna app sustituya a Finder por completo.
- A partir de macOS 26, macOS solo entrega a Finder las carpetas que abren otras apps. Manta solo asume entonces “Mostrar en Finder”; los ajustes te lo indican.
- Al desactivarlo, la función vuelve a la app anterior (otro gestor de archivos, por ejemplo) o a Finder.
- Solo está disponible cuando Manta se ejecuta desde **Aplicaciones** (o `~/Applications`).

## La ventana de un vistazo {#ueberblick}

- **Barra de herramientas** arriba: Nueva carpeta, Renombrar, Eliminar, Copiar/Mover al otro panel, archivos ocultos, Comparar y sincronizar, Transferencias, Conectar, Comandos y Ajustes.
- **Barra lateral** a la izquierda: Favoritos, Ubicaciones, Dispositivos, Servidores y Contenedores (consulta [Favoritos y barra lateral](07-favoriten-seitenleiste.md)).
- **Paneles**: una o varias listas de archivos una al lado de otra, cada una con pestañas y una barra de ruta (consulta [Paneles y pestañas](02-bereiche-tabs.md)).
- **Barra de estado** abajo: el número de ítems, la selección, las transferencias en curso, la carga del sistema (CPU, GPU, RAM; consulta [Carga del sistema](14-plugins.md#systemauslastung)) y una indicación sobre la paleta de comandos.

## La paleta de comandos {#befehlspalette}

Todos los comandos de Manta están en la **paleta de comandos**, también los de los plugins.

1. Pulsa **⇧⌘P** (o **⌘P**).
2. Escribe unas letras, por ejemplo `nueva ca` o `sftp`.
3. Elige con ↑ y ↓ y pulsa ↩.

La paleta muestra el atajo de cada comando a su lado, así que irás aprendiendo los atajos sobre la marcha.

![Paleta de comandos con una búsqueda](images/command-palette.png)

> [!NOTE]
> Manta solo tiene el menú **Manta** en la barra de menús (Acerca de Manta, Buscar actualizaciones…, Ajustes…, Salir de Manta). Todos los demás comandos están en la barra de herramientas, en el menú contextual (clic derecho) y en la paleta de comandos.

## Siguientes pasos

- [Paneles y pestañas](02-bereiche-tabs.md): trabajar con varios paneles.
- [Gestionar archivos](05-dateien.md): copiar, mover, renombrar, eliminar.
- [Atajos de teclado](18-tastenkuerzel.md): todos los atajos de un vistazo.
