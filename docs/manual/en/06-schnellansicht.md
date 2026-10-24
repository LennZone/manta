# Quick Look & Preview {#schnellansicht}

Press **Space** to look at a file without opening it. For files on your Mac Manta uses Apple's **Quick Look**, like Finder; for files on servers and in archives there is a **built-in preview**.

## Look at files quickly {#leertaste}

1. Select a file.
2. Press **Space** (or **⌘Y**, as in Finder).
3. Use **↑** and **↓** to go to the previous or next file; the panel follows.
4. **Space** or **esc** closes the preview.

With several files selected, the **‹** and **›** arrows in Quick Look page through the selection.

The keyboard stays in Manta: you can keep moving through the list with the arrow keys while the Quick Look panel is open.

## Quick Look (macOS) {#quick-look}

Quick Look is Apple's preview panel. It shows everything Finder shows: PDFs, images, videos, Office documents, text files and more. It works for **local** files and folders.

## Built-in preview {#integrierte-vorschau}

Files on **servers** (SFTP, FTP, S3, containers) and **in archives** open in Manta's own preview window, because Apple's Quick Look only knows local files.

- The built-in preview shows **text files** and **images**.
- For large text files it shows the beginning ("Showing the first … of the file.").
- For other file types (PDFs or videos, for example) it says what the file is and suggests opening it with ↩. Manta then downloads it and opens it with the default app.
- It doesn't show folders; press ↩ to open one. (Apple's Quick Look shows local folders.)
- To close it: **Space** or **⌘W** (it's a window of its own with the usual window buttons).

![Built-in preview of a text file from a server](images/preview-builtin.png)

## Change what Space does {#einstellung}

In **Settings** → **General** → **Preview** → **Space opens** choose:

- **Quick Look (macOS)** – the default.
- **Built-in preview** – for local files too.

Files on servers and in archives always use the built-in preview.

## More previews from plugins {#plugins}

- **Markdown Preview** (**⌥⌘M**): shows `.md` files nicely typeset in a sidebar – see [Markdown Preview](14-plugins.md#markdown).
- **Image Tools**: show an image's size, colour profile and EXIF data – see [Image Tools](14-plugins.md#bildwerkzeuge).

## Limitations {#grenzen}

- Quick Look is only for local files. Manta doesn't (yet) download server files to show them in Quick Look.
- The built-in preview shows only text and images.
