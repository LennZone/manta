# Comparar y sincronizar {#vergleichen}

**Comparar y sincronizar** muestra en qué se diferencian dos carpetas y, si quieres, las iguala: en tu Mac, entre tu Mac y un servidor, o entre dos servidores. La ventana compara dos paneles.

![La ventana Comparar y sincronizar](images/compare-sync.png)

## Comparar dos carpetas {#zwei-bereiche}

1. Abre una carpeta en el panel izquierdo y la otra en el panel derecho.
2. Pulsa **⇧⌘S** (**Comparar y sincronizar…**) o haz clic en su botón de la barra de herramientas.
3. Elige las opciones (consulta más abajo) y haz clic en **Volver a comparar**.

La lista muestra cada ítem con su estado:

| Estado | Significado |
|---|---|
| **Solo a la izquierda** / **Solo a la derecha** | Existe solo en un lado. |
| **Izquierda más reciente** / **Derecha más reciente** | En ambos lados, pero uno es más reciente (fecha de modificación). |
| **Diferentes** | Misma fecha, pero distinto tamaño. |
| **Idénticos** | Misma fecha y mismo tamaño. |

Manta compara la **fecha de modificación y el tamaño**, no el contenido byte a byte.

La leyenda de estados que hay sobre la lista también es un filtro: haz clic en un estado para ocultarlo (“Haz clic para ocultar”) y vuelve a hacer clic para mostrarlo.

## Con tres o más paneles {#a-b}

Con más de dos paneles, eliges cuáles dos comparar:

- En lugar de “Izquierda” y “Derecha”, los dos lados de la ventana se llaman **Panel A** y **Panel B**. En la esquina superior derecha de cada tarjeta, un menú desplegable elige qué panel es (“Elige qué panel comparar”). Los paneles se numeran de izquierda a derecha, por ejemplo “2 · Descargas”; el panel del otro lado está marcado con “(A)” o “(B)”. Si lo eliges, se intercambian A y B.
- El panel activo y su [panel de destino](02-bereiche-tabs.md#zielbereich) vienen preseleccionados.
- Mientras la ventana está abierta, aparecen pequeños botones **A**/**B** en la barra de ruta de cada panel (“Comparar este panel como A”). El papel elegido aparece relleno de color.

Los estados se llaman entonces **Solo en A**, **Solo en B**, **A más reciente** y **B más reciente**, y las columnas de tamaño, **Panel A** y **Panel B**.

![Comparación con tres paneles: elegir Panel A y Panel B](images/compare-sync-ab.png)

**A está siempre a la izquierda de la ventana y B, a la derecha.** Así, la dirección y las acciones mantienen su nombre: **Reflejar de izquierda a derecha** significa “de A a B”, “Copiar →” copia de A a B y “Eliminar a la derecha” elimina en B.

## Opciones {#optionen}

- **Incluir subcarpetas**: compara también el contenido de todas las subcarpetas.
- **Archivos ocultos**: incluye archivos como `.env` o `.DS_Store`.
- **Dirección** (los tres botones → ↔ ←):
  - **→ Reflejar de izquierda a derecha**: el lado derecho queda igual que el izquierdo.
  - **↔ Gana el archivo más reciente**: en ambos sentidos; se transfiere el archivo más reciente.
  - **← Reflejar de derecha a izquierda**: el lado izquierdo queda igual que el derecho.
- **Eliminar archivos sobrantes en el destino**: solo para sincronización en un sentido. Elimina en el destino lo que no existe en el origen.

Antes de empezar, la columna **Acción** muestra lo que va a ocurrir: “Copiar →”, “← Copiar”, “Eliminar a la derecha” o “Eliminar a la izquierda”.

## Sincronizar {#synchronisieren}

1. Compara las carpetas como se indica arriba.
2. Elige la dirección y revisa la columna **Acción**.
3. Haz clic en **Sincronizar N ítems**.

Las transferencias se realizan en segundo plano (“Sincronización iniciada: 12 transferencias en cola”). Sigue el progreso en **Transferencias** (**⌥⌘L**).

Si las carpetas coinciden, la ventana indica “Ambas carpetas están sincronizadas”.

> [!WARNING]
> Sincronizar **sobrescribe** archivos en el destino sin volver a preguntar, y **Eliminar archivos sobrantes en el destino** elimina definitivamente (consulta [Eliminar](05-dateien.md#loeschen)). Revisa la columna **Acción** antes de empezar.

Reglas que te protegen:

- Al reflejar, nunca se sobrescribe un archivo **más reciente** en el destino.
- Con **Gana el archivo más reciente**, los archivos con la misma fecha pero distinto tamaño se omiten, porque no está claro cuál es el correcto. **Eliminar archivos sobrantes en el destino** no se aplica aquí.
- Si un nombre es una carpeta en un lado y un archivo en el otro, se omite.
- Si no se puede leer una carpeta, la comparación se detiene en lugar de eliminar lo que no debe a partir de datos incompletos.

## La ventana {#fenster}

**Comparar y sincronizar** es una ventana de macOS independiente. Ciérrala con el botón rojo de la ventana o con **⌘W** (no con esc). Manta recuerda su posición mientras la app está abierta.

Con un solo panel, el comando está desactivado (“Se necesitan dos paneles – ⌘D divide el panel”).
