# Updates {#updates}

Manta sucht bei jedem Start leise nach einer neuen Version und installiert sie nur, wenn du zustimmst. Dieses Kapitel erklärt den Ablauf, was dabei über das Netz geht und was zu tun ist, wenn ein Update nicht klappt.

## Wie du von einem Update erfährst {#hinweis}

Gibt es eine neue Version, erscheinen:

- eine Mitteilung „Update verfügbar: 0.2.0“ mit der Taste **Anzeigen**,
- ein kleiner Hinweis in der Statusleiste („Update 0.2.0“).

Nichts schiebt sich vor deine Arbeit. Ist Manta offline, merkst du nichts; der Fehler landet nur im Protokoll.

## So suchst du von Hand nach Updates {#suchen}

- **Manta** → **Nach Updates suchen …** in der Menüleiste, oder
- **Nach Updates suchen …** in der Befehlspalette, oder
- **Einstellungen** → **Allgemein** → **Updates** → **Jetzt suchen**.

Ist alles aktuell, meldet Manta „Manta ist auf dem neuesten Stand“.

## So installierst du ein Update {#installieren}

1. Klicke in der Mitteilung auf **Anzeigen** (oder auf den Hinweis in der Statusleiste). Das Fenster **Softwareupdate** öffnet sich.
2. Lies unter **Neu in dieser Version**, was sich geändert hat. Darüber stehen Größe und Datum.
3. Wähle:
   - **Installieren und neu starten** – lädt das Update, prüft es, ersetzt die App und startet Manta neu.
   - **Später** – erinnert dich beim nächsten Start wieder.
   - **Diese Version überspringen** – diese und ältere Versionen werden nicht mehr angeboten.

![Fenster „Softwareupdate“](images/software-update.png)

Während des Downloads siehst du den Fortschritt und kannst abbrechen. Laufen noch Übertragungen, wartet Manta mit dem Neustart, bis sie fertig sind. Eventuell fragt macOS nach deinem Passwort, wenn Manta keine Schreibrechte im Ordner **Programme** hat.

Geht etwas schief, steht der Grund im Fenster, mit **Erneut versuchen**. Die alte Version bleibt dann unverändert.

## Wann Manta sich nicht selbst aktualisieren kann {#blockiert}

| Meldung | Lösung |
|---|---|
| „Manta läuft von einem Disk-Image“ | Zieh Manta in den Ordner **Programme** und öffne es von dort. |
| „Manta läuft von einem temporären Ort“ | macOS hat Manta aus einer schreibgeschützten Kopie gestartet (App-Translocation). Bewege Manta im Finder in den Ordner **Programme** und öffne es von dort. |
| „Updates sind in diesem Build nicht eingerichtet“ | Dein Build kann Updates nicht prüfen. Lade neue Versionen von der Website. |

## Kanäle: Stabil und Beta {#kanaele}

Unter **Einstellungen** → **Allgemein** → **Updates** → **Kanal** wählst du:

- **Stabil** (Standard): nur fertige Versionen.
- **Beta**: zusätzlich Vorabversionen. Sie bringen neue Funktionen früher, können aber Fehler enthalten.

Manta bietet nie eine ältere Version an als die installierte.

## Sicherheit {#sicherheit}

Jedes Update ist digital signiert. Manta installiert nur, was mit dem Schlüssel des Manta-Teams signiert ist und genau zur angekündigten Version passt. Ein manipulierter Download wird abgelehnt („Das Update ist nicht korrekt signiert, deshalb wurde nichts installiert“). Updates kommen nur über HTTPS.

## Datenschutz: was die Update-Suche sendet {#datenschutz}

Bei der Suche lädt Manta zwei kleine Dateien vom Update-Server (`manta-filemanager.com`): die Liste der Versionen und ihre Signatur. Dabei geht über das Netz:

- deine **IP-Adresse** (wie bei jedem Abruf einer Webseite),
- die Kennung **`Manta-Updater/<Version>`** – also welche Manta-Version du verwendest.

Manta sendet **keine** Benutzer-ID, keine Gerätekennung, keine Dateinamen und keine Nutzungsdaten. Welche Version für dich passt (Kanal, macOS-Version, übersprungene Version), entscheidet Manta auf deinem Mac, nicht der Server.

Willst du das nicht, schalte **Beim Start nach Updates suchen** aus. Dann geht Manta nur ins Netz, wenn du selbst **Nach Updates suchen …** wählst.

Wie bei fast jeder Website protokolliert der Webserver diese Abrufe mit IP-Adresse. Das erledigt der Hosting-Anbieter nach seinen eigenen Aufbewahrungsregeln; Manta selbst wertet die Abrufe nicht aus und verknüpft sie mit nichts.
