# Atajos de teclado {#tastenkuerzel}

Todos los atajos de Manta de un vistazo, agrupados por tema. La paleta de comandos (**⇧⌘P**) muestra el atajo de cada comando a su lado, también los de los plugins. La barra de menús también enumera todos los comandos con su atajo (**Archivo**, **Edición**, **Visualización**, **Ir**, **Ventana**), como Finder; así, las apps que muestran los atajos de la app activa encuentran todos los de Manta. Al hacer clic en un comando del menú, se ejecuta.

Símbolos: **⌘** Comando · **⇧** Mayúsculas · **⌥** Opción · **⌃** Control · **↩** Retorno · **⌫** Borrar (retroceso) · **⌦** Borrar hacia delante · **⇥** Tabulador

## General {#allgemein}

| Atajo | Comando |
|---|---|
| ⇧⌘P o ⌘P | Paleta de comandos |
| ⌘, | Ajustes… |
| ⌘? | Abrir el manual (este manual; en la ventana de Ayuda: el campo de búsqueda) |
| ⌘K | Conectarse al servidor… |
| ⌥⌘L | Mostrar transferencias |
| ⇧⌘S | Comparar y sincronizar… |
| ⌘H | Ocultar Manta |
| ⌥⌘H | Ocultar otros |
| ⌘Q | Salir de Manta |

## Paneles y pestañas {#bereiche}

| Atajo | Comando |
|---|---|
| ⇥ / ⇧⇥ | Cambiar de panel (al siguiente o, con ⇧, al anterior; después del último, vuelve al primero) |
| ⌥⌘← / ⌥⌘→ | Activar el panel de la izquierda / de la derecha |
| ⌘D | Dividir panel |
| ⇧⌘D | Cerrar panel |
| ⌘T | Nueva pestaña |
| ⌘W | Cerrar pestaña (en Ajustes, Comparar y sincronizar y la vista previa: cierra esa ventana) |

## Visualizaciones {#ansichten}

| Atajo | Comando |
|---|---|
| ⌘1 | Como iconos |
| ⌘2 | Como lista |
| ⌘3 | Como columnas |
| ⌘4 | Como galería |
| ⌘ + desplazamiento o pellizco | Cambiar el tamaño de los iconos (visualización Iconos) |
| ← / → | Visualización Columnas: ir a la columna de la izquierda / abrir la carpeta seleccionada como columna siguiente |
| ← / → | Visualizaciones Iconos y Galería: ítem anterior / siguiente |

Los atajos actúan sobre el panel activo. Consulta [Visualizaciones](03-ansichten.md).

## Navegación {#navigation}

| Atajo | Comando |
|---|---|
| ↩ o ⌘↓ | Abrir (archivos comprimidos como carpetas) |
| ⌥↩ | Abrir con la app predeterminada |
| ⌘↑ o ⌫ | Carpeta contenedora |
| ⌘[ o ⌘← | Atrás |
| ⌘] o ⌘→ | Adelante |
| ⌘L | Editar ruta |
| ⇧⌘G | Ir a la ruta… |
| ⌘R | Actualizar |
| ⇧⌘. | Mostrar/ocultar archivos ocultos |
| ⌘F | Filtrar archivos |
| ⌃⌘T | Añadir carpeta a Favoritos |

## Selección {#auswahl}

| Atajo | Efecto |
|---|---|
| ↑ / ↓ | Ítem anterior / siguiente |
| ⇧↑ / ⇧↓ | Ampliar la selección |
| Re Pág / Av Pág | 20 filas arriba / abajo |
| Inicio / Fin | Primer / último ítem |
| ⌘A | Seleccionar todo |
| esc | Anular la selección (o cerrar la vista previa) |
| Escribir letras | Saltar al nombre que coincida |

## Archivos {#dateien}

