# Plug-in integrati {#plugins}

Molte funzioni di Manta si trovano nei plug-in integrati. Sono attivi fin dall’inizio e puoi disattivarli uno per uno in **Impostazioni** → **Plug-in**. Git e Terminale hanno capitoli a parte: [Git](12-git.md), [Terminale](11-terminal.md).

## Attivare e disattivare i plug-in {#verwalten}

1. Apri le **Impostazioni** con **⌘,** e scegli **Plug-in**.
2. I plug-in integrati si trovano sotto **Integrati**. Fai clic su **Disattiva** o **Attiva**.
3. Se un plug-in ha delle opzioni, aprile con **Impostazioni** sulla sua scheda.

Per aggiungere plug-in di altri, apri **Scopri** in alto nella pagina Plug-in, fai doppio clic su un file `.mantaplugin`, trascinalo sulla pagina Plug-in oppure usa **Installa plug-in…**, da un file `.mantaplugin` o da una cartella. I plug-in di altri vengono eseguiti in una sandbox di macOS e possono fare solo ciò che i loro permessi consentono; i plug-in integrati fanno parte di Manta e non sono in sandbox. Per saperne di più: [Plug-in personali](15-eigene-plugins.md#installieren) e [Sicurezza](15-eigene-plugins.md#sicherheit).

Molti plug-in mostrano il loro contenuto in una **barra laterale a destra** o in un **pannello sotto il pannello dei file**. Aprili con le scorciatoie, con gli indicatori nella barra del percorso o con la palette dei comandi (“Mostra il pannello “…””).

![Impostazioni → Plug-in](images/settings-plugins.png)

## Dimensioni cartelle {#ordnergroessen}

**Dimensioni cartelle** mostra le dimensioni reali anche per le cartelle nella colonna **Dimensione**, misurate in background e memorizzate nella cache. Le dimensioni restano anche dopo un riavvio: Manta le mostra subito e le rimisura in background nel caso qualcosa sia cambiato.

- Mentre Manta sta ancora contando, mostra ad esempio “≥ 1,2 GB” con un piccolo indicatore di attività.
- Passa con il puntatore su una dimensione per vedere il numero di file e la dimensione su disco.
- Puoi ordinare per dimensione delle cartelle, e la dimensione conta anche per il totale della selezione.

La dimensione è la somma delle dimensioni dei file (come per i file). Manta non segue i link simbolici, conta gli hard link una sola volta e resta sul volume della cartella.

**Solo su richiesta**: se in **Impostazioni** → **Plug-in** → **Dimensioni cartelle** passi a **Solo su richiesta**, le cartelle mostrano “—” finché non fai clic con il tasto destro e scegli **Calcola dimensione**.

### Utilizzo disco {#speicherbelegung}

**⌥⌘U** (**Mostra utilizzo disco**) mostra cosa occupa spazio nella cartella attiva:

- una **treemap**: fai clic per ingrandire, doppio clic per aprire,
- un elenco degli **Elementi più grandi**.

![Utilizzo disco con una treemap](images/disk-usage.png)

Dimensioni cartelle e Utilizzo disco funzionano solo con le cartelle locali.

## Rinomina in blocco {#umbenennen}

Rinomina molti file in una volta, con regole, anteprima in tempo reale e annullamento.

1. Seleziona i file. Se non è selezionato nulla, la rinomina vale per tutti i file della cartella.
2. Premi **⇧⌘R** (**Rinomina in blocco…**) oppure **F2** con più elementi selezionati.
3. Aggiungi regole con **Aggiungi regola**. Vengono applicate dall’alto in basso.
4. Controlla l’**Anteprima**: nome originale → nuovo nome, con le modifiche evidenziate carattere per carattere.
5. Fai clic su **Rinomina** (o premi ↩).

![Rinomina in blocco con regole e anteprima](images/batch-rename.png)

Le regole:

| Regola | Cosa fa |
|---|---|
| **Trova e sostituisci** | Testo o espressione regolare, con gruppi `$1`; distinzione tra maiuscole e minuscole; prima occorrenza o tutte. |
| **Inserisci testo** | All’inizio, alla fine o in una posizione. |
| **Rimuovi caratteri** | I primi, gli ultimi o un intervallo di caratteri. |
| **Maiuscole/minuscole** | minuscole, MAIUSCOLE, Titolo, Frase. |
| **Numerazione** | 1, 2, 3 … con inizio, passo, cifre (zeri iniziali), come prefisso, suffisso o come nome. |
| **Data** | Data di modifica o data di oggi, in un formato a tua scelta. |
| **Estensione** | Cambia l’estensione del file, la mette in minuscolo/maiuscolo o la rimuove. |
| **Pulizia** | Rimuove gli spazi ai bordi, comprime gli spazi, translittera i caratteri accentati (ä → ae, é → e). |

Ogni regola può valere per **Nome**, **Estensione** o **Entrambi**, può essere disattivata e riattivata e può essere spostata.

Sicurezza:

- L’anteprima mostra subito i conflitti (nomi duplicati, nomi già esistenti, nomi vuoti o non validi). La rinomina funziona solo quando sono tutti risolti.
- Non viene mai sovrascritto nulla. Lo scambio di nomi (a ↔ b) e le modifiche solo di maiuscole/minuscole funzionano.
- Al termine un avviso propone **Annulla**. Lo trovi anche nella palette dei comandi: **Annulla rinomina in blocco**.

**Preimpostazioni**: in **Preimpostazioni** salvi un set di regole con un nome e lo ricarichi in seguito.

Rinomina in blocco funziona sul tuo Mac e sui server.

## Azioni rapide {#schnellaktionen}

Le **Azioni rapide** sono comandi tuoi per i file selezionati: ridimensionare immagini, creare zip, aprire in VS Code, calcolare checksum e qualsiasi altra cosa possa fare un comando shell.

### Eseguire un’azione rapida

- **Clic destro** sui file → sottomenu **Azioni rapide**. Elenca solo le azioni adatte alla selezione (ad esempio “Ridimensiona immagini al 50%” solo per le immagini).
- **Palette dei comandi** (**⇧⌘P**): digita il nome dell’azione.
- **Pannello**: **Mostra pannello Azioni rapide** nella palette mostra le azioni applicabili e, in **Ultime esecuzioni**, stato, durata e output.

Vengono eseguite al massimo due azioni in background alla volta. Se una richiede più tempo, compare un avviso con **Annulla**; alla fine ricevi un avviso con il risultato. Le azioni da terminale vengono eseguite in modo visibile nel [Terminale](11-terminal.md) sotto il pannello.

### Azioni incluse in Manta

Ridimensiona immagini al 50% · Converti in JPEG · Converti in PNG · Comprimi in ZIP · Estrai archivio · Apri in VS Code · Apri in Terminal.app · Copia percorso · Mostra nel Finder · Rendi eseguibile · Conta righe · Calcola SHA-256 · Copia nell’altro pannello (rsync)

### Le tue azioni in actions.json {#actions-json}

Le azioni si trovano in `actions.json` nella cartella dei dati del plug-in (`~/Library/Application Support/Manta/plugin-data/manta.quickactions/`).

1. Scegli **Modifica azioni rapide…** nella palette dei comandi. Il file si apre nel tuo editor di testo.
2. Modifica o aggiungi azioni e salva il file.
3. Manta lo ricarica entro un secondo e segnala eventuali problemi (“Azioni rapide: …”). A mano: **Ricarica azioni rapide**.

Un esempio:

```jsonc
{
  "actions": [
    {
      "id": "optimize",
      "title": "Optimize PNG",
      "title_de": "PNG optimieren",
      "applies_to": ["png"],
      "command": "optipng {file}",
      "message": "Optimized {count} image{s}",
      "message_de": "{count} Bild(er) optimiert",
      "refresh": true
    }
  ]
}
```

I campi più importanti:

| Campo | Significato |
|---|---|
| `id` | ID univoco (lettere, cifre, `.`, `-`, `_`). |
| `title`, `title_de` | Nome nel menu; `title_de` viene usato quando Manta è in tedesco, in tutte le altre lingue (italiano compreso) vale `title`. |
| `command` | Comando shell, eseguito con `/bin/sh -c` nella cartella della selezione. |
| `applies_to` | `"any"`, `"files"`, `"folders"` e/o estensioni come `"jpg"`. |
| `min`, `max` | Quanti elementi devono essere selezionati. |
| `mode` | `"background"` (predefinito), `"terminal"` (visibile nel terminale) o `"silent"`. |
| `output` | `"none"`, `"toast"` (output come avviso) o `"panel"`. |
| `confirm` | Domanda prima dell’esecuzione, ad esempio `"Delete {count} file{s}?"`. |
| `refresh` | Rilegge il pannello al termine. |
| `shortcut` | Una scorciatoia propria, ad esempio `"⌃⌥Z"`. |

Segnaposto nel comando: `{file}` (il comando viene allora eseguito una volta per ogni elemento), `{files}` (tutti insieme), `{name}`, `{stem}`, `{ext}`, `{dir}`, `{dest}` (la cartella nell’altro pannello) e `{count}`. Alle virgolette pensa Manta: scrivi `{file}`, non `'{file}'`.

Il riferimento completo si trova in [docs/quick-actions.md](../../quick-actions.md).

> [!TIP]
> Per ottenere le azioni predefinite aggiornate dopo un aggiornamento, rinomina `actions.json`. Manta crea di nuovo il file.

Le azioni rapide funzionano solo nelle cartelle locali.

## Info progetto {#projektinfo}

**Info progetto** riconosce il progetto in cui ti trovi e rende eseguibili i suoi script con un clic.

Riconosce: Laravel (`artisan`), Composer, Node (npm, pnpm, yarn, bun), Docker Compose, Rust (Cargo), Python, Make, just e Task.

- **Indicatore nella barra del percorso**: ad esempio “Laravel · 28 script”. Fai clic per mostrare o nascondere il pannello Progetto.
- **Pannello Progetto**: nome, radice, branch Git e versioni (ad esempio “PHP ^8.2”), i pulsanti **Apri in VS Code**, **Finder**, **.env** e **Vai alla radice**, gli script usati di recente e tutti gli script raggruppati per strumento.
- **Un clic su uno script** lo esegue nel [Terminale](11-terminal.md) sotto il pannello, nella radice del progetto, come se lo avessi digitato tu.
- **Palette dei comandi**: **Esegui script del progetto…**, **Mostra pannello Progetto**, **Esegui di nuovo l’ultimo script** (**⌥⌘R**).
- **Clic destro** su `package.json`, `composer.json`, `Makefile` e simili: **Mostra script**.

![Pannello Progetto per un progetto Laravel](images/project-info.png)

> [!NOTE]
> Gli script che possono cancellare dati (come `migrate:fresh`, `docker compose down -v`, `rm -rf`, o i nomi che contengono “clean”, “reset”, “deploy”, “publish”) chiedono conferma prima di partire. Sono contrassegnati da un segnale di avviso.

Info progetto richiede il plug-in Terminale e funziona solo nelle cartelle locali. Dettagli: [docs/project-info.md](../../project-info.md).

## Anteprima Markdown {#markdown}

**Anteprima Markdown** mostra il file `.md`, `.markdown` o `.mdx` selezionato, ben impaginato, in una barra laterale a destra. Se non è selezionato nulla, mostra il README della cartella.

- Mostrare e nascondere: **⌥⌘M** (**Mostra/nascondi anteprima Markdown**).
- Supporta il Markdown in stile GitHub: tabelle, elenchi di attività, avvisi, note a piè di pagina, codice evidenziato con un pulsante **Copia**, immagini locali.
- **Indice** mostra il sommario; i link ad altri file `.md` li aprono, **Indietro** torna indietro.
- **Apri nell’editor** e **Mostra nel pannello**.
- In tempo reale: quando modifichi il file, l’anteprima si aggiorna e mantiene la posizione di scorrimento.

Per la tua privacy l’anteprima non carica immagini dal web (il loro server verrebbe a sapere che hai aperto il file). Per attivarle: **Impostazioni** → **Plug-in** → **Anteprima Markdown** → **Carica immagini dal web (http/https)**.

I file oltre i 2 MB vengono mostrati solo in parte e su richiesta (**Mostra i primi 2 MB**). Niente HTML (tranne immagini e interruzioni di riga), niente immagini SVG, niente formule matematiche.

## Strumenti immagine {#bildwerkzeuge}

Gli **Strumenti immagine** mostrano i dettagli delle immagini e modificano molte immagini in una volta.

**Contrassegni nell’elenco:**

- **lo**: bassa risoluzione, lato più lungo inferiore a 800 px.
- **XL**: grande per il web, oltre 2 MB.
- Il suggerimento mostra ad esempio “1920×1080 · sRGB · 340 KB”.

**Pannello Immagine** (**Mostra pannello Immagine** nella palette): anteprima, dimensioni, proporzioni, dimensione del file, profilo colore, DPI, canale alfa, animazione e dati EXIF (fotocamera, obiettivo, esposizione, data di scatto). Se una foto contiene una posizione GPS, il pannello ti avvisa e offre **Apri in Mappe**.

**Clic destro → Strumenti immagine:**

| Comando | Cosa fa |
|---|---|
| **Ridimensiona…** | 50%, 1920/1280/800/400 px o una dimensione personalizzata; mantiene le proporzioni. |
| **Converti in…** | JPEG, PNG, HEIC, TIFF; WebP con `cwebp`. |
| **Ruota di 90° a sinistra/destra**, **Ruota di 180°**, **Rifletti orizzontalmente/verticalmente** | |
| **Rimuovi metadati…** | Rimuove EXIF (GPS compreso), XMP, IPTC e commenti, senza perdita di qualità per JPEG, PNG e WebP. HEIC e TIFF richiedono `exiftool`. |
| **Ottimizza per il web…** | Lato più lungo al massimo 2560 px, sRGB, qualità JPEG 82 (PNG mantenuto in caso di trasparenza). |
| **Crea set @2x/@1x…** | L’originale come @2x e una versione @1x a metà dimensione. |
| **Copia come data URI** | Per una singola immagine, da incorporare in HTML/CSS. |

In **Salva come** scegli **Accanto all’originale**, **In una sottocartella** o **Sostituisci originali** (gli originali vanno nel Cestino). Manta non sovrascrive mai file esistenti senza chiedere; ai nomi già occupati aggiunge `-2`, `-3` …

Gli Strumenti immagine usano `sips` di macOS. Per WebP installa `cwebp` (`brew install webp`), per i metadati HEIC/TIFF `exiftool` (`brew install exiftool`). Funzionano solo nelle cartelle locali.

![Pannello Immagine con dati EXIF](images/image-tools.png)

## Duplicati {#duplikate}

**Duplicati** trova i file identici in una cartella e nelle sue sottocartelle e sposta le copie in più nel Cestino.

1. Apri la cartella e scegli **Trova duplicati in questa cartella** nella palette dei comandi, oppure clic destro su una cartella → **Trova duplicati qui**.
2. Manta analizza la cartella in background (puoi annullare) e confronta la dimensione, poi un hash rapido, poi SHA-256.
3. Il pannello **Duplicati** mostra i gruppi, il numero di file duplicati e quanto spazio recupereresti. I gruppi con il risparmio maggiore vengono per primi.
4. In ogni gruppo viene mantenuta una copia (“Mantenuto”). Quale lo stabilisce la **Regola**: **Più vecchio**, **Più recente**, **Percorso più breve** o **Nella cartella …**. Fai clic su un’altra copia per mantenere invece quella.
5. **Elimina copie** (per gruppo) o **Sposta copie nel Cestino** (tutte), dopo una conferma con numero e dimensione.

![Pannello Duplicati con i gruppi](images/duplicates.png)

Buono a sapersi:

- Duplicati usa sempre il **Cestino** di macOS, mai un’eliminazione definitiva. Puoi ripristinare tutto.
- I file modificati dopo l’analisi non vengono toccati.
- `node_modules`, `vendor`, `.git`, i pacchetti (come `.app`), i link simbolici e i secondi hard link vengono saltati.
- **Mostra nel Finder** e **Seleziona nel pannello** ti aiutano a controllare.
- Opzioni: nel pannello, in **Impostazioni** → **Plug-in** → **Duplicati** (Salta i pacchetti, Includi file nascosti) e in `settings.json` (esclusioni, dimensione minima, un comando personalizzato per il cestino).

Duplicati funziona solo nelle cartelle locali.

## Carico di sistema {#systemauslastung}

**Carico di sistema** mostra a destra nella barra di stato quanto sta lavorando il Mac: “CPU 12%  GPU 3%  RAM 12,4 / 16 GB”, ognuno con un piccolo indicatore. I valori vengono letti ogni 2 secondi in background, senza privilegi di amministratore e senza programmi di supporto. Quando la finestra è ridotta a icona o coperta, la misurazione si ferma.

- Oltre l’85% di carico o con pressione della memoria elevata, valore e indicatore diventano ambra; con pressione critica, rossi.
- Il suggerimento mostra i dettagli con i termini di Monitoraggio Attività: i core (ad alte prestazioni e ad alta efficienza), il core più carico, i core GPU, la memoria app, la memoria residente, quella compressa, lo swap utilizzato e la pressione della memoria.
- **RAM** è la “Memoria utilizzata” di Monitoraggio Attività (memoria app + residente + compressa), senza le cache.
- Un clic apre **Monitoraggio Attività**.
- Nelle finestre strette scompare prima il suggerimento della palette dei comandi, poi la GPU; dopo, CPU e RAM mostrano solo icona e indicatore.

Puoi disattivarlo in **Impostazioni** → **Plug-in** → **Carico di sistema**; lì puoi anche deselezionare **Mostra il carico della GPU**. Sui Mac senza statistiche della GPU, la GPU viene omessa da sola.

## Plug-in inclusi {#mitgeliefert}

**Impostazioni** → **Plug-in** → **Disponibili** elenca i plug-in forniti con Manta ma non installati (ad esempio l’esempio “Hello Python”). **Installa** li copia nella tua cartella dei plug-in; partono disattivati. Vedi [Plug-in personali](15-eigene-plugins.md).
