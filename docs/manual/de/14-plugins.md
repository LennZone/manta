# Eingebaute Plugins {#plugins}

Viele Funktionen von Manta stecken in eingebauten Plugins. Sie sind von Anfang an eingeschaltet, und du kannst jedes unter **Einstellungen** → **Plugins** ausschalten. Git und Terminal haben eigene Kapitel: [Git](12-git.md), [Terminal](11-terminal.md).

## Plugins ein- und ausschalten {#verwalten}

1. Öffne **Einstellungen** mit **⌘,** und wähle **Plugins**.
2. Unter **Integriert** stehen die eingebauten Plugins. Klicke auf **Ausschalten** bzw. **Einschalten**.
3. Hat ein Plugin Optionen, öffnest du sie mit **Einstellungen** auf seiner Karte.

Plugins von anderen installierst du über **Entdecken** oben auf der Plugins-Seite, per Doppelklick auf eine `.mantaplugin`-Datei, indem du sie auf die Plugins-Seite ziehst, oder mit **Plugin installieren …** – aus einer `.mantaplugin`-Datei oder einem Ordner. Plugins von anderen laufen in einer macOS-Sandbox und können nur, was ihre Berechtigungen erlauben; eingebaute Plugins sind Teil von Manta und laufen ohne Sandbox. Mehr dazu: [Eigene Plugins](15-eigene-plugins.md#installieren) und [Sicherheit](15-eigene-plugins.md#sicherheit).

Viele Plugins zeigen ihre Inhalte in einem **Panel rechts** oder in einem **Panel unter dem Bereich**. Du öffnest sie über Tastenkürzel, Chips in der Pfadleiste oder die Befehlspalette („Panel „…“ einblenden“).

![Einstellungen → Plugins](images/settings-plugins.png)

## Ordnergrößen {#ordnergroessen}

**Ordnergrößen** zeigt in der Spalte **Größe** auch für Ordner die echte Größe – im Hintergrund gemessen und zwischengespeichert. Die Größen bleiben auch nach einem Neustart erhalten: Manta zeigt sie sofort und misst im Hintergrund nach, ob sich etwas geändert hat.

- Solange Manta noch zählt, steht dort z. B. „≥ 1,2 GB“ mit einem kleinen Kreisel.
- Fährst du über eine Größe, siehst du die Anzahl der Dateien und den Platz auf dem Volume.
- Nach Ordnergrößen kannst du sortieren, und sie zählen bei der Größe der Auswahl mit.

Die Größe ist die Summe der Dateigrößen (wie bei Dateien). Manta folgt keinen symbolischen Links, zählt feste Links einmal und bleibt auf dem Volume des Ordners.

**Nur bei Bedarf**: Wenn du unter **Einstellungen** → **Plugins** → **Ordnergrößen** auf **Nur bei Bedarf** umstellst, zeigen Ordner „—“, bis du sie mit der rechten Maustaste anklickst und **Größe berechnen** wählst.

### Speicherbelegung {#speicherbelegung}

**⌥⌘U** (**Speicherbelegung einblenden**) zeigt, was im aktiven Ordner Platz belegt:

- eine **Kachelgrafik** (Treemap): Klicken zum Vergrößern, Doppelklicken zum Öffnen,
- eine Liste der **größten Objekte**.

![Speicherbelegung mit Treemap](images/disk-usage.png)

Ordnergrößen und Speicherbelegung gibt es nur für lokale Ordner.

## Stapelweise umbenennen {#umbenennen}

Benenne viele Dateien auf einmal um – mit Regeln, Live-Vorschau und Widerrufen.

1. Wähle die Dateien aus. Ist nichts ausgewählt, gilt es für alle Dateien im Ordner.
2. Drücke **⇧⌘R** (**Stapelweise umbenennen …**) oder **F2** bei mehreren ausgewählten Objekten.
3. Füge mit **Regel hinzufügen** Regeln hinzu. Sie gelten von oben nach unten.
4. Prüfe die **Vorschau**: bisheriger Name → neuer Name, Änderungen sind Zeichen für Zeichen markiert.
5. Klicke auf **Umbenennen** (oder drücke ↩).

![Stapelweise umbenennen mit Regeln und Vorschau](images/batch-rename.png)

Die Regeln:

| Regel | Was sie tut |
|---|---|
| **Suchen & Ersetzen** | Text oder regulärer Ausdruck, mit `$1`-Gruppen; Groß/klein beachten; erstes oder alle Vorkommen. |
| **Text einfügen** | Am Anfang, am Ende oder an einer Position. |
| **Zeichen entfernen** | Die ersten, letzten oder einen Bereich von Zeichen. |
| **Groß-/Kleinschreibung** | klein, GROSS, Titel, Satz. |
| **Nummerierung** | 1, 2, 3 … mit Beginn, Schritt, Stellen (führende Nullen), davor, dahinter oder als Name. |
| **Datum** | Änderungsdatum oder heute, in einem Format deiner Wahl. |
| **Suffix** | Dateiendung ändern, klein/groß schreiben oder entfernen. |
| **Aufräumen** | Leerraum trimmen, Leerzeichen zusammenfassen, Umlaute umschreiben (ä → ae, é → e). |

Jede Regel kannst du auf **Name**, **Suffix** oder **Beides** anwenden, aus- und einschalten und verschieben.

Sicherheit:

- Konflikte (doppelte Namen, schon vorhandene Namen, leere oder ungültige Namen) zeigt die Vorschau sofort. Umbenennen geht erst, wenn alle gelöst sind.
- Manta überschreibt nie etwas. Tauschen von Namen (a ↔ b) und reine Änderungen der Groß-/Kleinschreibung funktionieren.
- Danach bietet eine Mitteilung **Widerrufen** an. Auch in der Befehlspalette: **Stapelweises Umbenennen widerrufen**.

**Vorlagen**: Unter **Vorlagen** sicherst du einen Regelsatz unter einem Namen und lädst ihn später wieder.

Stapelweise umbenennen funktioniert lokal und auf Servern.

## Schnellaktionen {#schnellaktionen}

**Schnellaktionen** sind deine eigenen Befehle für die ausgewählten Dateien: Bilder verkleinern, ZIP erstellen, in VS Code öffnen, Prüfsummen und alles, was ein Shell-Befehl kann.

### So führst du eine Schnellaktion aus

- **Rechtsklick** auf Dateien → Untermenü **Schnellaktionen**. Es zeigt nur Aktionen, die zur Auswahl passen (z. B. „Bilder auf 50 % verkleinern“ nur für Bilder).
- **Befehlspalette** (**⇧⌘P**): Tippe den Namen der Aktion.
- **Panel**: **Schnellaktionen-Panel einblenden** in der Palette zeigt die passenden Aktionen und unter **Zuletzt ausgeführt** Status, Dauer und Ausgabe.

Hintergrund-Aktionen laufen höchstens zu zweit gleichzeitig. Dauert eine länger, erscheint eine Mitteilung mit **Abbrechen**; am Ende eine Ergebnis-Mitteilung. Terminal-Aktionen laufen sichtbar im [Terminal](11-terminal.md) unter dem Bereich.

### Mitgelieferte Aktionen

Bilder auf 50 % verkleinern · In JPEG umwandeln · In PNG umwandeln · Als ZIP komprimieren · Archiv entpacken · In VS Code öffnen · In Terminal.app öffnen · Pfad kopieren · Im Finder zeigen · Ausführbar machen · Zeilen zählen · SHA-256 berechnen · Per rsync in anderen Bereich

### Eigene Aktionen in actions.json {#actions-json}

Die Aktionen stehen in der Datei `actions.json` im Datenordner des Plugins (`~/Library/Application Support/Manta/plugin-data/manta.quickactions/`).

1. Wähle in der Befehlspalette **Schnellaktionen bearbeiten …**. Die Datei öffnet sich in deinem Texteditor.
2. Ändere oder ergänze Aktionen und sichere die Datei.
3. Manta lädt sie innerhalb einer Sekunde neu und meldet Fehler („Schnellaktionen: …“). Von Hand: **Schnellaktionen neu laden**.

Ein Beispiel:

```jsonc
{
  "actions": [
    {
      "id": "optimize",
      "title": "Optimize PNG",
      "title_de": "PNG optimieren",
      "applies_to": ["png"],
      "command": "optipng {file}",
      "message": "Optimized {count} image{s}",
      "message_de": "{count} Bild(er) optimiert",
      "refresh": true
    }
  ]
}
```

Die wichtigsten Felder:

| Feld | Bedeutung |
|---|---|
| `id` | Eindeutige Kennung (Buchstaben, Ziffern, `.`, `-`, `_`). |
| `title`, `title_de` | Name im Menü; `title_de` wird verwendet, wenn Manta auf Deutsch läuft. |
| `command` | Shell-Befehl, läuft mit `/bin/sh -c` im Ordner der Auswahl. |
| `applies_to` | `"any"`, `"files"`, `"folders"` und/oder Endungen wie `"jpg"`. |
| `min`, `max` | Wie viele Objekte ausgewählt sein müssen. |
| `mode` | `"background"` (Standard), `"terminal"` (sichtbar im Terminal) oder `"silent"`. |
| `output` | `"none"`, `"toast"` (Ausgabe als Mitteilung) oder `"panel"`. |
| `confirm` | Frage vor dem Ausführen, z. B. `"Delete {count} file{s}?"`. |
| `refresh` | Bereich danach neu einlesen. |
| `shortcut` | Eigenes Tastenkürzel, z. B. `"⌃⌥Z"`. |

Platzhalter im Befehl: `{file}` (der Befehl läuft dann einmal pro Objekt), `{files}` (alle auf einmal), `{name}`, `{stem}`, `{ext}`, `{dir}`, `{dest}` (Ordner im anderen Bereich) und `{count}`. Manta setzt die Anführungszeichen selbst – schreib also `{file}`, nicht `'{file}'`.

Die vollständige Beschreibung steht in [docs/quick-actions.md](../../quick-actions.md).

> [!TIP]
> Um nach einem Update die aktuellen Standardaktionen zu bekommen, benenne `actions.json` um. Manta legt die Datei dann neu an.

Schnellaktionen funktionieren nur in lokalen Ordnern.

## Projektinfo {#projektinfo}

**Projektinfo** erkennt das Projekt, in dem du gerade bist, und macht seine Skripte mit einem Klick ausführbar.

Erkannt werden: Laravel (`artisan`), Composer, Node (npm, pnpm, yarn, bun), Docker Compose, Rust (Cargo), Python, Make, just und Task.

- **Chip in der Pfadleiste**: z. B. „Laravel · 28 Skripte“. Klick blendet das Projekt-Panel ein oder aus.
- **Projekt-Panel**: Name, Stammordner, Git-Branch und Versionen (z. B. „PHP ^8.2“), Tasten **In VS Code öffnen**, **Finder**, **.env** und **Zum Stammordner**, die zuletzt ausgeführten Skripte und alle Skripte nach Werkzeug geordnet.
- **Ein Klick auf ein Skript** führt es im [Terminal](11-terminal.md) unter dem Bereich aus, im Stammordner des Projekts – so, als hättest du es selbst getippt.
- **Befehlspalette**: **Projektskript ausführen …**, **Projekt-Panel einblenden**, **Letztes Skript erneut ausführen** (**⌥⌘R**).
- **Rechtsklick** auf `package.json`, `composer.json`, `Makefile` & Co.: **Skripte zeigen**.

![Projekt-Panel für ein Laravel-Projekt](images/project-info.png)

> [!NOTE]
> Skripte, die Daten löschen können (z. B. `migrate:fresh`, `docker compose down -v`, `rm -rf`, oder Namen mit „clean“, „reset“, „deploy“, „publish“), fragen vor dem Ausführen nach. Sie sind mit einem Warnzeichen markiert.

Projektinfo braucht das Terminal-Plugin und funktioniert nur in lokalen Ordnern. Details: [docs/project-info.md](../../project-info.md).

## Markdown-Vorschau {#markdown}

Die **Markdown-Vorschau** zeigt die ausgewählte `.md`-, `.markdown`- oder `.mdx`-Datei schön gesetzt in einem Panel rechts. Ist nichts ausgewählt, zeigt sie die README des Ordners.

- Öffnen und schließen: **⌥⌘M** (**Markdown-Vorschau ein-/ausblenden**).
- Unterstützt GitHub-Markdown: Tabellen, Aufgabenlisten, Hinweis-Kästen, Fußnoten, Code mit Syntaxfarben und **Kopieren**-Taste, lokale Bilder.
- **Inhalt** zeigt ein Inhaltsverzeichnis; Links zu anderen `.md`-Dateien öffnen diese, **Zurück** geht zurück.
- **Im Editor öffnen** und **Im Bereich zeigen**.
- Live: Änderst du die Datei, aktualisiert sich die Vorschau und behält die Scroll-Position.

Bilder aus dem Internet lädt die Vorschau zum Schutz deiner Privatsphäre nicht (der Server würde erfahren, dass du die Datei geöffnet hast). Einschalten: **Einstellungen** → **Plugins** → **Markdown-Vorschau** → **Bilder aus dem Internet laden**.

Dateien über 2 MB zeigt sie nur auf Wunsch teilweise (**Erste 2 MB zeigen**). Kein HTML (außer Bildern und Zeilenumbrüchen), keine SVG-Bilder, keine Formeln.

## Bildwerkzeuge {#bildwerkzeuge}

Die **Bildwerkzeuge** zeigen Details zu Bildern und bearbeiten viele Bilder auf einmal.

**Abzeichen in der Liste:**

- **lo** – niedrige Auflösung: längste Kante unter 800 px.
- **XL** – groß fürs Web: über 2 MB.
- Der Tooltip zeigt z. B. „1920×1080 · sRGB · 340 KB“.

**Bild-Panel** (**Bild-Panel einblenden** in der Palette): Vorschau, Abmessungen, Seitenverhältnis, Dateigröße, Farbprofil, DPI, Alphakanal, Animation und EXIF-Daten (Kamera, Objektiv, Belichtung, Aufnahmedatum). Hat ein Foto einen GPS-Standort, warnt das Panel davor und bietet **In Karten öffnen** an.

**Rechtsklick → Bildwerkzeuge:**

| Befehl | Was er tut |
|---|---|
| **Größe ändern …** | 50 %, 1920/1280/800/400 px oder eigene Größe; Seitenverhältnis bleibt. |
| **Konvertieren in …** | JPEG, PNG, HEIC, TIFF; WebP mit `cwebp`. |
| **90° nach links/rechts drehen**, **180° drehen**, **Horizontal/Vertikal spiegeln** | |
| **Metadaten entfernen …** | Entfernt EXIF (auch GPS), XMP, IPTC und Kommentare – verlustfrei bei JPEG, PNG und WebP. HEIC und TIFF brauchen `exiftool`. |
| **Fürs Web optimieren …** | Längste Kante höchstens 2560 px, sRGB, JPEG Qualität 82 (PNG bleibt bei Transparenz). |
| **@2x/@1x-Set erstellen …** | Original als @2x und eine halb so große @1x-Version. |
| **Als Data-URI kopieren** | Für ein einzelnes Bild, zum Einbetten in HTML/CSS. |

Unter **Sichern als** wählst du: **Neben dem Original**, **In einem Unterordner** oder **Originale ersetzen** (die Originale kommen in den Papierkorb). Vorhandene Dateien überschreibt Manta nie ungefragt; belegte Namen bekommen `-2`, `-3` …

Die Bildwerkzeuge nutzen `sips` aus macOS. Für WebP installierst du `cwebp` (`brew install webp`), für HEIC/TIFF-Metadaten `exiftool` (`brew install exiftool`). Sie funktionieren nur in lokalen Ordnern.

![Bild-Panel mit EXIF-Daten](images/image-tools.png)

## Duplikate {#duplikate}

**Duplikate** findet identische Dateien in einem Ordner und seinen Unterordnern und legt die überzähligen Kopien in den Papierkorb.

1. Öffne den Ordner und wähle in der Befehlspalette **Duplikate in diesem Ordner suchen** – oder Rechtsklick auf einen Ordner → **Duplikate hier suchen**.
2. Manta durchsucht den Ordner im Hintergrund (abbrechbar) und vergleicht Größe, dann einen schnellen Hash, dann SHA-256.
3. Das Panel **Duplikate** zeigt die Gruppen, die Anzahl doppelter Dateien und wie viel Platz frei würde. Die Gruppen mit der größten Ersparnis stehen oben.
4. In jeder Gruppe bleibt eine Kopie („Bleibt“). Welche, bestimmt die **Behalten-Regel**: **Älteste**, **Neueste**, **Kürzester Pfad** oder **Im Ordner …**. Klickst du auf eine andere Kopie, bleibt stattdessen diese.
5. **Kopien entfernen** (pro Gruppe) oder **Kopien in den Papierkorb** (alle) – nach einer Rückfrage mit Anzahl und Größe.

![Duplikate-Panel mit Gruppen](images/duplicates.png)

Gut zu wissen:

- Duplikate verwendet immer den **Papierkorb** von macOS, nie endgültiges Löschen. Du kannst alles zurücklegen.
- Dateien, die sich seit der Suche geändert haben, bleiben unangetastet.
- `node_modules`, `vendor`, `.git`, Pakete (z. B. `.app`), symbolische Links und zweite feste Links werden übersprungen.
- **Im Finder zeigen** und **Im Bereich auswählen** helfen beim Prüfen.
- Optionen: im Panel, unter **Einstellungen** → **Plugins** → **Duplikate** (Pakete überspringen, Unsichtbare Dateien einbeziehen) und in `settings.json` (Ausschlüsse, Mindestgröße, eigener Papierkorb-Befehl).

Duplikate funktioniert nur in lokalen Ordnern.

## Systemauslastung {#systemauslastung}

**Systemauslastung** zeigt rechts in der Statusleiste, wie stark dein Mac gerade arbeitet: „CPU 12 %  GPU 3 %  RAM 12,4 / 16 GB“, jeweils mit einem kleinen Balken. Die Werte werden alle 2 Sekunden im Hintergrund gelesen – ohne Administratorrechte und ohne Hilfsprogramm. Ist das Fenster minimiert oder verdeckt, pausiert die Messung.

- Über 85 % Last oder bei erhöhtem Speicherdruck werden Wert und Balken gelb, bei kritischem Speicherdruck rot.
- Der Tooltip zeigt Details mit den Begriffen der Aktivitätsanzeige: die Kerne (Performance- und Effizienz-Kerne), den am stärksten belasteten Kern, die GPU-Kerne, App-Speicher, reservierten und komprimierten Speicher, Swap und Speicherdruck.
- **RAM** ist der „Belegte Speicher“ der Aktivitätsanzeige (App-Speicher + reservierter + komprimierter Speicher), ohne Caches.
- Ein Klick öffnet die **Aktivitätsanzeige**.
- In schmalen Fenstern verschwindet zuerst der Hinweis auf die Befehlspalette, dann die GPU; danach zeigen CPU und RAM nur noch Symbol und Balken.

Ausschalten kannst du die Anzeige unter **Einstellungen** → **Plugins** → **Systemauslastung**; dort lässt sich auch **GPU-Last anzeigen** abwählen. Auf Macs ohne GPU-Statistik fehlt die GPU von selbst.

## Mitgelieferte Plugins {#mitgeliefert}

Unter **Einstellungen** → **Plugins** → **Verfügbar** stehen Plugins, die mit Manta geliefert, aber nicht installiert sind (z. B. das Beispiel „Hello Python“). **Installieren** kopiert sie in deinen Plugin-Ordner; danach sind sie zunächst ausgeschaltet. Siehe [Eigene Plugins](15-eigene-plugins.md).
