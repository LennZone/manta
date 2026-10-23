# Archivos comprimidos {#archive}

Manta abre los archivos comprimidos ZIP y TAR como si fueran carpetas: ves el contenido al instante, sin extraerlo antes. Este capítulo también explica cómo extraer archivos comprimidos y crear otros nuevos.

## Formatos admitidos {#formate}

- `.zip`
- `.tar`, `.tar.gz` / `.tgz`, `.tar.bz2`, `.tar.xz`, `.tar.zst`
- archivos comprimidos individuales: `.gz`, `.bz2`, `.xz`, `.zst`

## Abrir un archivo comprimido como carpeta {#oeffnen}

1. Selecciona el archivo comprimido.
2. Pulsa **↩** o haz doble clic en él.

La barra de ruta muestra el archivo comprimido como una carpeta, por ejemplo `Descargas › backup.zip › docs`. Una etiqueta con un candado marca el panel como **archivo comprimido de solo lectura**.

- **⌫** o **⌘↑** en la carpeta superior del archivo comprimido vuelve a la carpeta en la que está el archivo, con este seleccionado.
- **Atrás** y **Adelante** funcionan dentro y fuera del archivo comprimido.

![Un archivo ZIP abierto como carpeta](images/archive-as-folder.png)

Otras opciones:

- **⌥↩** (**Abrir con la app predeterminada**) abre el archivo comprimido con su app predeterminada (la Utilidad de Archivos de macOS, por ejemplo).
- Los documentos que por dentro son archivos ZIP (`.docx`, `.xlsx`, `.jar`, `.apk`…) se siguen abriendo en su app. Clic derecho → **Explorar archivo comprimido** muestra su contenido.
- Si no quieres que los archivos comprimidos se abran nunca como carpetas, desactiva **Ajustes** → **General** → **Abrir archivos comprimidos como carpetas**.

Los archivos comprimidos de servidores y los que están dentro de otros archivos comprimidos se descargan primero a un archivo temporal (con progreso y **Cancelar** en el panel). Los archivos comprimidos grandes se leen en segundo plano.

## Qué funciona en un archivo comprimido y qué no {#schreibgeschuetzt}

Los archivos comprimidos son de **solo lectura**. Manta rechaza, con una indicación, crear carpetas, renombrar, eliminar, mover y soltar archivos dentro del archivo comprimido.

**Copiar hacia fuera** funciona siempre:

- con **F5** al otro panel,
- arrastrando a otro panel,
- con **Extraer aquí** o **Extraer al otro panel**.

## Extraer un archivo comprimido {#entpacken}

**Un archivo comprimido completo:**

1. Selecciona el archivo comprimido.
2. Pulsa **⌥⌘E** (**Extraer aquí**), o haz clic derecho → **Extraer aquí** o **Extraer al otro panel**.

**Extraer aquí** descomprime junto al archivo comprimido: en una carpeta con el nombre del archivo o directamente, si el archivo contiene un solo ítem.

**Ítems sueltos de un archivo comprimido abierto:**

1. Abre el archivo comprimido como carpeta y selecciona los ítems.
2. **⌥⌘E** los extrae en la carpeta que contiene el archivo comprimido. **F5** o clic derecho → **Extraer al otro panel…** los extrae en el otro panel.

La extracción se realiza como transferencia en segundo plano (consulta [Transferencias](05-dateien.md#uebertragungen)).

## Crear un archivo comprimido {#komprimieren}

1. Selecciona archivos y carpetas.
2. Pulsa **⌥⌘C** (**Comprimir…**) o elígelo en el menú contextual.
3. Elige el **Formato**: zip, tar.gz, tar.xz o tar.zst.
4. Comprueba el **Nombre**. Manta sugiere uno, como “site.zip” o “Archivo 2.zip”.
5. Con el regulador **Nivel**, elige entre **Más rápido** y **Más pequeño**.
6. **Excluir .DS_Store y __MACOSX** está activado, para que no acaben archivos auxiliares de Mac en el archivo comprimido.
7. Haz clic en **Comprimir**.

El archivo comprimido se crea en la carpeta actual. Esto también funciona en servidores: Manta crea el archivo comprimido en un archivo temporal local y lo sube.

![Hoja Comprimir](images/compress-sheet.png)

> [!TIP]
> Para usuarios de Mac, **zip** es lo más cómodo. **tar.zst** es muy rápido y pequeño, pero requiere una herramienta compatible en el otro extremo.

## Seguridad {#sicherheit}

- Las entradas con `..` o rutas absolutas podrían escribir fuera de la carpeta de destino al extraerlas. Manta nunca las muestra ni las extrae, e informa con “Se han omitido N entradas no seguras”.
- Las entradas ZIP cifradas tienen una insignia de candado. Manta no puede abrirlas ni extraerlas y no pide contraseña.

## Limitaciones {#grenzen}

- No se pueden hacer cambios dentro de archivos comprimidos (añadir, eliminar, renombrar).
- No se admiten entradas ZIP protegidas con contraseña.
- Los enlaces simbólicos se muestran como enlaces; al extraer se copia el archivo de destino, y los enlaces a carpetas se omiten.
- Los TAR comprimidos de más de 2 GB son más lentos, porque Manta los vuelve a leer desde el principio para cada archivo.
- No se admiten los archivos ZIP divididos ni los archivos dispersos (sparse) de TAR.
