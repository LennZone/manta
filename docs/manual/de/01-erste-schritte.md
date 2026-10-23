# Erste Schritte {#erste-schritte}

Manta ist ein Dateimanager für den Mac mit mehreren Bereichen nebeneinander. Dieses Kapitel zeigt, wie du Manta installierst, was beim ersten Start passiert und warum macOS dich um Erlaubnis fragt.

![Manta mit zwei Bereichen und Seitenleiste](images/overview-two-panes.png)

## Systemvoraussetzungen {#voraussetzungen}

- macOS 11 (Big Sur) oder neuer.
- Ein Mac mit Apple-Chip (M1 oder neuer).

## So installierst du Manta {#installieren}

1. Öffne die Datei `Manta-<Version>.dmg` per Doppelklick.
2. Im Fenster, das sich öffnet, ziehst du **Manta** auf den Ordner **Programme**.
3. Wirf das Disk-Image aus (im Finder auf das Auswerfen-Symbol klicken).
4. Starte Manta aus dem Ordner **Programme**, z. B. über Launchpad oder Spotlight.

> [!IMPORTANT]
> Starte Manta immer aus dem Ordner **Programme**, nicht direkt aus dem Disk-Image. Nur dann kann sich Manta selbst aktualisieren ([Updates](17-updates.md)) und als Standard-Dateimanager eingerichtet werden.

![Installationsfenster des Disk-Images](images/dmg-window.png)

## Der erste Start: Gatekeeper {#gatekeeper}

Ist deine Manta-Version nicht von Apple beglaubigt (notarisiert), blockiert macOS den ersten Start mit einer Meldung wie „„Manta“ wurde nicht geöffnet“. Das ist eine Schutzfunktion von macOS (Gatekeeper), kein Fehler in Manta.

So öffnest du Manta trotzdem:

1. Klicke in der Meldung auf **Fertig** (nicht auf „In den Papierkorb legen“).
2. Öffne **Systemeinstellungen** → **Datenschutz & Sicherheit**.
3. Scrolle nach unten zum Abschnitt **Sicherheit**. Dort steht ein Hinweis zu Manta.
4. Klicke auf **Dennoch öffnen** und bestätige mit deinem Passwort oder Touch ID.

Unter älteren macOS-Versionen (vor macOS 15) reicht es, Manta im Finder mit gedrückter Control-Taste anzuklicken und **Öffnen** zu wählen.

