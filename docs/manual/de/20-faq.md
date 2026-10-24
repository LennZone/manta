# Häufige Fragen {#faq}

Kurze Antworten auf Fragen, die oft gestellt werden. Ausführliches findest du in den verlinkten Kapiteln.

## Allgemein

### Ersetzt Manta den Finder? {#finder-ersetzen}

Nicht ganz. Du kannst Manta als Standard-Dateimanager festlegen; dann öffnen sich „Im Finder anzeigen“ und (bis macOS 15) Ordner aus anderen Apps in Manta. Schreibtisch, Finder-Symbol im Dock und Doppelklicks in Finder-Fenstern bleiben aber immer beim Finder – das erlaubt macOS keiner App. Siehe [Standard-Dateimanager](01-erste-schritte.md#standard-dateimanager).

### Kostet Manta etwas oder sammelt es Daten? {#daten}

Manta sammelt keine Nutzungsdaten. Ins Netz geht es nur für deine eigenen Verbindungen und die Update-Suche, die nur deine IP-Adresse und die Manta-Version übermittelt. Siehe [Datenschutz](17-updates.md#datenschutz).

Manta ist kostenlos und Open Source unter der MIT-Lizenz: Du darfst es frei benutzen, ändern und weitergeben. Der Quellcode liegt auf [GitHub](https://github.com/web-workers-com/manta) – in der App unter **Hilfe** → **Quellcode auf GitHub …** –, Beiträge sind willkommen. Neuigkeiten, Hilfe und Kontakt findest du auf der Website [manta-filemanager.com](https://manta-filemanager.com/) – in der App unter **Hilfe** → **Manta-Website & Support …** und in den Einstellungen unter [Über Manta](16-einstellungen.md#ueber).

Manta baut auf Open-Source-Komponenten auf. Ihre Lizenzhinweise findest du unter **Hilfe** → **Lizenzen von Drittanbietern** oder [hier](../../third-party-licenses.md).

### Ich habe einen Fehler gefunden oder eine Idee – wo melde ich das? {#feedback}

Auf GitHub: **Hilfe** → **Fehler melden …** öffnet das Formular für Fehlerberichte, Manta- und macOS-Version sind schon eingetragen; **Hilfe** → **Idee vorschlagen …** öffnet das Formular für Ideen. Was in einen guten Fehlerbericht gehört und wo die Protokolldatei liegt, steht unter [Fehler melden](19-fehlerbehebung.md#fehler-melden). Sicherheitslücken meldest du vertraulich, nie in einem öffentlichen Issue – siehe [Sicherheitsrichtlinie](https://github.com/web-workers-com/manta/blob/main/SECURITY.md).

### Gibt es Manta auf Deutsch? {#sprache}

Ja. **Einstellungen** → **Allgemein** → **Sprache**: **System** (Standard, folgt den bevorzugten Sprachen in macOS) oder eine der sieben Sprachen der App: **Deutsch**, **English**, **简体中文**, **Español**, **Français**, **Italiano**, **Русский**. Der Wechsel wirkt sofort. Auch das Handbuch (**⌘?**) erscheint in der Sprache der App; gibt es es in einer Sprache nicht, zeigt Manta die englische Fassung.

### Wie viele Bereiche kann ich öffnen? {#bereiche}

So viele, wie in dein Fenster passen – jeder braucht etwa 280 Punkte Breite. Mindestens einer bleibt immer offen. Siehe [Bereiche & Tabs](02-bereiche-tabs.md).

## Dateien

### Wo ist der Papierkorb? Kann ich Gelöschtes wiederherstellen? {#papierkorb}

Ja, für Objekte auf deinem Mac: **⌘⌫** legt sie wie im Finder in den Papierkorb von macOS, und **Zurücklegen** im Finder holt sie zurück. **⌥⌘⌫** (**Sofort löschen …**) löscht nach Rückfrage endgültig. Server, S3 und Container haben keinen Papierkorb: Dort ist Löschen immer endgültig, und Manta fragt vorher nach. Siehe [Löschen](05-dateien.md#loeschen).

### Wird beim Ziehen kopiert oder bewegt? {#ziehen}

Manta fragt nach und hat **Kopieren** vorausgewählt – auch auf demselben Volume, wo der Finder bewegen würde. So verschwindet nichts versehentlich aus seinem Ordner. Halte beim Loslassen **⌘** gedrückt, um **Bewegen** vorzuwählen; **⌥** wählt **Kopieren**. Der Hinweis am Mauszeiger sagt dir vorher, was vorausgewählt ist. In den [Einstellungen](16-einstellungen.md#kopieren-bewegen) kannst du stattdessen „Immer kopieren“ oder „Immer bewegen“ (ohne Dialog) einstellen. Ziehen auf den Papierkorb legt immer in den Papierkorb. Siehe [Drag & Drop](05-dateien.md#drag-drop).

### Ich habe eine Datei vom Server geöffnet und geändert – wo ist die Änderung? {#server-datei}

In einer temporären Kopie auf deinem Mac. Manta lädt geänderte Dateien nicht automatisch hoch. Kopiere die Datei in einen lokalen Ordner, bearbeite sie dort und kopiere sie mit **F5** zurück. Siehe [Navigation](04-navigation.md#oeffnen).

### Warum sehe ich meine `.env`- oder `.git`-Dateien nicht? {#unsichtbar}

Dateien mit einem Punkt am Anfang sind unsichtbar. Drücke **⇧⌘.**. Siehe [Unsichtbare Dateien](04-navigation.md#unsichtbare-dateien).

### Warum ändert sich ein Ordner nicht, wenn eine andere App Dateien hineinlegt? {#aktualisieren}

Drücke **⌘R**. Manta liest Ordner nach eigenen Aktionen neu ein, aber nicht, wenn andere Apps etwas ändern.

### Kann ich in ZIP-Archiven Dateien ändern? {#archiv-aendern}

Nein, Archive sind schreibgeschützt. Entpacke sie, ändere die Dateien und komprimiere sie neu (**⌥⌘C**). Siehe [Archive](10-archive.md).

### Was passiert mit einer Kopie, wenn das WLAN weg ist oder ich Manta beende? {#unterbrochen}

Nichts geht verloren. Bei einem Netzwerkabbruch wartet die Übertragung und läuft an derselben Stelle weiter, sobald die Verbindung zurück ist. Beendest du Manta (oder stürzt es ab), fragt es beim nächsten Start „Unterbrochene Übertragungen fortsetzen?“. Siehe [Übertragungen pausieren und fortsetzen](05-dateien.md#pausieren).

### Was sind die Dateien `.NAME.manta-part`? {#manta-part}

Unsichtbare Teildateien, in die Manta beim Kopieren schreibt. Erst die fertige Datei bekommt ihren richtigen Namen. Siehe [Teildateien](05-dateien.md#teildateien).

## Verbindungen und Cloud

### Werden meine Server-Passwörter sicher gespeichert? {#passwoerter}

Ja. Mit **Passwort merken** speichert Manta Passwörter, Passphrasen und geheime S3-Schlüssel im macOS-Schlüsselbund, nicht in einer Datei. Ältere, unverschlüsselt gespeicherte Passwörter verschiebt Manta beim Start dorthin. Ohne **Passwort merken** fragt Manta bei jeder Verbindung. Ganz ohne gespeichertes Passwort kommt SFTP mit dem ssh-agent aus. Siehe [Verbindungen](08-verbindungen.md#favoriten).

### Kann ich mich direkt bei Google Drive anmelden? {#google}

Nein. Installiere **Google Drive für den Desktop** und melde dich dort an; Manta zeigt dann den Google-Drive-Ordner. Siehe [Cloud-Speicher](09-cloud.md#google-drive).

### Unterstützt Manta WebDAV, SMB oder Dropbox ohne App? {#protokolle}

Direkt verbinden kann Manta mit SFTP, FTP, FTPS, S3 und Containern. SMB- und WebDAV-Freigaben bindest du im Finder ein (**Gehe zu** → **Mit Server verbinden**); sie erscheinen dann unter **Geräte**. Dropbox, OneDrive & Co. zeigt Manta über die Apps der Anbieter.

### Warum steht ein Warnschild am FTP-Chip? {#ftp-warnung}

Dein Netzwerk blockiert verschlüsselte FTP-Datenverbindungen. Manta überträgt Dateien deshalb unverschlüsselt, die Anmeldung bleibt verschlüsselt. Siehe [Datenkanal](08-verbindungen.md#datenkanal).

### Kann Manta Container starten oder stoppen? {#container}

Nein. Manta zeigt und ändert nur Dateien in Containern. Container startest und stoppst du mit Docker Desktop oder im Terminal.

## Erweiterungen

### Kann ich eigene Befehle hinzufügen, ohne ein Plugin zu schreiben? {#eigene-befehle}

Ja, mit **Schnellaktionen**: Ein Eintrag in `actions.json` mit einem Shell-Befehl genügt. Siehe [Schnellaktionen](14-plugins.md#actions-json).

### Sind Plugins sicher? {#plugins-sicher}

Eingebaute Plugins sind Teil von Manta. Externe Plugins laufen in einer macOS-Sandbox und können nur, was ihre Berechtigungen erlauben – installiere trotzdem nur, was du vertraust. Siehe [Eigene Plugins](15-eigene-plugins.md#sicherheit).
