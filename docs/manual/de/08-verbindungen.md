# Verbindungen {#verbindungen}

Manta öffnet Ordner auf Servern (SFTP, FTP, FTPS, WebDAV, SMB), in S3-Speichern und in Docker- und Kubernetes-Containern wie lokale Ordner. Du kopierst, bewegst und benennst dort genauso wie auf deinem Mac.

## So verbindest du dich mit einem Server {#verbinden}

1. Drücke **⌘K** (**Mit Server verbinden …**), klicke auf **Verbinden** in der Symbolleiste oder auf das **+** neben **Server** in der Seitenleiste.
2. Wähle oben das **Protokoll**: **SFTP**, **FTP**, **FTPS**, **WebDAV**, **SMB**, **S3** oder **Container**.
3. Fülle die Felder aus (siehe unten).
4. Optional: Klicke auf **Verbindung testen**. Manta meldet z. B. „Angemeldet. Benutzerordner: /home/anna“.
5. Klicke auf **Verbinden**.

Der Server öffnet sich im aktiven Bereich (der Dialog sagt dir, in welchem). Die Verbindung erscheint in der Seitenleiste unter **Server**.

![Dialog „Mit Server verbinden“ für SFTP](images/connect-sftp.png)

> [!TIP]
> Im Feld **Server** kannst du alles auf einmal eintippen: `benutzer@host:port`. Manta verteilt es auf die Felder.

## SFTP {#sftp}

SFTP ist der sichere Standard für Linux-Server und viele Hosting-Pakete.

| Feld | Bedeutung |
|---|---|
| **Server**, **Port** | Adresse des Servers; Port 22, wenn du nichts änderst. |
| **Benutzer** | Dein Benutzername auf dem Server. |
| **Anmelden mit** | **Passwort**, **Schlüsseldatei** (mit optionaler **Passphrase**, „nur bei verschlüsseltem Schlüssel“) oder **SSH-Agent** („Verwendet die Schlüssel, die im laufenden ssh-agent geladen sind“). |
| **Ordner** | Optional – sonst dein Benutzerordner auf dem Server. |

### Server-Schlüssel prüfen {#server-schluessel}

> [!WARNING]
> Manta prüft den Server-Schlüssel gegen deine Datei `~/.ssh/known_hosts` – dieselbe, die auch `ssh` im Terminal benutzt.
>
> - **Bekannter Server, passender Schlüssel:** Manta verbindet sich ohne Rückfrage.
> - **Unbekannter Server** (nicht in `known_hosts`): Manta zeigt den Dialog **Unbekannter Server** mit Server, Schlüsseltyp und Fingerabdruck (`SHA256:…`, wie ihn `ssh-keygen -lf` ausgibt). Vergleiche den Fingerabdruck mit dem, den dir der Administrator oder Hoster nennt. **Verbinden und merken** trägt den Schlüssel in `known_hosts` ein (`host` bzw. `[host]:port` bei einem anderen Port als 22) und verbindet sich; **Abbrechen** verbindet sich nicht.
> - **Geänderter Schlüssel:** Manta verbindet sich **nicht** und zeigt eine rote Warnung (möglicher Man-in-the-Middle-Angriff). Einen Knopf „Trotzdem verbinden“ gibt es absichtlich nicht. Das passiert auch nach einer Neuinstallation des Servers – frag beim Administrator nach dem richtigen Fingerabdruck. Stimmt er, entferne den alten Eintrag im Terminal mit dem Befehl aus dem Dialog (z. B. `ssh-keygen -R host`, **Befehl kopieren**) und verbinde dich erneut.
>
> Übertragungen, die sich nach einem Neustart im Hintergrund neu verbinden, fragen nie nach: Bei einem unbekannten Schlüssel warten sie auf **Verbinden**, bei einem geänderten schlagen sie fehl.

## FTP und FTPS {#ftp}

