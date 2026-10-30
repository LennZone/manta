# Scorciatoie da tastiera {#tastenkuerzel}

Tutte le scorciatoie di Manta a colpo d’occhio, raggruppate per argomento. La palette dei comandi (**⇧⌘P**) mostra la scorciatoia accanto a ogni comando, anche per i comandi dei plug-in. Anche la barra dei menu elenca ogni comando con la sua scorciatoia (**File**, **Modifica**, **Vista**, **Vai**, **Finestra**), come il Finder; così le app che mostrano le scorciatoie dell’app attiva trovano tutte quelle di Manta. Un clic su un comando di menu lo esegue.

Simboli: **⌘** Comando · **⇧** Maiuscole · **⌥** Opzione · **⌃** Controllo · **↩** A capo · **⌫** Elimina (backspace) · **⌦** Cancella in avanti · **⇥** Tab

## Generali {#allgemein}

| Scorciatoia | Comando |
|---|---|
| ⇧⌘P o ⌘P | Palette dei comandi |
| ⌘, | Impostazioni… |
| ⌘? | Apri il manuale (questo manuale; nella finestra Aiuto: il campo di ricerca) |
| ⌘K | Connessione al server… |
| ⌥⌘L | Mostra trasferimenti |
| ⇧⌘S | Confronta e sincronizza… |
| ⌘H | Nascondi Manta |
| ⌥⌘H | Nascondi altre |
| ⌘Q | Esci da Manta |

## Pannelli e schede {#bereiche}

| Scorciatoia | Comando |
|---|---|
| ⇥ / ⇧⇥ | Cambia pannello (al successivo o, con ⇧, al precedente, ricominciando dal primo) |
| ⌥⌘← / ⌥⌘→ | Attiva pannello a sinistra / a destra |
| ⌘D | Dividi pannello |
| ⇧⌘D | Chiudi pannello |
| ⌘T | Nuova scheda |
| ⌘W | Chiudi scheda (in Impostazioni, Confronta e sincronizza e nell’anteprima: chiude quella finestra) |

## Viste {#ansichten}

| Scorciatoia | Comando |
|---|---|
| ⌘1 | Come icone |
| ⌘2 | Come elenco |
| ⌘3 | Come colonne |
| ⌘4 | Come galleria |
| ⌘ + scorrimento o pizzico | Cambia la dimensione delle icone (vista Icone) |
| ← / → | Vista Colonne: va alla colonna a sinistra / apre la cartella selezionata come colonna successiva |
| ← / → | Viste Icone e Galleria: elemento precedente / successivo |

Le scorciatoie agiscono sul pannello attivo. Vedi [Viste](03-ansichten.md).

## Navigazione {#navigation}

| Scorciatoia | Comando |
|---|---|
| ↩ o ⌘↓ | Apri (gli archivi come cartelle) |
| ⌥↩ | Apri con l’app predefinita |
| ⌘↑ o ⌫ | Cartella superiore |
| ⌘[ o ⌘← | Indietro |
| ⌘] o ⌘→ | Avanti |
| ⌘L | Modifica percorso |
| ⇧⌘G | Vai al percorso… |
| ⌘R | Aggiorna |
| ⇧⌘. | Mostra/nascondi file nascosti |
| ⌘F | Filtra file |
| ⌃⌘T | Aggiungi cartella ai Preferiti |

## Selezione {#auswahl}

| Scorciatoia | Effetto |
|---|---|
| ↑ / ↓ | Elemento precedente / successivo |
| ⇧↑ / ⇧↓ | Estendi la selezione |
| Pagina su / Pagina giù | 20 righe su / giù |
| Inizio / Fine | Primo / ultimo elemento |
| ⌘A | Seleziona tutto |
| esc | Annulla la selezione (o chiudi l’anteprima) |
| Digitare lettere | Salta al nome corrispondente |

## File {#dateien}

