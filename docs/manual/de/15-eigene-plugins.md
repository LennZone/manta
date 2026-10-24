# Eigene Plugins {#eigene-plugins}

Du kannst Manta mit eigenen Plugins erweitern – in jeder Programmiersprache. Dieses Kapitel erklärt, was ein Plugin kann, wie du eines installierst, aktualisierst und findest und worauf du achten musst. Die technische Anleitung steht in der Entwicklerdokumentation.

## Was Plugins können {#moeglichkeiten}

- **Abzeichen** neben Dateinamen anzeigen (wie die Git-Abzeichen).
- **Chips** in der Pfadleiste anzeigen (wie den Branch oder das Projekt).
- **Befehle** für die Befehlspalette und das Kontextmenü anbieten, auf Wunsch mit eigenem Tastenkürzel.
- **Panels** unter einem Bereich, rechts neben den Bereichen oder als Dialog zeigen.
- **Einträge in der Statusleiste** zeigen, mit Symbol, kleinem Balken und Warnfarbe (wie die [Systemauslastung](14-plugins.md#systemauslastung)).
- **Mitteilungen** anzeigen.

Ein Plugin ist ein Ordner mit einer Datei `plugin.toml` und einem Programm (z. B. ein Python-Skript). Manta startet das Programm und tauscht mit ihm Nachrichten aus (JSON-Zeilen über stdin/stdout). Eine `.mantaplugin`-Datei ist einfach dieser Ordner als ZIP-Archiv.

## So installierst du ein Plugin {#installieren}

1. Öffne **Einstellungen** (**⌘,**) → **Plugins**.
2. Klicke auf **Plugin installieren …** und wähle **Aus einer Datei (.mantaplugin, .zip) …** oder **Aus einem Ordner …** (dem Ordner mit der `plugin.toml`). Schneller geht’s per **Doppelklick** auf eine `.mantaplugin`-Datei im Finder, oder du **ziehst sie auf** die Plugins-Seite. Plugins aus dem Community-Verzeichnis installierst du über [Entdecken](#entdecken).
3. Bevor etwas installiert wird, zeigt Manta eine Übersicht: Name, Version, Entwickler, Beschreibung, **Was es darf** (z. B. „Liest deine Dateien“, „Ändert oder löscht Dateien“, „Greift auf das Netzwerk zu“, „Startet andere Programme“, „Führt Befehle im Terminal aus“), den **Befehl**, die **Quelle**, einen Link zum **Quellcode**, die **Lizenz** und die Größe. Darunter steht der Hinweis, dass Manta Plugins nicht prüft.
4. Klicke auf **Installieren**. Das Plugin steht dann unter **Installiert** – noch ausgeschaltet.

![Übersicht vor dem Installieren eines Plugins](images/plugin-install.png)

> [!NOTE]
> Die Berechtigungen gibt der Entwickler des Plugins an, und Manta **setzt sie durch**: Das Plugin läuft in einer macOS-Sandbox und kann nichts anderes tun (siehe [Sicherheit](#sicherheit)).

### Einschalten {#einschalten}

Klicke auf der Karte des Plugins auf **Einschalten**. Beim ersten Mal fragt Manta nach und zeigt noch einmal Entwickler, Version, Berechtigungen, den Befehl und den Ordner, in dem er läuft.

![Rückfrage beim Einschalten eines externen Plugins](images/plugin-confirm.png)

Hast du das Plugin aus dem Internet geladen, hält macOS seine Dateien unter Quarantäne, und Gatekeeper würde Programme darin blockieren. Die Rückfrage weist darauf hin: Erst wenn du einschaltest, entfernt Manta die Quarantäne-Markierung von den Dateien des Plugins, damit seine Programme laufen können – nie ohne dein Zutun.

Weitere Tasten auf der Karte eines Plugins: **Neu laden**, **Protokoll einblenden**, **Ordner zeigen**, **Deinstallieren**. Ganz oben: **Plugin-Ordner öffnen** und **Neu laden** (sucht nach neuen, geänderten oder entfernten Plugins).

## Aktualisieren und deinstallieren {#aktualisieren}

- **Aktualisieren**: Hat das Plugin-Verzeichnis eine neuere Version, zeigt die Karte des Plugins „Version … ist im Plugin-Verzeichnis verfügbar“ und eine Taste **Aktualisieren**. Oder installiere die neue Version genauso wie oben, aus einer Datei oder einem Ordner. Hat sie dieselbe ID wie ein installiertes Plugin, heißt die Übersicht „… aktualisieren?“ und zeigt unter **Installiert** die Version, die ersetzt wird. Nach der Bestätigung ersetzt Manta das Plugin; ob es ein- oder ausgeschaltet war, bleibt erhalten.
- **Deinstallieren**: Klicke auf der Karte des Plugins auf **Deinstallieren**. Manta löscht den Plugin-Ordner; der Datenordner des Plugins bleibt erhalten.

## Plugins entdecken {#entdecken}

Klicke unter **Einstellungen** → **Plugins** auf **Entdecken** (oder wähle in der Befehlspalette **Plugins entdecken …**). Manta zeigt die Plugins aus dem Community-Verzeichnis: Name und Beschreibung in deiner Sprache, Entwickler, Lizenz, die nötige Manta-Version, die Berechtigungen und einen Link zum Quellcode.

1. Klicke auf **Installieren** (oder auf **Aktualisieren**, wenn es eine neuere Version eines installierten Plugins gibt).
2. Manta lädt das Plugin herunter und vergleicht seine SHA-256-Prüfsumme mit dem Verzeichnis. Stimmen sie nicht überein, wird nichts installiert.
3. Die gewohnte Übersicht erscheint („SHA-256 stimmt mit dem Plugin-Verzeichnis überein“); weiter wie [oben](#installieren).

Plugins, die ein neueres Manta brauchen oder auf deinem Mac nicht laufen, werden angezeigt, lassen sich aber nicht installieren. **Im Web** öffnet das Verzeichnis auf [manta-filemanager.com/plugins](https://manta-filemanager.com/plugins/).

> [!NOTE]
> **Datenschutz:** Manta lädt die Liste nur dann von GitHub, wenn du **Entdecken** öffnest (einmal pro Sitzung) oder auf **Aktualisieren** klickst – nie im Hintergrund. Manta merkt sich die letzte Liste, damit „Entdecken“ auch offline funktioniert und installierte Plugins Updates anbieten können, ohne online zu gehen.

## Sicherheit {#sicherheit}

Externe Plugins laufen in einer **macOS-Sandbox**: Sie können nur, was ihre Berechtigungen erlauben. Ohne Berechtigungen kann ein Plugin nur seinen eigenen Ordner und das System lesen und in seinen eigenen Datenordner schreiben.

| Berechtigung | Erlaubt dem Plugin |
|---|---|
| Liest deine Dateien | jede Datei zu lesen |
| Ändert oder löscht Dateien | überall Dateien zu schreiben, anzulegen und zu löschen |
| Greift auf das Netzwerk zu | sich mit dem Internet und deinem Netzwerk zu verbinden |
| Startet andere Programme | andere Programme zu starten (sie bleiben in der Sandbox) |
| Führt Befehle im Terminal aus | Befehle in das Terminal von Manta einzugeben |

Dieselben Regeln gelten für alles, worum ein Plugin Manta bittet (eine Datei lesen, einen Ordner auflisten, Dateien umbenennen). Die Karte des Plugins zeigt **Sandbox**; ihr Tooltip listet die Berechtigungen auf. Eingebaute Plugins sind Teil von Manta und laufen ohne Sandbox.

Funktioniert die Sandbox auf deinem Mac nicht, zeigt die Karte **Ohne Sandbox** und den Grund, und die Übersicht weist darauf hin, dass die Berechtigungen nicht durchgesetzt werden können. Das Plugin läuft dann mit deinen Benutzerrechten.

> [!WARNING]
> Die Sandbox macht ein Plugin nicht harmlos: Ein Plugin, das deine Dateien lesen und auf das Netzwerk zugreifen darf, kann deine Dateien woandershin senden. Manta prüft Plugins nicht – auch keine aus dem Plugin-Verzeichnis. Installiere nur Plugins, denen du vertraust, und lies bei Plugins aus dem Internet den Code, bevor du sie einschaltest.

Deshalb zeigt Manta vor dem Installieren eine Übersicht, neue Plugins sind zunächst ausgeschaltet, und Manta fragt beim ersten Einschalten nach.

## Wenn ein Plugin nicht läuft {#probleme}

- Die Karte zeigt **Fehler** und den Grund, z. B. wenn `python3` fehlt.
- **Protokoll einblenden** zeigt die letzten Meldungen des Plugins.
- Stürzt ein Plugin ab, startet Manta es neu – höchstens dreimal pro Minute. Danach bleibt es im Fehlerzustand, bis du **Neu laden** klickst.
- Liegen im Plugin-Ordner zwei Ordner mit derselben Plugin-ID, zeigt der zweite **Nicht ladbar** und eine Meldung. Entferne einen der beiden.

## Wo die Plugins liegen {#speicherorte}

| Was | Wo |
|---|---|
| Installierte Plugins | `~/Library/Application Support/Manta/plugins/<id>/` |
| Daten und Protokoll eines Plugins | `~/Library/Application Support/Manta/plugin-data/<id>/` (Protokoll: `plugin.log`) |
| An/aus-Zustand | `~/Library/Application Support/Manta/plugins.json` |
| Letzte Liste des Plugin-Verzeichnisses | `~/Library/Application Support/Manta/plugin-registry.json` |
| Sandbox-Regeln eines Plugins | `~/Library/Application Support/Manta/plugin-data/<id>/sandbox.sb` |

Deinstallieren löscht den Plugin-Ordner; der Datenordner bleibt erhalten.

## Selbst ein Plugin schreiben {#schreiben}

Der schnellste Einstieg ist das Beispiel **Hello Python** (`examples/plugins/hello-python` im Quellcode, Zeile für Zeile kommentiert, nur Python-Standardbibliothek). Du kannst es auch unter **Einstellungen** → **Plugins** → **Verfügbar** installieren. Vorlagen gibt es auch für Node.js, Shell (bash + jq) und Rust (`examples/plugins/`); für Rust gibt es außerdem das Crate `manta-plugin-protocol`.

Anleitung, Protokoll-Referenz und Beispiele: [Manta-Plugins schreiben (docs/plugins.md, Englisch)](../../plugins.md).

Plugins können alle Sprachen von Manta sprechen: Manta teilt ihnen die eingestellte Sprache mit, und in der `plugin.toml` kannst du Name und Beschreibung übersetzen (`[localized.de]`, `[localized.fr]` usw.).

## Entwickeln und testen {#entwickeln}

- **Plugins neu laden** (Befehlspalette) startet alle laufenden externen Plugins neu und durchsucht den Plugin-Ordner erneut.
- **Entwicklermodus**: Starte Manta im Terminal mit
  `/Applications/Manta.app/Contents/MacOS/manta --plugin-dev ~/code/my-plugin`
  (mehrfach angebbar, oder per Umgebungsvariable `MANTA_PLUGIN_DEV=ordner1:ordner2`). Das Plugin läuft dann für diese Sitzung direkt aus diesem Ordner – vertraut, eingeschaltet und mit dem Etikett **Entwicklung**. Eine installierte Kopie mit derselben ID tritt so lange zurück; gespeichert wird nichts.
- **Ohne Manta testen**: `python3 scripts/plugin-test.py pfad/zum/plugin`.
- **Verpacken**: `python3 scripts/plugin-pack.py pfad/zum/plugin` erstellt die `.mantaplugin`-Datei und gibt ihre SHA-256-Prüfsumme für das Verzeichnis aus.
- **Sandbox**: Gib die Berechtigungen an, die dein Plugin braucht – ohne sie kann es das nicht. Wird etwas verweigert, steht es im Protokoll (`refused …`).
- **Protokoll**: **Protokoll einblenden** auf der Karte oder die Datei `~/Library/Application Support/Manta/plugin-data/<id>/plugin.log`.

Mehr dazu: [Entwickeln und Fehler suchen (docs/plugins.md, Englisch)](../../plugins.md#develop-and-debug).

## Ein Plugin veröffentlichen {#veroeffentlichen}

Lege das Plugin in ein öffentliches Repository mit einer von der OSI anerkannten Lizenz, hänge eine `.mantaplugin`-Datei an ein Release und öffne einen Pull-Request, der es in `plugins/registry.json` einträgt. Dann erscheint es unter **Entdecken**. Details: [Publishing a plugin (docs/plugins.md)](../../plugins.md#publishing-a-plugin).
