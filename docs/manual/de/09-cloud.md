# Cloud-Speicher {#cloud}

Manta zeigt iCloud Drive, Google Drive, OneDrive, Dropbox und andere Cloud-Speicher, die dein Mac bereits synchronisiert. Du siehst, welche Dateien nur in der Cloud liegen, und kannst sie laden oder den lokalen Download wieder entfernen.

![Cloud-Abschnitt in der Seitenleiste und Sync-Abzeichen in der Liste](images/cloud-sidebar-badges.png)

## Wie Manta auf Cloud-Speicher zugreift {#prinzip}

Manta meldet sich **nicht selbst** bei Google, Microsoft oder Dropbox an. Stattdessen nutzt Manta die Ordner, die die offiziellen Apps der Anbieter auf deinem Mac anlegen:

- **iCloud Drive** ist in macOS eingebaut.
- **Google Drive**, **OneDrive**, **Dropbox**, **Box** und andere legen ihre Ordner unter `~/Library/CloudStorage` an, sobald du ihre App installiert und dich dort angemeldet hast.

Für Manta sind das normale Ordner auf deinem Mac. Alles, was du in Manta tust – kopieren, umbenennen, löschen –, synchronisiert die App des Anbieters anschließend mit der Cloud.

## So richtest du Google Drive ein {#google-drive}

1. Lade **Google Drive für den Desktop** von Google herunter und installiere es.
2. Melde dich in der Google-Drive-App mit deinem Google-Konto an.
3. In der Seitenleiste erscheint unter **Cloud** von selbst **Google Drive** – Manta bemerkt neue Ordner in `~/Library/CloudStorage`. Klappt das ausnahmsweise nicht, klicke auf den Kreispfeil neben **Cloud**.

OneDrive und Dropbox richtest du genauso ein: App des Anbieters installieren, anmelden, fertig.

> [!NOTE]
> Manta hat **keine eigene Google-Anmeldung**. Ohne Google Drive für den Desktop siehst du Google Drive in Manta nicht.