**FTP** überträgt alles unverschlüsselt, auch dein Passwort. Verwende es nur im lokalen Netz oder für öffentliche Server. Mit **Anonym** meldest du dich ohne Passwort an, sofern der Server das erlaubt.

**FTPS** ist FTP mit Verschlüsselung (explizites TLS, meist auf Port 21). Implizites FTPS auf Port 990 unterstützt Manta nicht.

### Der Datenkanal bei FTPS {#datenkanal}

FTP nutzt zwei Verbindungen: eine für die Anmeldung und Befehle, eine zweite („Datenkanal“) für Ordnerinhalte und Dateien. Manche Firmen-Firewalls lassen verschlüsselte Datenverbindungen nicht durch. Deshalb kannst du unter **Datenkanal** wählen:

| Einstellung | Verhalten |
|---|---|
| **Automatisch** (Standard) | Verschlüsselt. Blockiert das Netzwerk verschlüsselte Datenverbindungen, geht es unverschlüsselt weiter; die Anmeldung bleibt verschlüsselt. |
| **Erforderlich** | Immer verschlüsselt. Schlägt fehl, wenn das Netzwerk verschlüsselte Datenverbindungen blockiert. |
| **Aus** | Ordnerinhalte und Dateien werden unverschlüsselt übertragen; die Anmeldung bleibt verschlüsselt. |

Fällt Manta bei **Automatisch** auf einen unverschlüsselten Datenkanal zurück, siehst du das:

- Der Ort-Chip in der Pfadleiste zeigt ein Warnschild.
- Einmal erscheint der Hinweis „Datenverbindung unverschlüsselt (Netzwerk blockiert TLS) – Anmeldung bleibt verschlüsselt“.

> [!WARNING]
> Im Rückfall sind **Dateiinhalte und Dateinamen unverschlüsselt** unterwegs, nur Benutzername und Passwort bleiben geschützt. Überträgst du vertrauliche Daten, stelle den Datenkanal auf **Erforderlich** und nutze ein anderes Netz – oder SFTP.

