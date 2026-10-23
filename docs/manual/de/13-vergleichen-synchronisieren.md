# Vergleichen & Synchronisieren {#vergleichen}

**Vergleichen und synchronisieren** zeigt, worin sich zwei Ordner unterscheiden, und gleicht sie auf Wunsch an – lokal, zwischen Mac und Server oder zwischen zwei Servern. Das Fenster vergleicht zwei Bereiche.

![Fenster „Vergleichen und synchronisieren“](images/compare-sync.png)

## So vergleichst du zwei Ordner {#zwei-bereiche}

1. Öffne den einen Ordner im linken Bereich, den anderen im rechten.
2. Drücke **⇧⌘S** (**Vergleichen und synchronisieren …**) oder klicke auf die Taste in der Symbolleiste.
3. Wähle die Optionen (siehe unten) und klicke auf **Erneut vergleichen**.

Die Liste zeigt jedes Objekt mit seinem Status:

| Status | Bedeutung |
|---|---|
| **Nur links** / **Nur rechts** | Gibt es nur auf einer Seite. |
| **Links neuer** / **Rechts neuer** | Auf beiden Seiten, aber eine ist neuer (Änderungsdatum). |
| **Verschieden** | Gleiches Datum, aber andere Größe. |
| **Identisch** | Gleiches Datum und gleiche Größe. |

Manta vergleicht **Änderungsdatum und Größe**, nicht den Inhalt Byte für Byte.

Die Status-Legende über der Liste ist zugleich ein Filter: Klick auf einen Status blendet ihn aus („Zum Ausblenden klicken“), ein zweiter Klick wieder ein.

## Mit drei oder mehr Bereichen {#a-b}

Hast du mehr als zwei Bereiche, wählst du, welche zwei verglichen werden:

- Im Fenster heißen die beiden Seiten statt „Links“ und „Rechts“ **Bereich A** und **Bereich B**. Oben rechts in jeder Karte wählst du per Aufklappmenü, welcher Bereich das ist („Wähle den Bereich, der verglichen wird“). Die Bereiche sind von links nach rechts nummeriert, z. B. „2 · Downloads“; der Bereich der anderen Seite ist mit „(A)“ bzw. „(B)“ markiert. Wählst du ihn, tauschen A und B die Plätze.
- Vorausgewählt sind der aktive Bereich und sein [Zielbereich](02-bereiche-tabs.md#zielbereich).
- Solange das Fenster offen ist, erscheinen in der Pfadleiste jedes Bereichs kleine **A**/**B**-Tasten („Diesen Bereich als A vergleichen“). Die gewählte Rolle ist farbig gefüllt.

Die Status heißen dann **Nur in A**, **Nur in B**, **A neuer** und **B neuer**; die Größenspalten **Bereich A** und **Bereich B**.

![Vergleichen mit drei Bereichen: Auswahl von Bereich A und B](images/compare-sync-ab.png)

**A steht im Fenster immer links, B rechts.** Richtung und Aktionen behalten deshalb ihre Namen: **Links nach rechts spiegeln** heißt „A nach B“, „Kopieren →“ kopiert von A nach B, „Rechts löschen“ löscht in B.

## Optionen {#optionen}

- **Mit Unterordnern** – vergleicht auch den Inhalt aller Unterordner.
- **Unsichtbare Dateien** – bezieht Dateien wie `.env` oder `.DS_Store` ein.
- **Richtung** (die drei Tasten → ↔ ←):
  - **→ Links nach rechts spiegeln**: Rechts wird wie links.
  - **↔ Neuere Datei gewinnt**: In beide Richtungen; die jeweils neuere Datei wird übertragen.
  - **← Rechts nach links spiegeln**: Links wird wie rechts.
- **Überzählige im Ziel löschen** – nur beim Synchronisieren in eine Richtung. Löscht im Ziel, was es in der Quelle nicht gibt.

Die Spalte **Aktion** zeigt vor dem Start, was passieren wird: „Kopieren →“, „← Kopieren“, „Rechts löschen“ oder „Links löschen“.

## So synchronisierst du {#synchronisieren}

1. Vergleiche die Ordner wie oben.
2. Wähle die Richtung und prüfe die Spalte **Aktion**.
3. Klicke auf **N Objekte synchronisieren**.

Die Übertragungen laufen im Hintergrund („Synchronisierung gestartet: 12 Übertragungen in der Warteschlange“). Den Fortschritt siehst du unter **Übertragungen** (**⌥⌘L**).

Sind die Ordner gleich, meldet das Fenster „Beide Ordner sind synchron“.

> [!WARNING]
> Synchronisieren **überschreibt** Dateien im Ziel ohne weitere Nachfrage, und **Überzählige im Ziel löschen** löscht endgültig (siehe [Löschen](05-dateien.md#loeschen)). Prüfe die Spalte **Aktion**, bevor du startest.

Regeln, die dich schützen:

- Beim Spiegeln wird eine **neuere** Datei im Ziel nie überschrieben.
- Bei **Neuere Datei gewinnt** werden Dateien mit gleichem Datum, aber anderer Größe übersprungen, weil unklar ist, welche gilt. **Überzählige im Ziel löschen** gilt hier nicht.
- Ist ein Name auf einer Seite ein Ordner und auf der anderen eine Datei, wird er übersprungen.
- Kann ein Ordner nicht gelesen werden, bricht der Vergleich ab, statt mit unvollständigen Daten etwas Falsches zu löschen.

## Das Fenster {#fenster}

**Vergleichen und synchronisieren** ist ein eigenes macOS-Fenster. Schließen: rote Fenstertaste oder **⌘W** (nicht esc). Manta merkt sich seine Position, solange die App läuft.

Mit nur einem Bereich ist der Befehl ausgeschaltet („Benötigt zwei Bereiche – ⌘D teilt den Bereich“).
