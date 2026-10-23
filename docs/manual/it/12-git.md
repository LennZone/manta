# Git {#git}

Quando una cartella si trova in un repository Git, Manta mostra lo stato di ogni file direttamente nell’elenco, il branch nella barra del percorso e un pannello Git per preparare le modifiche ed eseguire i commit. `git` deve essere installato sul tuo Mac (ad esempio con gli Xcode Command Line Tools).

![Elenco dei file con contrassegni Git e il pannello Git](images/git-panel.png)

## Stato nell’elenco dei file {#abzeichen}

I file modificati hanno un contrassegno. Passaci sopra con il puntatore e un suggerimento spiega lo stato:

- **Modificato, non preparato** / **Preparato per il commit** / **Modificato, preparato in parte**
- **Aggiunto, preparato per il commit**
- **Non tracciato** (nuovo, Git non conosce ancora il file)
- **Eliminato, non preparato** / **Eliminazione preparata per il commit**
- **Rinominato da …** / **Copiato da …**
- **Conflitto di merge**
- **Ignorato da .gitignore**: questi file sono attenuati.

Le cartelle indicano quanti file modificati contengono (“3 file modificati all’interno”).

## Il branch nella barra del percorso {#chip}

Un indicatore nella barra del percorso mostra il branch attuale. Il suo suggerimento ti dice di più:

- quale branch upstream traccia e se sei avanti o indietro (“Traccia origin/main (2 avanti)”),
- l’ultimo commit,
- “HEAD scollegato su …” quando non è attivo nessun branch.

Se è in corso un merge, un rebase, un cherry-pick, un revert o un bisect, l’indicatore lo mostra. Un clic sull’indicatore apre il pannello Git.

## Il pannello Git {#seitenleiste}

Per mostrarlo o nasconderlo: **⇧⌘B** (**Mostra/nascondi pannello Git**) o un clic sull’indicatore del branch.

Il pannello mostra:

- in alto il branch, l’upstream e quanti commit ci sono da inviare o da ricevere,
- i gruppi **Conflitti**, **Preparate**, **Modifiche** e **Non tracciati**,
- un campo per il **Messaggio di commit**,
- i **Commit recenti**.

### Eseguire il commit delle modifiche {#commit}

1. Apri il pannello Git con **⇧⌘B**.
2. Prepara i file: **Prepara** su un file, oppure **Prepara tutto** sul gruppo **Modifiche**.
3. Fai clic su un file per vederne il diff.
4. Scrivi un **Messaggio di commit**.
5. Fai clic su **Commit** o premi **⌘↩**.

Manta conferma con “Commit abc1234 creato: …”.

Per annullare la preparazione: **Annulla preparazione** o **Annulla tutto**.

### Risolvere i conflitti {#konflikte}

Durante un conflitto di merge i file interessati sono elencati sotto **Conflitti**. Risolvi il conflitto nel tuo editor, poi scegli **Segna come risolto (prepara)**. Quando sono tutti risolti, il pannello indica “Esegui il commit per completare il merge”. Per rebase, cherry-pick e revert mostra il comando `--continue` corrispondente da usare nel terminale.

## Aggiornare {#aktualisieren}

Manta aggiorna automaticamente lo stato Git. Se necessario: **Aggiorna stato Git** nella palette dei comandi, oppure il pulsante di aggiornamento nel pannello.

## Limiti {#grenzen}

- Git funziona solo con le cartelle **locali** (“Lo stato Git è disponibile per le cartelle su questo Mac, non sui server.”).
- Push, pull, cambio di branch e merge si fanno nel [Terminale](11-terminal.md) o in un’app Git. Il pannello mostra lo stato, prepara le modifiche ed esegue i commit.