| Scorciatoia | Comando |
|---|---|
| Barra spaziatrice o ⌘Y | Visualizzazione rapida / anteprima (barra spaziatrice o esc la chiude) |
| ⌘I | Ottieni informazioni |
| F5 | Copia nell’altro pannello… |
| F6 | Sposta nell’altro pannello… |
| F2 | Rinomina… (più elementi: rinomina in blocco) |
| ⇧⌘R | Rinomina in blocco… |
| ⇧⌘N | Nuova cartella… |
| ⌥⌘N | Nuovo file… |
| ⌘⌫ o ⌦ | Sposta nel Cestino (sui server: elimina definitivamente dopo una conferma, vedi [Eliminare](05-dateien.md#loeschen)) |
| ⌥⌘⌫ | Elimina immediatamente… (definitivo, dopo una conferma) |
| ⇧⌘⌫ | Svuota il Cestino… (dopo una conferma, vedi [Cestino](07-favoriten-seitenleiste.md#papierkorb)) |
| ⌘Z | Annulla (sposta nel Cestino, rinomina, nuova cartella, nuovo file; vedi [Annulla](05-dateien.md#widerrufen)) |
| ⇧⌘Z | Ripeti |
| ⌥⌘C | Comprimi… |
| ⌥⌘E | Estrai qui |
| ⌘ durante il trascinamento | Preseleziona Sposta invece di Copia (senza tasti è preselezionato Copia; ⌥ preseleziona esplicitamente Copia) |

> [!NOTE]
> Su molte tastiere Mac devi tenere premuto **fn** (🌐) per **F2**, **F5** e **F6**, a meno che tu non abbia attivato “Usa i tasti F1, F2, ecc. come tasti funzione standard” in Impostazioni di Sistema → **Tastiera**.

## Plug-in {#plugins}

| Scorciatoia | Comando | Plug-in |
|---|---|---|
| ⌃` | Mostra/nascondi Terminale | Terminale |
| ⇧⌘B | Mostra/nascondi pannello Git | Git |
| ⌘↩ | Commit (nel pannello Git) | Git |
| ⌥⌘U | Mostra utilizzo disco | Dimensioni cartelle |
| ⌥⌘M | Mostra/nascondi anteprima Markdown | Anteprima Markdown |
| ⌥⌘R | Esegui di nuovo l’ultimo script | Info progetto |

Le tue azioni rapide possono avere una scorciatoia propria (il campo `shortcut` in `actions.json`).

## Nelle finestre di dialogo e nelle finestre {#dialoge}

| Scorciatoia | Effetto |
|---|---|
| ↩ | Conferma (Copia, Crea, Rinomina …) |
| esc | Annulla la finestra di dialogo |
| ⌘W | Chiude Impostazioni, Confronta e sincronizza o l’anteprima |

**Palette dei comandi**: ↑ ↓ per scegliere · ↩ per eseguire · esc per chiudere

**Vai al percorso**: ↩ apre in una nuova scheda · ⌘↩ apre in questa scheda · ⇥ completa · esc annulla

## Quando le scorciatoie non funzionano {#fokus}

- In un **campo di testo** (filtro, percorso, campi delle finestre di dialogo) funzionano solo **⇧⌘P**, **⇧⌘G**, **⌘,** e **⌘?**; tutti gli altri tasti appartengono al campo.
- Nel **terminale**, **⌘C**, **⌘V**, **⌘A**, **⌘K**, **⌘←**, **⌘→**, **⌘⌫** e **⌘Z** appartengono al terminale. Fai clic nell’elenco dei file per restituire la tastiera a Manta.
- Nelle finestre **Impostazioni**, **Confronta e sincronizza** e **anteprima** funzionano solo le loro scorciatoie.

## Uso solo da tastiera {#tastatur}

Manta si usa interamente da tastiera.

| Tasto | Effetto |
|---|---|
| ⌃F5 | Porta il focus della tastiera sui controlli, a partire dalla barra strumenti (come «Sposta il focus sulla barra strumenti» di macOS) |
| ⇥ / ⇧⇥ | Nei controlli: successivo / precedente – barra strumenti → barra laterale → pannelli (barra del percorso, intestazioni di colonna, elenco dei file) → barra di stato, poi da capo |
| ↑ ↓ ← → | Nella barra laterale, nei dialoghi e negli elenchi di opzioni: al controllo successivo in quella direzione |
| Spazio / ↩ | Premi il pulsante, l’opzione o l’interruttore con il focus |
| esc | Esci dai controlli: l’elenco dei file riprende la tastiera; nei dialoghi: annulla |
| ⇥ / ⇧⇥ nell’elenco dei file | Al pannello successivo / precedente |

- L’**elenco dei file** è un’unica fermata di tabulazione per pannello (non ogni riga). Quando ha il focus funzionano tutte le sue abbreviazioni: frecce, ↩, Spazio, digitazione per saltare.
- Un **anello di focus** nel colore di evidenziazione mostra quale controllo ha la tastiera. I clic del mouse non lo mostrano.
- I **dialoghi** trattengono il focus: ⇥ si sposta solo tra i loro campi e pulsanti. Alla chiusura (anche Impostazioni) il focus torna dov’era.
- Con la **navigazione da tastiera** attiva in Impostazioni di Sistema (Tastiera) tutto funziona allo stesso modo; anche lì ⌃F5 è «Sposta il focus sulla barra strumenti».

## Differenze rispetto al Finder {#finder}

| Scorciatoia | Finder | Manta |
|---|---|---|
| ⌘D | Duplica | Dividi pannello |
| ⇧⌘D | Apri la Scrivania | Chiudi pannello |
| ⇧⌘P | Mostra anteprima | Palette dei comandi |
| ⌘R | Mostra originale | Aggiorna |
| ⌥⌘C | Copia il nome del percorso | Comprimi… |
| ⌥⌘L | Apri Download | Mostra trasferimenti |
| ⌫ | – | Cartella superiore |

Come nel Finder: **⌘⌫** sposta nel Cestino, **⌥⌘⌫** elimina immediatamente, **⇧⌘⌫** svuota il Cestino, **⌘T** apre una nuova scheda, **⌘Y** apre Visualizzazione rapida, **⇧⌘N**, **⇧⌘G**, **⇧⌘.**, **⌘[** / **⌘]**, **⌘↑** / **⌘↓**, **⌘1** – **⌘4**, **⌘K** e **⌃⌘T** (Aggiungi alla barra laterale/ai Preferiti). Il trascinamento preseleziona Copia, anche sullo stesso volume (dove il Finder sposta); **⌘** preseleziona Sposta.

> [!NOTE]
> **Novità di questa versione:** **⌘T** apre una nuova scheda (prima: **⇧⌘T**), i Trasferimenti sono passati a **⌥⌘L** (prima: **⌘T**), **⌘⌫** sposta gli elementi locali nel Cestino (prima: eliminazione definitiva, che ora è **⌥⌘⌫**). Manta lo mostra una volta dopo l’aggiornamento.
