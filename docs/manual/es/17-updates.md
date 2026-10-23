# Actualizaciones {#updates}

Manta busca discretamente una versión nueva en cada arranque y solo la instala si estás de acuerdo. Este capítulo explica cómo funciona, qué se envía por la red y qué hacer si una actualización falla.

## Cómo te enteras de una actualización {#hinweis}

Cuando hay una versión nueva, ves:

- un aviso “Actualización disponible: 0.2.0” con un botón **Mostrar**,
- una pequeña indicación en la barra de estado (“Actualización 0.2.0”).

No aparece nada encima de tu trabajo. Si Manta no tiene conexión, no notarás nada; el fallo solo se anota en el registro.

## Buscar actualizaciones tú mismo {#suchen}

- **Manta** → **Buscar actualizaciones…** en la barra de menús, o
- **Buscar actualizaciones…** en la paleta de comandos, o
- **Ajustes** → **General** → **Actualizaciones** → **Buscar ahora**.

Si todo está al día, Manta indica “Manta está actualizado”.

## Instalar una actualización {#installieren}

1. Haz clic en **Mostrar** en el aviso (o en la indicación de la barra de estado). Se abre la ventana **Actualización de software**.
2. Lee las **Novedades**. El tamaño y la fecha de publicación aparecen encima.
3. Elige:
   - **Instalar y reiniciar**: descarga la actualización, la verifica, sustituye la app y reinicia Manta.
   - **Más tarde**: te lo vuelve a recordar en el siguiente arranque.
   - **Omitir esta versión**: esta versión y las anteriores ya no se ofrecen.

![La ventana Actualización de software](images/software-update.png)

Durante la descarga ves el progreso y puedes cancelarla. Si todavía hay transferencias en curso, Manta espera a que terminen antes de reiniciarse. Es posible que macOS te pida la contraseña si Manta no puede escribir en la carpeta **Aplicaciones**.

Si algo falla, la ventana muestra el motivo, con **Reintentar**. Tu versión actual no cambia.

## Cuando Manta no puede actualizarse {#blockiert}

| Mensaje | Solución |
|---|---|
| “Manta se ejecuta desde una imagen de disco” | Arrastra Manta a la carpeta **Aplicaciones** y ábrelo desde allí. |
| “Manta se ejecuta desde una ubicación temporal” | macOS ha abierto Manta desde una copia de solo lectura (App Translocation). Traslada Manta a la carpeta **Aplicaciones** con Finder y ábrelo desde allí. |
| “Las actualizaciones no están configuradas en esta versión” | Tu versión no puede verificar actualizaciones. Descarga las versiones nuevas del sitio web. |

## Canales: Estable y Beta {#kanaele}

En **Ajustes** → **General** → **Actualizaciones** → **Canal**, elige:

- **Estable** (por omisión): solo versiones finales.
- **Beta**: también versiones preliminares. Ofrecen funciones nuevas antes, pero pueden contener errores.

Manta nunca ofrece una versión anterior a la instalada.

## Seguridad {#sicherheit}

Cada actualización está firmada digitalmente. Manta solo instala lo que está firmado con la clave del equipo de Manta y coincide exactamente con la versión anunciada. Una descarga manipulada se rechaza (“La actualización no está firmada correctamente, así que no se ha instalado nada.”). Las actualizaciones solo llegan por HTTPS.

## Privacidad: qué envía la búsqueda de actualizaciones {#datenschutz}

Para comprobarlo, Manta descarga dos archivos pequeños del servidor de actualizaciones (`manta-filemanager.com`): la lista de versiones y su firma. Lo que se envía por la red:

- tu **dirección IP** (como en cualquier solicitud web),
- el identificador **`Manta-Updater/<version>`**, es decir, qué versión de Manta usas.

Manta **no** envía ningún ID de usuario, identificador del dispositivo, nombre de archivo ni dato de uso. Qué versión te conviene (canal, versión de macOS, versión omitida) lo decide Manta en tu Mac, no el servidor.

Si no quieres esto, desactiva **Buscar actualizaciones al abrir**. Manta solo se conectará entonces para buscar actualizaciones cuando elijas tú **Buscar actualizaciones…**.

Como casi cualquier sitio web, el servidor registra estas solicitudes con la dirección IP. El proveedor de alojamiento lo hace según sus propias normas de conservación; Manta en sí no analiza las solicitudes ni las vincula con nada.