Solange kein Google-Drive-Ordner da ist, steht unter **Cloud** die Zeile **Google Drive verbinden …**. Ein Klick darauf erklärt die Einrichtung und bietet den Link **Google Drive für den Desktop laden** (<https://www.google.com/drive/download/>). Mit **Nicht mehr anzeigen** verschwindet der Hinweis; unter **Einstellungen › Allgemein › Cloud** holst du ihn zurück.

## Der Abschnitt „Cloud“ in der Seitenleiste {#seitenleiste}

Der Abschnitt **Cloud** steht zwischen **Orte** und **Geräte**. Jeder gefundene Cloud-Speicher hat eine eigene Zeile mit dem Symbol des Anbieters. Gibt es einen Anbieter mehrmals (zwei Google-Konten, OneDrive der Firma und geteilte Bibliotheken), steht das Konto grau dahinter; sonst zeigt es der Tooltip zusammen mit dem Pfad:

- iCloud Drive (immer zuerst, wenn es eingeschaltet ist)
- Google Drive (bei mehreren Konten: Google Drive *name@gmail.com*)
- OneDrive *Persönlich* bzw. OneDrive *Name deiner Firma*, OneDrive *Geteilte Bibliotheken*
- Dropbox, Box und weitere (z. B. Nextcloud, Proton Drive, Synology Drive)

Ein Klick öffnet den Speicher im aktiven Bereich.


## Was die Abzeichen bedeuten {#abzeichen}

In Cloud-Ordnern zeigt Manta den Sync-Status: in der Liste als Symbol am rechten Ende der Namensspalte, in der Symbolansicht als runde Plakette oben rechts am Symbol. Der Tooltip nennt den Status in Worten (z. B. „Nur in der Cloud – nicht geladen“).

| Abzeichen | Bedeutung | Mögliche Aktionen |
|---|---|---|
| (keins) | Geladen und aktuell | Download entfernen |
| Wolke mit Pfeil nach unten | Nur in der Cloud, nicht auf diesem Mac | Jetzt laden |
| Wolke mit Ausrufezeichen | Geladen, aber in der Cloud gibt es eine neuere Version | Jetzt laden, Download entfernen |
| Kreis mit Pfeil nach unten | Wird gerade geladen | – |
| Wolke mit Pfeil nach oben (blau, pulsiert) | Wird hochgeladen oder wartet auf den Upload – z. B. gleich nach dem Kopieren in Google Drive | – |
| Rotes Ausrufezeichen im Kreis | Upload-Fehler: Der Anbieter konnte das Objekt nicht hochladen; der Tooltip nennt seinen Grund (z. B. „Upload-Fehler: Speicher voll“) | – |

Nach dem Kopieren in einen Cloud-Ordner lädt die App des Anbieters im Hintergrund hoch. Solange das läuft, zeigt die Statusleiste zusätzlich „Google Drive lädt 2 Objekte hoch“ (rot bei Fehlern). iCloud Drive meldet bei manchen Macs auch den Fortschritt („Wird hochgeladen … 42 %“ im Tooltip); Google Drive, OneDrive und Dropbox melden nur „wird hochgeladen“. Dateien, die Anbieter grundsätzlich nicht hochladen (`.DS_Store`, Office-Sperrdateien `~$…`), bekommen kein Abzeichen.

Das Lesen des Status lädt nichts herunter. Du kannst also durch große Cloud-Ordner blättern, ohne dass dein Mac alles lädt.

Manta liest den Status aus den Dateiattributen (Kennzeichen `SF_DATALESS` und die Statuswerte `NSURLUbiquitousItem…` von macOS, die iCloud Drive und die File-Provider-Apps melden) und aktualisiert ihn alle paar Sekunden, solange der Ordner sichtbar ist.

## So lädst du Dateien aus der Cloud {#jetzt-laden}

1. Wähle Dateien oder Ordner aus, die nur in der Cloud liegen.
2. Rechtsklick → **Jetzt laden**.

Ordner werden mit ihrem gesamten Inhalt geladen. Das Abzeichen wechselt zu „wird geladen“ und verschwindet, wenn alles da ist.

Öffnest oder kopierst du eine Datei, die nur in der Cloud liegt, lädt macOS sie ohnehin automatisch.

## So gibst du Speicherplatz frei {#download-entfernen}

1. Wähle geladene Dateien oder Ordner aus.
2. Rechtsklick → **Download entfernen**.

Die Dateien bleiben in der Cloud und in Manta sichtbar, belegen aber keinen Platz mehr auf deinem Mac.

> [!IMPORTANT]
> **Download entfernen** klappt zuverlässig bei **iCloud Drive**. Bei Google Drive, OneDrive, Dropbox und anderen entscheidet die App des Anbieters, ob sie das zulässt. Lehnt sie ab, sagt Manta dir das. Nutze dann im **Finder** Rechtsklick → **Download entfernen** – der Finder darf das, andere Apps nicht.

## Google Drive für Experten: eigener OAuth-Client {#experte}

Manta enthält zusätzlich eine eigene Google-Drive-Anbindung (Drive-API), die **ausgeblendet** ist: Manta liefert dafür keinen OAuth-Client mit. Nur wenn du Google Drive für den Desktop nicht nutzen kannst und einen eigenen Client hast:

1. Lege in der Google Cloud Console einen OAuth-Client vom Typ **Desktop-App** an und aktiviere die Google-Drive-API.
2. Öffne **Einstellungen › Allgemein › Cloud › Experte: eigener Google-OAuth-Client**, trage Client-ID und Clientschlüssel ein und klicke auf **Speichern**.
3. Im Verbinden-Dialog gibt es jetzt den Typ **Google Drive**. Lässt du **Konto** leer, öffnet Manta beim Verbinden den Browser zur Anmeldung.

Mit **Entfernen** verschwindet Google Drive wieder aus dem Verbinden-Dialog. Der Client liegt in `~/Library/Application Support/Manta/gdrive-client.json`.

## Löschen in Cloud-Ordnern {#loeschen}

> [!IMPORTANT]
> Cloud-Ordner sind Ordner auf deinem Mac. **⌘⌫** legt Objekte darin wie überall in den **Papierkorb** (siehe [Löschen](05-dateien.md#loeschen) und [Papierkorb](07-favoriten-seitenleiste.md#papierkorb)); **⌥⌘⌫** löscht nach Rückfrage endgültig. Die App des Anbieters überträgt das Löschen danach **in die Cloud und auf all deine Geräte**. Je nach Anbieter findest du die Datei noch eine Weile im Papierkorb auf seiner Website.

Willst du nur Platz sparen, verwende **Download entfernen** statt Löschen.

## Grenzen {#grenzen}

- Manta zeigt nur Cloud-Speicher, die dein Mac schon synchronisiert. Ohne die App des Anbieters geht es nicht.
- **Download entfernen** bei Drittanbietern nur, wenn deren App es erlaubt (sonst im Finder).
- Wie schnell Änderungen in der Cloud ankommen, bestimmt die App des Anbieters, nicht Manta.
