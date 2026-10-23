# Archive {#archive}

Manta öffnet ZIP- und TAR-Archive wie Ordner: Du siehst den Inhalt sofort, ohne vorher zu entpacken. Außerdem packst du hier Archive aus und erstellst neue.

## Unterstützte Formate {#formate}

- `.zip`
- `.tar`, `.tar.gz` / `.tgz`, `.tar.bz2`, `.tar.xz`, `.tar.zst`
- einzelne komprimierte Dateien: `.gz`, `.bz2`, `.xz`, `.zst`

## So öffnest du ein Archiv als Ordner {#oeffnen}

1. Wähle das Archiv aus.
2. Drücke **↩** oder doppelklicke darauf.

Die Pfadleiste zeigt das Archiv wie einen Ordner, z. B. `Downloads › backup.zip › docs`. Ein Chip mit Schloss markiert den Bereich als **schreibgeschütztes Archiv**.

- **⌫** oder **⌘↑** im obersten Ordner des Archivs führt zurück in den Ordner, in dem das Archiv liegt – das Archiv ist dann ausgewählt.
- **Zurück** und **Vorwärts** funktionieren über das Archiv hinweg.

![Ein ZIP-Archiv als Ordner geöffnet](images/archive-as-folder.png)

Andere Wege:

- **⌥↩** (**Mit Standard-App öffnen**) öffnet das Archiv mit der Standard-App (z. B. dem Archivierungsprogramm von macOS).
- Dokumente, die intern ZIP-Dateien sind (`.docx`, `.xlsx`, `.jar`, `.apk` …), öffnen sich weiter in ihrer App. Ihren Inhalt siehst du mit Rechtsklick → **Archiv durchsuchen**.
- Willst du Archive nie als Ordner öffnen, schalte **Einstellungen** → **Allgemein** → **Archive als Ordner öffnen** aus.

Archive auf Servern und Archive in Archiven lädt Manta zuerst in eine temporäre Datei (mit Fortschritt und **Abbrechen** im Bereich). Große Archive liest Manta im Hintergrund ein.

## Was in einem Archiv geht – und was nicht {#schreibgeschuetzt}

Archive sind **schreibgeschützt**. Neue Ordner, Umbenennen, Löschen, Bewegen und das Ablegen von Dateien im Archiv verweigert Manta mit einem Hinweis.

**Herauskopieren** geht jederzeit:

- mit **F5** in den anderen Bereich,
- per Drag & Drop in einen anderen Bereich,
- mit **Hier entpacken** oder **In anderen Bereich entpacken**.

## So entpackst du ein Archiv {#entpacken}

**Ein ganzes Archiv:**

1. Wähle die Archivdatei aus.
2. Drücke **⌥⌘E** (**Hier entpacken**) – oder Rechtsklick → **Hier entpacken** bzw. **In anderen Bereich entpacken**.

**Hier entpacken** packt neben dem Archiv aus: in einen Ordner mit dem Namen des Archivs, oder direkt, wenn das Archiv nur ein einziges Objekt enthält.

**Einzelne Objekte aus einem geöffneten Archiv:**

1. Öffne das Archiv als Ordner und wähle die Objekte aus.
2. **⌥⌘E** entpackt sie in den Ordner, der das Archiv enthält. **F5** oder Rechtsklick → **In anderen Bereich entpacken …** entpackt sie in den anderen Bereich.

Entpacken läuft als Übertragung im Hintergrund (siehe [Übertragungen](05-dateien.md#uebertragungen)).

## So erstellst du ein Archiv {#komprimieren}

1. Wähle Dateien und Ordner aus.
2. Drücke **⌥⌘C** (**Komprimieren …**) oder wähle es im Kontextmenü.
3. Wähle das **Format**: zip, tar.gz, tar.xz oder tar.zst.
4. Prüfe den **Namen**. Manta schlägt einen vor, z. B. „site.zip“ oder „Archiv 2.zip“.
5. Wähle mit dem Regler **Stufe** zwischen **Schneller** und **Kleiner**.
6. **.DS_Store und __MACOSX auslassen** ist eingeschaltet; so landen keine Mac-Hilfsdateien im Archiv.
7. Klicke auf **Komprimieren**.

Das Archiv entsteht im aktuellen Ordner. Das funktioniert auch auf Servern: Manta baut das Archiv dann lokal in einer temporären Datei und lädt es hoch.

![Komprimieren-Dialog](images/compress-sheet.png)

> [!TIP]
> Für Mac-Empfänger ist **zip** am bequemsten. **tar.zst** ist sehr schnell und klein, braucht aber auf der Gegenseite ein passendes Programm.

## Sicherheit {#sicherheit}

- Einträge mit `..` oder absoluten Pfaden könnten beim Entpacken außerhalb des Zielordners schreiben. Manta zeigt und entpackt sie nie und meldet „N unsichere Einträge übersprungen“.
- Verschlüsselte ZIP-Einträge haben ein Schloss-Abzeichen. Manta kann sie nicht öffnen oder entpacken und fragt nicht nach einem Passwort.

## Grenzen {#grenzen}

- Keine Änderungen in Archiven (hinzufügen, löschen, umbenennen).
- Keine passwortgeschützten ZIP-Einträge.
- Symbolische Links werden als Links gezeigt; beim Entpacken wird die Zieldatei kopiert, Links auf Ordner werden übersprungen.
- Komprimierte TARs über 2 GB werden langsamer, weil Manta sie für jede Datei neu von vorn liest.
- Geteilte ZIP-Archive („split“) und TAR-Sparse-Dateien gehen nicht.
