# Favoriten & Seitenleiste {#seitenleiste}

Die Seitenleiste links bringt dich mit einem Klick zu deinen wichtigsten Orten: Favoriten, Standardordner, Laufwerke, verbundene Server, Container und Cloud-Speicher. Ein Klick öffnet den Ort im aktiven Bereich.

![Seitenleiste mit allen Abschnitten](images/sidebar.png)

## Die Abschnitte {#abschnitte}

| Abschnitt | Inhalt |
|---|---|
| **Favoriten** | Ordner und Server-Verbindungen, die du dir gemerkt hast. |
| **Orte** | Benutzerordner, Schreibtisch, Dokumente, Downloads und ganz unten der **Papierkorb** (siehe unten). |
| **Cloud** | iCloud Drive und die Ordner der Cloud-Apps (Google Drive für den Desktop, OneDrive, Dropbox, Box …), siehe [Cloud-Speicher](09-cloud.md). Der Abschnitt erscheint nur, wenn Manta mindestens einen Cloud-Speicher findet. |
| **Geräte** | Macintosh HD und alle eingebundenen Volumes (USB-Sticks, Festplatten, Netzwerkfreigaben, Disk-Images). |
| **Server** | Aktuell verbundene Server. Ohne Verbindung steht hier **Mit Server verbinden …**. |
| **Container** | Laufende und gestoppte Docker-Container und Kubernetes-Pods (nur wenn Docker installiert ist oder ein Kubernetes-Kontext gesetzt ist). |

Der Knopf mit dem Kreispfeil neben **Cloud** und **Geräte** sucht erneut nach Cloud-Ordnern und Volumes. Neue Cloud-Apps erkennt Manta aber auch von selbst.

Fährst du mit der Maus über einen Ort, zeigt ein Tooltip den vollständigen Pfad.

## So fügst du einen Ordner zu den Favoriten hinzu {#favorit-hinzufuegen}

Es gibt drei Wege:

- **Tastatur**: Öffne den Ordner im aktiven Bereich und drücke **⌃⌘T** (**Ordner zu Favoriten hinzufügen**).
- **Maus**: Klicke auf das **+** neben der Überschrift **Favoriten** („Aktuellen Ordner hinzufügen“), oder wähle **Ordner zu Favoriten hinzufügen** im Kontextmenü einer leeren Stelle im Bereich.
- **Ziehen**: Zieh einen oder mehrere Ordner aus einem Bereich auf den Abschnitt **Favoriten** (siehe unten).

Ordner **in Archiven** können keine Favoriten sein („Ordner in Archiven können nicht zu den Favoriten hinzugefügt werden“).

### Ordner auf die Favoriten ziehen {#favoriten-ziehen}

1. Wähle in einem Bereich einen oder mehrere Ordner aus – in jeder Ansicht, lokal oder auf einem Server.
2. Zieh sie auf den Abschnitt **Favoriten**. Der Abschnitt wird hervorgehoben, eine Linie zeigt, wo die Ordner eingefügt werden, und am Mauszeiger steht z. B. „2 Ordner zu den Favoriten“.
3. Lass los. Manta meldet „„Projekte“ zu den Favoriten hinzugefügt“ bzw. „2 Ordner zu den Favoriten hinzugefügt“.

Gut zu wissen:

- **Nur Ordner**: Ziehst du nur Dateien, zeigt der Mauszeiger „Nur Ordner können Favoriten sein“ und nichts passiert. Bei gemischter Auswahl nimmt Manta nur die Ordner.
- **Keine Doppelten**: Ist ein Ordner schon ein Favorit, blinkt der vorhandene Eintrag kurz auf, und Manta sagt „„Name“ ist schon in den Favoriten“. Der Name des Favoriten spielt dabei keine Rolle, nur der Ort.
- **Server-Ordner**: Ziehst du einen Ordner von einem Server, entsteht ein Server-Favorit mit diesem Ordner als Startordner, z. B. „files.example.com – www“. Ein neues Passwort wird dabei nicht gespeichert; liegt für diesen Server-Account schon eines im Schlüsselbund (von einem anderen Favoriten), nutzt der neue Favorit es mit. Sonst fragt Manta beim Öffnen danach (außer bei Anmeldung per ssh-agent oder Schlüsseldatei ohne Passphrase).

![Ordner wird auf die Favoriten gezogen](images/favorites-drag.png)

## Favoriten ordnen und entfernen {#favoriten-ordnen}

- **Neu ordnen**: Zieh einen Favoriten in der Liste nach oben oder unten. Der gezogene Eintrag wird blass, eine Linie zeigt die neue Position. Die Reihenfolge wird sofort gespeichert.
- **Entfernen**: Rechtsklick auf den Favoriten → **Aus Favoriten entfernen**, oder zieh den Favoriten auf den **Papierkorb** in der Seitenleiste. Der Ordner selbst bleibt natürlich erhalten.

## Papierkorb {#papierkorb}

Der **Papierkorb** steht als letzter Eintrag unter **Orte**, wie im Finder. Sein Symbol zeigt, ob er leer oder voll ist (gefüllter Eimer = es liegt etwas darin); der Tooltip nennt die Anzahl der Objekte.