| Atajo | Comando |
|---|---|
| Barra espaciadora o ⌘Y | Vista Rápida / vista previa (la barra espaciadora o esc la cierran) |
| ⌘I | Obtener información |
| F5 | Copiar al otro panel… |
| F6 | Mover al otro panel… |
| F2 | Renombrar… (varios ítems: renombrar en lote) |
| ⇧⌘R | Renombrar en lote… |
| ⇧⌘N | Nueva carpeta… |
| ⌥⌘N | Nuevo archivo… |
| ⌘⌫ o ⌦ | Trasladar a la papelera (en servidores: eliminar definitivamente tras preguntar; consulta [Eliminar](05-dateien.md#loeschen)) |
| ⌥⌘⌫ | Eliminar inmediatamente… (definitivo, tras preguntar) |
| ⇧⌘⌫ | Vaciar papelera… (tras preguntar; consulta [Papelera](07-favoriten-seitenleiste.md#papierkorb)) |
| ⌘Z | Deshacer (trasladar a la papelera, renombrar, nueva carpeta, nuevo archivo; consulta [Deshacer](05-dateien.md#widerrufen)) |
| ⇧⌘Z | Rehacer |
| ⌥⌘C | Comprimir… |
| ⌥⌘E | Extraer aquí |
| ⌘ al arrastrar | Preseleccionar Mover en lugar de Copiar (sin tecla se preselecciona Copiar; ⌥ preselecciona Copiar de forma explícita) |

> [!NOTE]
> En muchos teclados de Mac tienes que mantener pulsada **fn** (🌐) para **F2**, **F5** y **F6**, salvo que hayas activado “Usar las teclas F1, F2, etc. como teclas de función estándar” en Ajustes del Sistema → **Teclado**.

## Plugins {#plugins}

| Atajo | Comando | Plugin |
|---|---|---|
| ⌃` | Mostrar/ocultar terminal | Terminal |
| ⇧⌘B | Mostrar/ocultar panel Git | Git |
| ⌘↩ | Commit (en el panel Git) | Git |
| ⌥⌘U | Mostrar uso del disco | Tamaños de carpeta |
| ⌥⌘M | Mostrar/ocultar vista previa de Markdown | Vista previa de Markdown |
| ⌥⌘R | Volver a ejecutar el último script | Info del proyecto |

Tus propias acciones rápidas pueden tener un atajo propio (el campo `shortcut` de `actions.json`).

## En diálogos y ventanas {#dialoge}

| Atajo | Efecto |
|---|---|
| ↩ | Confirmar (Copiar, Crear, Renombrar…) |
| esc | Cancelar el diálogo |
| ⌘W | Cerrar Ajustes, Comparar y sincronizar o la vista previa |

**Paleta de comandos**: ↑ ↓ para elegir · ↩ para ejecutar · esc para cerrar

**Ir a la ruta**: ↩ abre en una pestaña nueva · ⌘↩ abre en esta pestaña · ⇥ completa · esc cancela

## Cuando los atajos no funcionan {#fokus}

- En un **campo de texto** (filtro, ruta, campos de diálogo) solo funcionan **⇧⌘P**, **⇧⌘G**, **⌘,** y **⌘?**; todas las demás teclas pertenecen al campo.
- En el **terminal**, **⌘C**, **⌘V**, **⌘A**, **⌘K**, **⌘←**, **⌘→**, **⌘⌫** y **⌘Z** pertenecen al terminal. Haz clic en la lista de archivos para devolver el teclado a Manta.
- En las ventanas **Ajustes**, **Comparar y sincronizar** y **vista previa** solo funcionan sus propios atajos.

## Uso solo con teclado {#tastatur}

Puedes usar Manta por completo con el teclado.

| Tecla | Qué hace |
|---|---|
| ⌃F5 | Llevar el foco del teclado a los controles, empezando por la barra de herramientas (como «Mover el foco a la barra de herramientas» de macOS) |
| ⇥ / ⇧⇥ | En los controles: siguiente / anterior – barra de herramientas → barra lateral → paneles (barra de ruta, encabezados de columna, lista de archivos) → barra de estado, y vuelta a empezar |
| ↑ ↓ ← → | En la barra lateral, los diálogos y las listas de opciones: al siguiente control en esa dirección |
| Espacio / ↩ | Pulsar el botón, la opción o el interruptor con el foco |
| esc | Salir de los controles: la lista de archivos vuelve a tener el teclado; en diálogos: cancelar |
| ⇥ / ⇧⇥ en la lista de archivos | Al panel siguiente / anterior |

- La **lista de archivos** es una sola parada de tabulación por panel (no cada fila). Mientras tiene el foco funcionan todos sus atajos: flechas, ↩, Espacio, escribir para saltar.
- Un **anillo de foco** en el color de resalte muestra qué control tiene el teclado. Los clics del ratón no lo muestran.
- Los **diálogos** retienen el foco: ⇥ solo recorre sus campos y botones. Al cerrarlos (también Ajustes), el foco vuelve a donde estaba.
- Con la **navegación con el teclado** activada en Ajustes del Sistema (Teclado) todo funciona igual; ⌃F5 también es allí «Mover el foco a la barra de herramientas».

## Diferencias con Finder {#finder}

| Atajo | Finder | Manta |
|---|---|---|
| ⌘D | Duplicar | Dividir panel |
| ⇧⌘D | Abrir el Escritorio | Cerrar panel |
| ⇧⌘P | Mostrar vista previa | Paleta de comandos |
| ⌘R | Mostrar original | Actualizar |
| ⌥⌘C | Copiar ruta de acceso | Comprimir… |
| ⌥⌘L | Abrir Descargas | Mostrar transferencias |
| ⌫ | – | Carpeta contenedora |

Igual que en Finder: **⌘⌫** traslada a la papelera, **⌥⌘⌫** elimina inmediatamente, **⇧⌘⌫** vacía la papelera, **⌘T** abre una pestaña nueva, **⌘Y** abre Vista Rápida, **⇧⌘N**, **⇧⌘G**, **⇧⌘.**, **⌘[** / **⌘]**, **⌘↑** / **⌘↓**, **⌘1** – **⌘4**, **⌘K** y **⌃⌘T** (Añadir a la barra lateral/Favoritos). Al arrastrar se preselecciona Copiar, también en el mismo volumen (Finder ahí mueve); **⌘** preselecciona Mover.

> [!NOTE]
> **Cambios en esta versión:** **⌘T** abre una pestaña nueva (antes: **⇧⌘T**), Transferencias ha pasado a **⌥⌘L** (antes: **⌘T**) y **⌘⌫** traslada los ítems locales a la papelera (antes: eliminación definitiva; ahora eso es **⌥⌘⌫**). Manta lo muestra una vez después de la actualización.
