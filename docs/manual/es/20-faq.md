# Preguntas frecuentes {#faq}

Respuestas breves a preguntas frecuentes. Los capítulos enlazados tienen los detalles.

## General

### ¿Sustituye Manta a Finder? {#finder-ersetzen}

No del todo. Puedes convertir Manta en tu gestor de archivos predeterminado; entonces “Mostrar en Finder” y (hasta macOS 15) las carpetas de otras apps se abren en Manta. Pero el escritorio, el icono de Finder en el Dock y los dobles clics dentro de ventanas de Finder se quedan siempre con Finder: macOS no permite que ninguna app los asuma. Consulta [Gestor de archivos predeterminado](01-erste-schritte.md#standard-dateimanager).

### ¿Cuesta algo Manta o recopila datos? {#daten}

Manta no recopila datos de uso. Solo se conecta a internet para tus propias conexiones y para buscar actualizaciones, y en ese caso solo envía tu dirección IP y la versión de Manta. Consulta [Privacidad](17-updates.md#datenschutz).

Manta es gratuito y de código abierto bajo la licencia MIT: puedes usarlo, modificarlo y compartirlo libremente. El código fuente está en [GitHub](https://github.com/web-workers-com/manta) (en la app, en **Ayuda** → **Código fuente en GitHub…**) y las contribuciones son bienvenidas. En el sitio web [manta-filemanager.com](https://manta-filemanager.com/) encontrarás novedades, ayuda y contacto; en la app, en el menú **Ayuda** → **Sitio web y soporte de Manta…** y en Ajustes, en [Acerca de Manta](16-einstellungen.md#ueber).

Manta se basa en componentes de código abierto. Sus avisos de licencia están en **Ayuda** → **Licencias de terceros** o [aquí](../../third-party-licenses.md).

### He encontrado un error o tengo una idea: ¿dónde lo comunico? {#feedback}

En GitHub: **Ayuda** → **Informar de un problema…** abre el formulario de informe de errores con las versiones de Manta y macOS ya rellenadas; **Ayuda** → **Sugerir una idea…** abre el formulario de ideas. Qué debe incluir un buen informe de errores y dónde está el archivo de registro: consulta [Informar de un error](19-fehlerbehebung.md#fehler-melden). Informa de las vulnerabilidades de seguridad de forma privada, nunca en un issue público; consulta la [política de seguridad](https://github.com/web-workers-com/manta/blob/main/SECURITY.md).

### ¿Está Manta disponible en español? {#sprache}

Sí. **Ajustes** → **General** → **Idioma**: **Sistema** (por omisión, sigue tus idiomas preferidos de macOS) o uno de los siete idiomas de la app: **Deutsch**, **English**, **简体中文**, **Español**, **Français**, **Italiano**, **Русский**. El cambio se aplica al instante. El manual (**⌘?**) también sigue el idioma de la app; si no existe en ese idioma, Manta muestra la versión en inglés.

### ¿Cuántos paneles puedo abrir? {#bereiche}

Tantos como quepan en tu ventana: cada uno necesita unos 280 puntos de anchura. Siempre queda al menos uno abierto. Consulta [Paneles y pestañas](02-bereiche-tabs.md).

## Archivos

### ¿Dónde está la papelera? ¿Puedo recuperar archivos eliminados? {#papierkorb}

Sí, en el caso de los ítems de tu Mac: **⌘⌫** los traslada a la papelera de macOS, como Finder, y la opción **Devolver** de Finder los restaura. **⌥⌘⌫** (**Eliminar inmediatamente…**) elimina definitivamente tras preguntar. Los servidores, S3 y los contenedores no tienen papelera: allí eliminar es siempre definitivo, y Manta pregunta antes. Consulta [Eliminar](05-dateien.md#loeschen).

### ¿Arrastrar copia o mueve? {#ziehen}

Manta pregunta, con **Copiar** preseleccionado, también en el mismo volumen, donde Finder movería. Así nada desaparece de su carpeta por accidente. Mantén pulsada **⌘** al soltar para preseleccionar **Mover**; **⌥** preselecciona **Copiar**. La insignia junto al puntero te indica de antemano qué está preseleccionado. En [Ajustes](16-einstellungen.md#kopieren-bewegen) puedes elegir en su lugar “Copiar siempre” o “Mover siempre” (sin diálogo). Lo que se suelta en la papelera siempre se traslada a la papelera. Consulta [Arrastrar y soltar](05-dateien.md#drag-drop).

### He abierto un archivo de un servidor y lo he modificado. ¿Dónde está mi cambio? {#server-datei}

En una copia temporal en tu Mac. Manta no sube automáticamente los archivos modificados. Copia el archivo a una carpeta local, edítalo ahí y vuelve a copiarlo con **F5**. Consulta [Navegación](04-navigation.md#oeffnen).

### ¿Por qué no veo mis archivos `.env` o `.git`? {#unsichtbar}

Los archivos que empiezan por un punto están ocultos. Pulsa **⇧⌘.**. Consulta [Archivos ocultos](04-navigation.md#unsichtbare-dateien).

### ¿Por qué no se actualiza una carpeta cuando otra app añade archivos? {#aktualisieren}

Pulsa **⌘R**. Manta vuelve a leer las carpetas después de sus propias acciones, pero no cuando otras apps cambian algo.

### ¿Puedo modificar archivos dentro de archivos ZIP? {#archiv-aendern}

No, los archivos comprimidos son de solo lectura. Extráelos, modifica los archivos y vuelve a comprimirlos (**⌥⌘C**). Consulta [Archivos comprimidos](10-archive.md).

### ¿Qué pasa con una copia si se cae la Wi-Fi o cierro Manta? {#unterbrochen}

No se pierde nada. Cuando se cae la red, la transferencia espera y continúa en el mismo punto en cuanto vuelve la conexión. Cuando cierras Manta (o falla), en el siguiente arranque pregunta “¿Reanudar las transferencias interrumpidas?”. Consulta [Pausar y reanudar transferencias](05-dateien.md#pausieren).

### ¿Qué son los archivos `.NOMBRE.manta-part`? {#manta-part}

Archivos parciales ocultos en los que Manta escribe mientras copia. Solo el archivo terminado recibe su nombre real. Consulta [Archivos parciales](05-dateien.md#teildateien).

## Conexiones y nube

### ¿Se guardan de forma segura mis contraseñas de servidor? {#passwoerter}

Sí. Con **Recordar contraseña**, Manta guarda las contraseñas, frases de contraseña y claves secretas de S3 en el Llavero de macOS, no en un archivo. Las contraseñas que las versiones anteriores guardaban sin cifrar se trasladan ahí al arrancar. Sin **Recordar contraseña**, Manta pregunta cada vez. SFTP con ssh-agent no necesita ninguna contraseña guardada. Consulta [Conexiones](08-verbindungen.md#favoriten).

### ¿Puedo iniciar sesión directamente en Google Drive? {#google}

No. Instala **Google Drive para ordenadores** e inicia sesión ahí; Manta muestra entonces la carpeta de Google Drive. Consulta [Almacenamiento en la nube](09-cloud.md#google-drive).

### ¿Admite Manta WebDAV, SMB o Dropbox sin la app? {#protokolle}

Manta se conecta directamente a SFTP, FTP, FTPS, S3 y contenedores. Monta los recursos compartidos SMB y WebDAV en Finder (**Ir** → **Conectarse al servidor**); después aparecen en **Dispositivos**. Manta muestra Dropbox, OneDrive y similares a través de las apps de los proveedores.

### ¿Por qué hay un escudo de advertencia en la etiqueta FTP? {#ftp-warnung}

Tu red bloquea las conexiones de datos FTP cifradas. Por eso Manta transfiere los archivos sin cifrar; el inicio de sesión sigue cifrado. Consulta [Canal de datos](08-verbindungen.md#datenkanal).

### ¿Puede Manta iniciar o detener contenedores? {#container}

No. Manta solo muestra y modifica archivos en los contenedores. Inicia y detén los contenedores con Docker Desktop o en el terminal.

## Ampliar Manta

### ¿Puedo añadir mis propios comandos sin escribir un plugin? {#eigene-befehle}

Sí, con las **Acciones rápidas**: basta una entrada con un comando de shell en `actions.json`. Consulta [Acciones rápidas](14-plugins.md#actions-json).

### ¿Son seguros los plugins? {#plugins-sicher}

Los plugins integrados forman parte de Manta. Los plugins externos se ejecutan con los permisos de tu usuario y sin sandbox: instala solo lo que sea de confianza. Consulta [Tus propios plugins](15-eigene-plugins.md#sicherheit).
