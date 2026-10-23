# Git {#git}

Liegt ein Ordner in einem Git-Repository, zeigt Manta den Status jeder Datei direkt in der Liste, den Branch in der Pfadleiste und ein Git-Panel zum Vormerken und Committen. Dafür muss `git` auf deinem Mac installiert sein (z. B. mit den Xcode Command Line Tools).

![Dateiliste mit Git-Abzeichen und Git-Panel](images/git-panel.png)

## Status in der Dateiliste {#abzeichen}

Neben geänderten Dateien steht ein Abzeichen. Fährst du mit der Maus darüber, erklärt ein Tooltip den Status:

- **Geändert, nicht vorgemerkt** / **Zum Commit vorgemerkt** / **Geändert, teilweise vorgemerkt**
- **Hinzugefügt, zum Commit vorgemerkt**
- **Nicht verfolgt** (neu, Git kennt die Datei noch nicht)
- **Gelöscht, nicht vorgemerkt** / **Löschung zum Commit vorgemerkt**
- **Umbenannt von …** / **Kopiert von …**
- **Merge-Konflikt**
- **Durch .gitignore ignoriert** – solche Dateien erscheinen abgeblendet.

Ordner zeigen, wie viele geänderte Dateien darin liegen („3 geänderte Dateien darin“).

## Branch in der Pfadleiste {#chip}

Ein Chip in der Pfadleiste zeigt den aktuellen Branch. Der Tooltip verrät mehr:

- welchem Upstream-Branch er folgt und ob du voraus oder zurück bist („Folgt origin/main (2 voraus)“),
- den letzten Commit,
- „Losgelöster HEAD bei …“, wenn kein Branch ausgecheckt ist.

Läuft gerade ein Merge, Rebase, Cherry-Pick, Revert oder Bisect, zeigt der Chip das an. Ein Klick auf den Chip öffnet das Git-Panel.

## Das Git-Panel {#seitenleiste}

Öffnen und schließen: **⇧⌘B** (**Git-Panel ein-/ausblenden**) oder Klick auf den Branch-Chip.

Das Panel zeigt:

- oben Branch, Upstream und wie viele Commits zum Pushen oder Pullen anstehen,
- die Gruppen **Konflikte**, **Vorgemerkt**, **Änderungen** und **Nicht verfolgt**,
- ein Feld für die **Commit-Nachricht**,
- die **Letzten Commits**.

### So committest du Änderungen {#commit}

1. Öffne das Git-Panel mit **⇧⌘B**.
2. Merke die Dateien vor: **Vormerken** bei einer Datei oder **Alle vormerken** bei der Gruppe **Änderungen**.
3. Klicke auf eine Datei, um ihren Diff zu sehen.
4. Tippe eine **Commit-Nachricht**.
5. Klicke auf **Commit** oder drücke **⌘↩**.

Manta meldet „Commit abc1234 erstellt: …“.

Zum Zurücknehmen: **Vormerkung aufheben** bzw. **Alle zurücknehmen**.

### Konflikte lösen {#konflikte}

Bei einem Merge-Konflikt stehen die betroffenen Dateien unter **Konflikte**. Löse den Konflikt im Editor, dann **Als gelöst markieren (vormerken)**. Sind alle gelöst, sagt das Panel „Committe, um den Merge abzuschließen“. Bei Rebase, Cherry-Pick und Revert zeigt sie den passenden `--continue`-Befehl für das Terminal.

## Aktualisieren {#aktualisieren}

Manta aktualisiert den Git-Status automatisch. Falls nötig: **Git-Status aktualisieren** in der Befehlspalette oder die Aktualisieren-Taste im Panel.

## Grenzen {#grenzen}

- Git funktioniert nur für **lokale** Ordner („Der Git-Status ist für Ordner auf diesem Mac verfügbar, nicht auf Servern“).
- Push, Pull, Branches wechseln und Merge erledigst du im [Terminal](11-terminal.md) oder in einer Git-App. Das Panel zeigt den Status, merkt vor und committet.
