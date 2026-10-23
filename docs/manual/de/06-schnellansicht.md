# Schnellansicht & Vorschau {#schnellansicht}

Mit der **Leertaste** siehst du dir eine Datei an, ohne sie zu öffnen. Für Dateien auf deinem Mac nutzt Manta Apples **Schnellansicht** wie der Finder; für Dateien auf Servern und in Archiven gibt es eine **integrierte Vorschau**.

## So siehst du dir Dateien schnell an {#leertaste}

1. Wähle eine Datei aus.
2. Drücke die **Leertaste** (oder wie im Finder **⌘Y**).
3. Mit **↑** und **↓** wechselst du zur vorherigen oder nächsten Datei; das Fenster folgt.
4. **Leertaste** oder **esc** schließt die Vorschau wieder.

Hast du mehrere Dateien ausgewählt, blätterst du in der Schnellansicht mit den Pfeilen **‹** und **›** durch die Auswahl.

Die Tastatur bleibt dabei in Manta: Du kannst weiter mit den Pfeiltasten durch die Liste gehen, während das Schnellansicht-Fenster offen ist.

![Schnellansicht über einem Bereich](images/quick-look.png)

## Schnellansicht (macOS) {#quick-look}

Die Schnellansicht ist Apples Vorschaufenster. Sie zeigt alles, was auch der Finder zeigt: PDFs, Bilder, Videos, Office-Dokumente, Textdateien und mehr. Sie funktioniert für **lokale** Dateien und Ordner.

## Integrierte Vorschau {#integrierte-vorschau}

Dateien auf **Servern** (SFTP, FTP, S3, Container) und **in Archiven** zeigt Manta in einem eigenen Vorschaufenster, weil Apples Schnellansicht nur lokale Dateien kennt.

- Die integrierte Vorschau zeigt **Textdateien** und **Bilder**.
- Bei großen Textdateien zeigt sie den Anfang („Es werden die ersten … der Datei angezeigt“).
- Für andere Dateitypen (z. B. PDFs oder Videos) sagt sie, was die Datei ist, und schlägt vor, sie mit ↩ zu öffnen. Manta lädt sie dann herunter und öffnet sie mit der Standard-App.
- Ordner zeigt sie nicht; öffne sie mit ↩. (Apples Schnellansicht zeigt lokale Ordner.)
- Schließen: **Leertaste** oder **⌘W** (es ist ein eigenes Fenster mit den üblichen Fenstertasten).

![Integrierte Vorschau einer Textdatei vom Server](images/preview-builtin.png)

## Die Leertaste umstellen {#einstellung}

Unter **Einstellungen** → **Allgemein** → **Vorschau** → **Leertaste öffnet** wählst du:

- **Schnellansicht (macOS)** – Standard.
- **Integrierte Vorschau** – auch für lokale Dateien.

Dateien auf Servern und in Archiven verwenden immer die integrierte Vorschau.

## Weitere Vorschauen durch Plugins {#plugins}

- **Markdown-Vorschau** (**⌥⌘M**): zeigt `.md`-Dateien schön gesetzt in einem Panel – siehe [Markdown-Vorschau](14-plugins.md#markdown).
- **Bildwerkzeuge**: zeigen Größe, Farbprofil und EXIF-Daten eines Bilds – siehe [Bildwerkzeuge](14-plugins.md#bildwerkzeuge).

## Grenzen {#grenzen}

- Die Schnellansicht gibt es nur für lokale Dateien. Für Server-Dateien lädt Manta (noch) keine Kopie, um sie in der Schnellansicht zu zeigen.
- Die integrierte Vorschau zeigt nur Text und Bilder.
