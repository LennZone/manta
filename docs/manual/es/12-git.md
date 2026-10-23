# Git {#git}

Cuando una carpeta está en un repositorio Git, Manta muestra el estado de cada archivo directamente en la lista, la rama en la barra de ruta y un panel Git para preparar cambios y hacer commits. `git` debe estar instalado en tu Mac (por ejemplo, con las Command Line Tools de Xcode).

![Lista de archivos con insignias de Git y el panel Git](images/git-panel.png)

## Estado en la lista de archivos {#abzeichen}

Los archivos modificados tienen una insignia. Pasa el puntero por encima y una descripción emergente explica el estado:

- **Modificado, no preparado** / **Preparado para el commit** / **Modificado, preparado en parte**
- **Añadido, preparado para el commit**
- **Sin seguimiento** (nuevo; Git todavía no conoce el archivo)
- **Eliminado, no preparado** / **Eliminación preparada para el commit**
- **Renombrado desde…** / **Copiado desde…**
- **Conflicto de fusión**
- **Ignorado por .gitignore**: estos archivos aparecen atenuados.

Las carpetas indican cuántos archivos modificados contienen (“3 archivos modificados dentro”).

## La rama en la barra de ruta {#chip}

Una etiqueta en la barra de ruta muestra la rama actual. Su descripción emergente te da más detalles:

- a qué rama upstream sigue y si vas por delante o por detrás (“Sigue a origin/main (2 por delante)”),
- el último commit,
- “HEAD desacoplado en…” cuando no hay ninguna rama activa.

Si hay en curso una fusión, un rebase, un cherry-pick, un revert o un bisect, la etiqueta lo indica. Al hacer clic en la etiqueta se abre el panel Git.

## El panel Git {#seitenleiste}

Para mostrarlo u ocultarlo: **⇧⌘B** (**Mostrar/ocultar panel Git**) o haz clic en la etiqueta de la rama.

El panel muestra:

- arriba, la rama, la rama upstream y cuántos commits hay para hacer push o pull,
- los grupos **Conflictos**, **Preparados**, **Cambios** y **Sin seguimiento**,
- un campo para el **Mensaje del commit**,
- los **Commits recientes**.

### Hacer commit de cambios {#commit}

1. Abre el panel Git con **⇧⌘B**.
2. Prepara archivos: **Preparar** en un archivo, o **Preparar todo** en el grupo **Cambios**.
3. Haz clic en un archivo para ver su diff.
4. Escribe un **Mensaje del commit**.
5. Haz clic en **Commit** o pulsa **⌘↩**.

Manta informa: “Commit abc1234 creado: …”.

Para deshacer la preparación: **Quitar** o **Quitar todo**.

### Resolver conflictos {#konflikte}

Durante un conflicto de fusión, los archivos afectados aparecen en **Conflictos**. Resuelve el conflicto en tu editor y después elige **Marcar como resuelto (preparar)**. Cuando todos están resueltos, el panel indica “Haz commit para terminar la fusión”. Para rebase, cherry-pick y revert, muestra el comando `--continue` correspondiente para el terminal.

## Actualizar {#aktualisieren}

Manta actualiza el estado de Git automáticamente. Si hace falta: **Actualizar estado de Git** en la paleta de comandos, o el botón de actualizar del panel.

## Limitaciones {#grenzen}

- Git solo funciona con carpetas **locales** (“El estado de Git está disponible para carpetas de este Mac, no en servidores.”).
- Push, pull, cambiar de rama y fusionar se hacen en el [Terminal](11-terminal.md) o en una app de Git. El panel muestra el estado, prepara cambios y hace commits.