- **Öffnen**: Klicke auf **Papierkorb**. Er öffnet sich im aktiven Bereich wie ein normaler Ordner. Mit **Sofort löschen …** (oder **⌘⌫**) löschst du darin einzelne Objekte endgültig – Manta fragt vorher nach.
- **Entleeren**: Rechtsklick auf **Papierkorb** → **Papierkorb entleeren …**, oder **⇧⌘⌫**, oder **Papierkorb entleeren …** in der Befehlspalette. Manta fragt nach („Möchtest du die Objekte im Papierkorb wirklich endgültig löschen?“) und nennt Anzahl und Größe. Du kannst das nicht widerrufen. Objekte, die sich nicht löschen lassen (z. B. geschützte), überspringt Manta und meldet sie.
- **Hineinziehen**: Zieh Objekte aus einem Bereich auf **Papierkorb**. Der Eintrag wird hervorgehoben, am Mauszeiger steht z. B. „3 Objekte in den Papierkorb legen“. Lokale Objekte landen ohne Rückfrage im Papierkorb (wie mit **⌘⌫**). Objekte von einem **Server**, aus **S3** oder einem **Container** können nicht in den Papierkorb: Manta fragt, ob sie endgültig gelöscht werden sollen. Objekte aus einem **Archiv** lassen sich nicht löschen – der Mauszeiger zeigt das. Auch Dateien aus anderen Apps (z. B. aus dem Finder) kannst du auf den Papierkorb ziehen.
- **Zurücklegen**: Das geht nur im Finder (Rechtsklick → **Im Finder öffnen**, dann **Zurücklegen**). Der Finder speichert den ursprünglichen Ort in eigenen Daten, die Manta nicht lesen kann.

### Festplattenvollzugriff {#papierkorb-zugriff}

macOS schützt den Papierkorb: Um seinen Inhalt anzuzeigen, braucht Manta den **Festplattenvollzugriff**. Fehlt er, zeigt der Bereich „Manta braucht Festplattenvollzugriff, um den Papierkorb anzuzeigen“ mit zwei Knöpfen:

1. **Festplattenvollzugriff öffnen** öffnet **Systemeinstellungen › Datenschutz & Sicherheit › Festplattenvollzugriff**. Schalte dort **Manta** ein und öffne den Papierkorb erneut (**⌘R**). Manchmal muss Manta dafür neu gestartet werden.
2. **Im Finder öffnen** zeigt den Papierkorb im Finder – auch wenn Manta dein Standard-Dateimanager ist.

Ohne Festplattenvollzugriff funktionieren das Hineinziehen und **⌘⌫** trotzdem, und der Eintrag zeigt weiter an, ob der Papierkorb leer oder voll ist. **Papierkorb entleeren …** übernimmt dann der Finder (macOS fragt beim ersten Mal, ob Manta den Finder steuern darf); dabei leert der Finder auch den Papierkorb anderer Volumes.

## Server als Favoriten {#server-favoriten}

Eine Server-Verbindung wird zum Favoriten, wenn du im Dialog **Mit Server verbinden** die Option **Zu Favoriten hinzufügen** einschaltest. Du kannst ihr einen eigenen Namen geben („Name in der Seitenleiste (optional)“) und einen Startordner festlegen.

Rechtsklick auf einen Server-Favoriten:

- **Öffnen** – verbindet und öffnet den Startordner.
- **Verbindung bearbeiten …** – öffnet den Verbindungsdialog mit den gespeicherten Daten.
- **Aus Favoriten entfernen**.

Drückst du **⌃⌘T** in einem Ordner auf einem Server, legt Manta einen Server-Favoriten mit diesem Ordner als Startordner an. Das Passwort wird dabei nicht übernommen.

Mehr dazu in [Verbindungen](08-verbindungen.md#favoriten).

## Laufwerke {#geraete}

Neu eingesteckte Laufwerke erscheinen unter **Geräte**. Taucht eines nicht auf, klicke auf das Pfeil-Symbol neben **Geräte** (**Volumes neu einlesen**).

Einen Auswerfen-Knopf für Laufwerke hat Manta nicht: Wirf sie im Finder aus (oder mit dem Auswerfen-Symbol neben dem Laufwerk in der Finder-Seitenleiste). Das Auswerfen-Symbol in Mantas Seitenleiste gibt es nur bei verbundenen Servern; es trennt die Verbindung.

## Verbundene Server {#server}

Unter **Server** steht jede offene Verbindung mit einem grünen Punkt. Ein Klick öffnet sie im aktiven Bereich. Das Auswerfen-Symbol beim Darüberfahren trennt die Verbindung (**Verbindung trennen**).

## Container {#container}

Der Abschnitt **Container** listet deine Docker-Container, gruppiert nach Compose-Projekt wie in Docker Desktop. Laufende Container haben einen farbigen Punkt; gestoppte sind abgeblendet und unter „Gestoppt (N)“ zusammengeklappt. Für den aktuellen Kubernetes-Kontext gibt es eine Namespace-Auswahl mit Pods und ihren Containern.

Die Liste aktualisiert sich alle 5 Sekunden im Hintergrund. Das Pfeil-Symbol (**Container aktualisieren**) liest sie sofort neu ein. Mehr in [Verbindungen](08-verbindungen.md#container).

## Wo die Favoriten gespeichert sind {#speicherort}

In `~/Library/Application Support/Manta/favorites.json`. Passwörter stehen nicht darin: Mit **Passwort merken** landen sie im macOS-Schlüsselbund, siehe [Verbindungen](08-verbindungen.md#favoriten).