Das musst du nur einmal tun. Mehr dazu unter [Fehlerbehebung](19-fehlerbehebung.md#gatekeeper).

## Daten von RustLift {#rustlift}

Manta hieß früher RustLift. Hast du RustLift benutzt, übernimmt Manta beim ersten Start deine Einstellungen, Favoriten und Plugin-Daten. Eine Mitteilung „Daten von RustLift übernommen“ bestätigt das.

## Warum macOS um Erlaubnis fragt {#berechtigungen}

macOS schützt bestimmte Ordner und Funktionen. Jede App muss einmal fragen, bevor sie darauf zugreift – auch ein Dateimanager. Die Fragen kommen nicht alle beim Start, sondern erst, wenn du den Ordner zum ersten Mal öffnest oder die Funktion zum ersten Mal brauchst.

| macOS fragt nach … | Wann | Warum Manta das braucht |
|---|---|---|
| Ordner „Schreibtisch“, „Dokumente“, „Downloads“ | Wenn du den Ordner zum ersten Mal öffnest | Um die Dateien darin anzuzeigen und zu verwalten. |
| Wechseldatenträger, Netzwerkvolumes | Beim ersten Zugriff auf einen USB-Stick oder eine Netzwerkfreigabe | Dasselbe für externe Laufwerke und Server-Freigaben. |
| Lokales Netzwerk | Bei der ersten Verbindung zu einem Server in deinem Netz | Für SFTP-, FTP- und S3-Server in deinem lokalen Netzwerk (z. B. ein NAS). |
| Automation (Finder, Terminal) | Wenn eine Funktion Finder oder Terminal steuert | Z. B. um Objekte in den Papierkorb zu legen (Duplikate), Dateien im Finder zu zeigen oder Schnellaktionen wie „In Terminal.app öffnen“ auszuführen. |

Wenn du „Nicht erlauben“ klickst, zeigt Manta beim Öffnen des Ordners einen Hinweis. Du kannst die Erlaubnis jederzeit nachholen: **Systemeinstellungen** → **Datenschutz & Sicherheit** → **Dateien und Ordner** (bzw. **Automation** oder **Lokales Netzwerk**).

> [!TIP]
> Manche Ordner (z. B. `~/Library/Mail` oder die Time-Machine-Daten) lassen sich nur mit **Festplattenvollzugriff** öffnen. Den erteilst du unter **Systemeinstellungen** → **Datenschutz & Sicherheit** → **Festplattenvollzugriff**. Das ist optional; die meisten Leute brauchen es nicht.

> [!NOTE]
> Manta sammelt keine Nutzungsdaten. Ins Internet geht Manta nur für die Verbindungen, die du selbst öffnest, und für die Suche nach Updates (siehe [Updates und Datenschutz](17-updates.md#datenschutz)).

## Standard-Dateimanager {#standard-dateimanager}

Bei den ersten Starts fragt Manta: **Manta als Standard-Dateimanager verwenden?**

- **Als Standard festlegen**: „Im Finder anzeigen“ in anderen Apps öffnet dann Manta, ebenso Ordner, die andere Apps oder das Terminal öffnen (z. B. `open ~/Downloads`).
- **Nicht jetzt**: Manta fragt bei einem späteren Start noch einmal (höchstens dreimal).
- **Nicht mehr fragen**: Manta fragt nie wieder.

Du kannst das jederzeit ändern unter **Einstellungen** → **Allgemein** → **Standard-Dateimanager** oder in der Befehlspalette mit **Standard-Dateimanager …**.

![Frage nach dem Standard-Dateimanager](images/default-file-manager-prompt.png)

Was macOS dabei zulässt – und was nicht:

- Der Schreibtisch, das Finder-Symbol im Dock und Doppelklicks in Finder-Fenstern bleiben **immer** beim Finder. macOS erlaubt keiner App, den Finder ganz zu ersetzen.
- Ab macOS 26 gibt macOS das Öffnen von Ordnern durch andere Apps keiner App außer dem Finder. Dann übernimmt Manta nur „Im Finder anzeigen“; die Einstellungen sagen dir das.
- Ausschalten gibt die Rolle an die vorherige App zurück (z. B. einen anderen Dateimanager) oder an den Finder.
- Die Funktion gibt es nur, wenn Manta aus **Programme** (oder `~/Applications`) läuft.

## Das Fenster im Überblick {#ueberblick}

- **Symbolleiste** oben: Neuer Ordner, Umbenennen, Löschen, Kopieren/Bewegen in den anderen Bereich, unsichtbare Dateien, Vergleichen und synchronisieren, Übertragungen, Verbinden, Befehle und Einstellungen.
- **Seitenleiste** links: Favoriten, Orte, Geräte, Server und Container (siehe [Favoriten & Seitenleiste](07-favoriten-seitenleiste.md)).
- **Bereiche**: eine oder mehrere Dateilisten nebeneinander, jede mit Tabs und Pfadleiste (siehe [Bereiche & Tabs](02-bereiche-tabs.md)).
- **Statusleiste** unten: Anzahl der Objekte, Auswahl, laufende Übertragungen, die Systemauslastung (CPU, GPU, RAM; siehe [Systemauslastung](14-plugins.md#systemauslastung)) und ein Hinweis auf die Befehlspalette.

## Die Befehlspalette {#befehlspalette}

Jeder Befehl in Manta ist über die **Befehlspalette** erreichbar – auch die der Plugins.

1. Drücke **⇧⌘P** (oder **⌘P**).
2. Tippe ein paar Buchstaben, z. B. `neu ord` oder `sftp`. Englische und deutsche Begriffe funktionieren beide.
3. Wähle mit ↑ und ↓ und drücke ↩.

Die Palette zeigt neben jedem Befehl sein Tastenkürzel. So lernst du die Kürzel nebenbei.

![Befehlspalette mit Suchbegriff](images/command-palette.png)

> [!NOTE]
> Manta hat in der Menüleiste nur das Menü **Manta** (Über Manta, Nach Updates suchen …, Einstellungen …, Beenden). Alle anderen Befehle findest du in der Symbolleiste, im Kontextmenü (Rechtsklick) und in der Befehlspalette.

## Nächste Schritte

- [Bereiche & Tabs](02-bereiche-tabs.md): mit mehreren Bereichen arbeiten.
- [Dateien verwalten](05-dateien.md): kopieren, bewegen, umbenennen, löschen.
- [Tastenkürzel](18-tastenkuerzel.md): alle Kürzel auf einen Blick.