Die Einstellung wird mit dem Favoriten gesichert. Siehe auch [Fehlerbehebung: FTP-Datenkanal](19-fehlerbehebung.md#ftp-datenkanal).

![FTPS-Dialog mit Datenkanal-Auswahl](images/connect-ftps-data-channel.png)

## S3 und S3-kompatibler Speicher {#s3}

Unter **S3** verbindest du dich mit Amazon S3 und kompatiblen Diensten. Buckets und Präfixe erscheinen als Ordner.

| Feld | Bedeutung |
|---|---|
| **Anbieter** | Amazon S3, Cloudflare R2, MinIO, Backblaze B2, Wasabi, DigitalOcean Spaces, Hetzner Object Storage oder **Anderer S3-kompatibler**. Die Vorlage füllt Endpunkt und Region aus. |
| **Account-ID** | Nur bei Cloudflare R2 (Cloudflare-Dashboard › R2 › Account-ID). |
| **Region**, **Endpunkt** | Der Endpunkt folgt meist der Region. Liegt ein Bucket in einer anderen Region, korrigiert Manta das selbst. |
| **Anmelden mit** | **AWS-Profil** (Schlüssel aus `~/.aws/credentials`), **Zugriffsschlüssel** (Schlüssel-ID und geheimer Schlüssel) oder **Öffentlich** (für öffentliche Buckets). |
| **Bucket** | Optional – sonst siehst du alle Buckets. |
| **Pfadbasierte Adressierung** | Nötig für MinIO und die meisten selbst gehosteten Server. |

![S3-Dialog mit Anbieter-Vorlage](images/connect-s3.png)

Gut zu wissen:

- Umbenennen und Bewegen innerhalb eines Buckets ist intern „kopieren und löschen“. Objekte über 5 GB lassen sich nicht umbenennen.
- Änderungsdatum und Zugriffsrechte lassen sich in S3 nicht setzen.
- Löschst du die letzte Datei eines „Ordners“, verschwindet der Ordner, wenn es keinen eigenen Ordner-Eintrag gibt.
- AWS-Profile mit SSO, `credential_process` oder Rollenwechsel (assume-role) gehen nicht, nur feste Schlüssel.
- Geht die Uhr deines Mac mehr als 15 Minuten falsch, lehnt S3 die Anfragen ab („Die Uhr deines Mac geht falsch“).

## WebDAV {#webdav}

**WebDAV** verbindet dich mit Nextcloud, ownCloud, NAS-Systemen (Synology, QNAP), Apache- oder nginx-Servern und anderen WebDAV-Diensten.

| Feld | Bedeutung |
|---|---|
| **Server**, **Port** | Die Adresse des Servers. Du kannst eine ganze URL einfügen, etwa `https://cloud.example.com/remote.php/dav/files/anna`; Manta füllt die Felder aus. |
| **Sicherheit** | **HTTPS** (Port 443) oder **HTTP (unverschlüsselt)** (Port 80). |
| **Benutzer**, **Anmelden mit** | **Passwort** oder **Anonym**. Manta beherrscht Basic- und Digest-Anmeldung. |
| **Servertyp** | **WebDAV-Server**, **Nextcloud** oder **ownCloud**. Nextcloud und ownCloud tragen den **WebDAV-Pfad** `/remote.php/dav/files/<Benutzername>` ein. |
| **WebDAV-Pfad** | Wo der WebDAV-Ordner auf dem Server beginnt, etwa `/dav`. Leer: die oberste Ebene des Servers. |
| **Ordner** | Optional – der Ordner darin, der sich öffnen soll. |

Gut zu wissen:

- Zertifikate prüft macOS, wie in Safari. Ein selbst signiertes Zertifikat funktioniert, sobald du ihm in der Schlüsselbundverwaltung vertraust.
- Abgebrochene Downloads gehen dort weiter, wo sie aufgehört haben. Abgebrochene Uploads beginnen die Datei neu (WebDAV kann nicht an eine Datei anhängen).
- Kopieren innerhalb desselben Servers nutzt die Kopierfunktion des Servers (kein Herunter- und wieder Hochladen).
- Änderungsdaten lassen sich setzen, wo der Server es erlaubt (Nextcloud, ownCloud); sonst bekommen Kopien die Zeit des Uploads.

> [!TIP]
> Mit Zwei-Faktor-Authentifizierung bei Nextcloud oder ownCloud meldest du dich mit einem **App-Passwort** an (Einstellungen → Sicherheit in der Weboberfläche), nicht mit deinem normalen Passwort.

> [!WARNING]
> Mit **HTTP (unverschlüsselt)** werden Passwort und Dateien unverschlüsselt übertragen. Verwende HTTP nur in einem Netzwerk, dem du vertraust.

![Verbindungsdialog für WebDAV mit der Nextcloud-Vorgabe](images/connect-webdav.png)

## SMB {#smb}

**SMB** öffnet Freigaben von Windows-PCs, NAS-Systemen und anderen Macs. Die Verbindung stellt macOS selbst her (genau wie **Mit Server verbinden** im Finder) und aktiviert die Freigabe in `/Volumes`; Manta zeigt sie dann wie einen lokalen Ordner.

| Feld | Bedeutung |
|---|---|
| **Server** | Name oder Adresse des Servers, etwa `nas.local`. `smb://server/freigabe/ordner` füllt alle Felder aus. |
| **Benutzer**, **Anmelden mit** | **Passwort** oder **Gast** (ohne Passwort – oder mit deinem Kerberos-Ticket bzw. dem Passwort aus deinem Schlüsselbund). |
| **Freigabe** | Die Freigabe, die sich öffnen soll. **Durchsuchen …** zeigt die Freigaben des Servers; ein Klick wählt eine aus. |
| **Ordner** | Optional – ein Ordner in der Freigabe. |

Gut zu wissen:

- Eine Freigabe, die schon aktiviert ist (zum Beispiel vom Finder), nutzt Manta so, wie sie ist; sie bleibt beim Trennen aktiv. Eine Freigabe, die Manta aktiviert hat, wirft Manta beim Trennen oder Beenden wieder aus.
- Der Ortschip zeigt zum Beispiel „SMB · nas.local/Team“.
- Abgebrochene Kopien gehen dort weiter, wo sie aufgehört haben. Verschwindet die Freigabe (Netzwerkwechsel, Ruhezustand), aktiviert Manta sie vor dem nächsten Versuch neu.
- Die Liste der Freigaben kommt von `smbutil`, ohne Passwort (Gast, Kerberos oder Schlüsselbund). Verlangt der Server dafür ein Passwort, gib den Namen der Freigabe einfach von Hand ein.

![Verbindungsdialog für SMB mit der Liste der Freigaben](images/connect-smb.png)

### Netzwerk-Volumes in der Seitenleiste {#netzwerk-volumes}

Freigaben, die du im Finder verbunden hast (SMB, AFP, NFS, WebDAV), erscheinen unter **Geräte** mit einem Netzwerk-Symbol. Fahre darüber und klicke auf das Auswerfen-Symbol (**Auswerfen**), um sie zu deaktivieren. Hast du etwas Neues verbunden, klicke auf den Pfeil neben **Geräte**.

## Docker und Kubernetes {#container}

Manta öffnet Dateien **in Containern** wie einen Ordner. Dazu nutzt Manta die Befehle `docker` bzw. `kubectl`, die auf deinem Mac installiert sein müssen.

### Über die Seitenleiste

Der schnellste Weg: Klicke im Abschnitt **Container** der Seitenleiste auf einen Container. Er öffnet sich in seinem Arbeitsverzeichnis.

### Über den Dialog

1. Drücke **⌘K** und wähle **Container**.
2. Wähle die **Laufzeit**: **Docker** oder **Kubernetes**.
3. Docker: Wähle den Container aus der Liste oder tippe Name oder ID. Kubernetes: Wähle **Kontext**, **Namespace**, **Pod** und gegebenenfalls den Container.
4. Klicke auf **Verbinden**.

Favoriten merken sich den Container **nach Namen**. So funktioniert ein Favorit auch, nachdem der Container neu erstellt wurde.

![Container-Auswahl im Verbindungsdialog](images/connect-container.png)

### Gestoppte Container und Images ohne Shell

- **Gestoppte Container** öffnen sich **schreibgeschützt** (Chip **Gestoppt**). Du kannst Dateien herauskopieren; zum Ändern starte den Container.
- Container **ohne Shell** (Distroless- oder Scratch-Images) sind ebenfalls schreibgeschützt.
- Ordner eines gestoppten Containers liest Manta als Ganzes; ist das mehr als 512 MB, meldet Manta „zu groß“. Starte dann den Container.

### Eine Shell im Container

Rechtsklick → **Shell im Container öffnen** (auch in der Befehlspalette) öffnet im Terminal unter dem Bereich eine Shell im Container, direkt im angezeigten Ordner. Siehe [Terminal](11-terminal.md#container-ssh).

> [!NOTE]
> Manta ändert nur Dateien, mit denen du selbst etwas tust. Container startet, stoppt oder löscht Manta nie.

## Verbindungen als Favoriten {#favoriten}

1. Schalte im Verbindungsdialog **Zu Favoriten hinzufügen** ein.
2. Gib optional einen Namen für die Seitenleiste ein.
3. Optional: **Passwort merken** (bzw. **Passphrase merken**, **Geheimen Schlüssel merken**).

> [!NOTE]
> Gemerkte Passwörter, Passphrasen und geheime S3-Schlüssel speichert Manta im **macOS-Schlüsselbund** (Anmeldung, Objekt „com.manta-filemanager.manta.server“, Account z. B. `sftp://benutzer@host:22`), nicht in `favorites.json`. Die Datei vermerkt nur, dass es einen Eintrag gibt. Manta liest das Passwort erst beim Verbinden; macOS kann dabei einmal fragen, ob Manta auf den Schlüsselbund zugreifen darf.
>
> Ältere Versionen haben gemerkte Passwörter unverschlüsselt in `favorites.json` gespeichert. Beim ersten Start verschiebt Manta sie in den Schlüsselbund und schreibt die Datei ohne sie neu. Klappt das nicht (Schlüsselbund gesperrt oder Zugriff verweigert), bleibt die Datei unverändert und Manta versucht es beim nächsten Start erneut.
>
> Entfernst du einen Favoriten, löscht Manta auch seinen Schlüsselbund-Eintrag (außer ein anderer Favorit für denselben Server-Account nutzt ihn noch). Beim Bearbeiten zeigt das Passwortfeld „im Schlüsselbund gesichert“; lässt du es leer, bleibt das gespeicherte Passwort.

Einen Favoriten änderst du mit Rechtsklick → **Verbindung bearbeiten …** (Dialog „Verbindung bearbeiten“).

## Verbindung trennen {#trennen}

Fahre in der Seitenleiste unter **Server** über die Verbindung und klicke auf das Auswerfen-Symbol (**Verbindung trennen**). Bereiche, die den Server zeigten, melden „Getrennt“. Bei einer SMB-Freigabe wirft das Trennen auch die Freigabe aus, wenn Manta sie aktiviert hat.

## Wenn es nicht klappt {#fehler}

Manta übersetzt technische Fehler in verständliche Meldungen mit einem Tipp. Die häufigsten:

| Meldung | Was du tun kannst |
|---|---|
| „‚host‘ wurde nicht gefunden“ | Überprüfe die Serveradresse auf Tippfehler. |
| „host hat die Verbindung auf Port … abgelehnt“ | Stimmen Port und Protokoll? Läuft der Server? |
| „host antwortet nicht“ | Netzwerk, VPN und Firewall prüfen. |
| „Der Server hat die Anmeldedaten nicht akzeptiert“ | Benutzername und Passwort prüfen oder eine andere Anmeldemethode wählen. |
| „Kein verwendbarer Schlüssel im ssh-agent“ | Mit `ssh-add` einen Schlüssel hinzufügen. |
| „Es konnte keine sichere Verbindung hergestellt werden“ | Der Server kann auf diesem Port kein FTPS, oder sein Zertifikat ist nicht vertrauenswürdig. |
| „Docker läuft nicht“ | Docker Desktop starten. |
| „kubectl ist nicht installiert“ | `brew install kubectl` |

Weitere Hilfe: [Fehlerbehebung](19-fehlerbehebung.md).

## Grenzen {#grenzen}

- Kein implizites FTPS (Port 990).
- Kubernetes: kein Port-Forwarding, keine temporären Debug-Container; Pods brauchen `sh`.
- Schreiben in Container geht über `cat`; neue Dateien gehören dem Benutzer, mit dem `exec` läuft.
- Google Drive & Co. verbindest du nicht hier, sondern über die Cloud-Ordner deines Mac, siehe [Cloud-Speicher](09-cloud.md). Manta bringt keine eigene Google-Anmeldung mit; nur wer in den Einstellungen einen **eigenen** OAuth-Client einträgt, bekommt hier zusätzlich „Google Drive“, siehe [Google Drive für Experten](09-cloud.md#experte).
- WebDAV: keine Sperren (`LOCK`); abgebrochene Uploads beginnen neu.
- SMB braucht die SMB-Unterstützung von macOS; Manta hat keinen eigenen SMB-Client. Mit einem anderen als dem Standardport klappt die Liste der Freigaben eventuell nicht.
