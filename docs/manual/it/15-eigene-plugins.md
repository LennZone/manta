# Plug-in personali {#eigene-plugins}

Puoi estendere Manta con plug-in tuoi, in qualsiasi linguaggio di programmazione. Questo capitolo spiega cosa può fare un plug-in, come installarlo, aggiornarlo e trovarne di nuovi e a cosa fare attenzione. La guida tecnica si trova nella documentazione per sviluppatori.

## Cosa possono fare i plug-in {#moeglichkeiten}

- Mostrare **contrassegni** accanto ai nomi dei file (come i contrassegni Git).
- Mostrare **indicatori** nella barra del percorso (come il branch o il progetto).
- Offrire **comandi** per la palette dei comandi e il menu contestuale, se vuoi con una scorciatoia.
- Mostrare **pannelli** sotto un pannello dei file, nella barra laterale destra o come finestra di dialogo.
- Mostrare **elementi nella barra di stato**, con icona, un piccolo indicatore e un colore di avviso (come [Carico di sistema](14-plugins.md#systemauslastung)).
- Mostrare **avvisi**.

Un plug-in è una cartella con un file `plugin.toml` e un programma (ad esempio uno script Python). Manta avvia il programma e scambia messaggi con esso (righe JSON tramite stdin/stdout). Un file `.mantaplugin` è semplicemente questa cartella compressa in ZIP.

## Installare un plug-in {#installieren}

1. Apri **Impostazioni** (**⌘,**) → **Plug-in**.
2. Fai clic su **Installa plug-in…** e scegli **Da un file (.mantaplugin, .zip)…** o **Da una cartella…** (quella con `plugin.toml`).
3. Prima di installare qualsiasi cosa, Manta mostra un riepilogo: nome, versione, autore, descrizione, **Cosa dichiara di fare** (ad esempio “Legge i tuoi file”, “Modifica o elimina file”, “Usa la rete”, “Esegue altri programmi”, “Esegue comandi nel Terminale”), il **Comando** eseguito, l’**Origine**, un link al **Codice sorgente**, la **Licenza** e la dimensione. Sotto, un avviso ricorda che Manta non controlla i plug-in.
4. Fai clic su **Installa**. Il plug-in viene quindi elencato sotto **Installati**, ancora disattivato.

![Riepilogo prima di installare un plug-in](images/plugin-install.png)

> [!NOTE]
> Ciò che un plug-in dichiara di fare è indicato dal suo autore. Manta non può verificarlo né imporlo.

### Attivarlo {#einschalten}

Fai clic su **Attiva** sulla scheda del plug-in. La prima volta Manta ti chiede di confermare e mostra di nuovo autore, versione, permessi, il comando e la cartella in cui viene eseguito.

![Conferma all’attivazione di un plug-in esterno](images/plugin-confirm.png)

Se hai scaricato il plug-in da Internet, macOS tiene i suoi file in quarantena e Gatekeeper bloccherebbe i programmi che contiene. La conferma lo segnala: solo quando attivi il plug-in Manta rimuove il contrassegno di quarantena dai suoi file, così che i suoi programmi possano essere eseguiti; mai a tua insaputa.

Altri pulsanti sulla scheda di un plug-in: **Ricarica**, **Mostra log**, **Mostra cartella**, **Disinstalla**. In alto: **Apri cartella plug-in** e **Ricarica** (cerca plug-in aggiunti, modificati o rimossi).

## Aggiornare e disinstallare {#aktualisieren}

- **Aggiornare**: installa la nuova versione nello stesso modo, da un file o da una cartella. Se ha lo stesso id di un plug-in installato, il riepilogo indica un aggiornamento e mostra sotto **Installato** la versione che verrà sostituita. Dopo la conferma Manta sostituisce il plug-in e mantiene il suo stato attivo o disattivato.
- **Disinstallare**: fai clic su **Disinstalla** sulla scheda del plug-in. Manta elimina la cartella del plug-in; la sua cartella dei dati viene mantenuta.

## Trovare plug-in {#entdecken}

In **Impostazioni** → **Plug-in**, fai clic su **Sfoglia i plug-in** in alto a destra (o scegli **Sfoglia i plug-in…** nella palette dei comandi). Si apre l’elenco dei plug-in su [manta-filemanager.com/plugins](https://manta-filemanager.com/plugins/), con i plug-in integrati e quelli della community. Ogni voce ha un link al codice sorgente e un checksum SHA-256 del download. Scarica il file `.mantaplugin` e installalo come descritto [sopra](#installieren).

## Sicurezza {#sicherheit}

> [!WARNING]
> I plug-in esterni vengono eseguiti come programmi con **i permessi del tuo utente**: possono leggere, modificare ed eliminare i tuoi file, avviare altri programmi e inviare dati in rete. Manta non controlla i plug-in e non li isola in una sandbox, nemmeno quelli dell’elenco dei plug-in. Installa solo plug-in di cui ti fidi e leggi il codice dei plug-in presi da Internet prima di attivarli.

Per questo Manta mostra un riepilogo prima dell’installazione, i nuovi plug-in partono disattivati e Manta chiede conferma la prima volta che ne attivi uno.

## Quando un plug-in non funziona {#probleme}

- La scheda mostra **Errore** e il motivo, ad esempio quando manca `python3`.
- **Mostra log** mostra gli ultimi messaggi del plug-in.
- Se un plug-in si chiude in modo anomalo, Manta lo riavvia, al massimo tre volte al minuto. Dopo di che resta in stato di errore finché non fai clic su **Ricarica**.
- Se la cartella dei plug-in contiene due cartelle con lo stesso id di plug-in, la seconda mostra **Impossibile caricare** e un messaggio. Rimuovine una.

## Dove si trovano i plug-in {#speicherorte}

| Cosa | Dove |
|---|---|
| Plug-in installati | `~/Library/Application Support/Manta/plugins/<id>/` |
| Dati e log di un plug-in | `~/Library/Application Support/Manta/plugin-data/<id>/` (log: `plugin.log`) |
| Stato attivo/disattivato | `~/Library/Application Support/Manta/plugins.json` |

La disinstallazione elimina la cartella del plug-in; la sua cartella dei dati viene mantenuta.

## Scrivere un plug-in {#schreiben}

Il modo più rapido per iniziare è l’esempio **Hello Python** (`examples/plugins/hello-python` nel codice sorgente, commentato riga per riga, solo libreria standard di Python). Puoi installarlo anche da **Impostazioni** → **Plug-in** → **Disponibili**. Ci sono modelli anche per Node.js, shell (bash + jq) e Rust (`examples/plugins/`); per Rust c’è inoltre il crate `manta-plugin-protocol`.

Guida, riferimento del protocollo ed esempi: [Scrivere plug-in per Manta (docs/plugins.md)](../../plugins.md).

I plug-in possono parlare tutte le lingue di Manta: Manta comunica loro la lingua attuale, e `plugin.toml` può contenere un nome e una descrizione tradotti (`[localized.it]`, `[localized.de]` e così via).

## Sviluppare e provare {#entwickeln}

- **Ricarica i plug-in** (palette dei comandi) riavvia tutti i plug-in esterni in esecuzione e analizza di nuovo la cartella dei plug-in.
- **Modalità sviluppatore**: avvia Manta dal Terminale con
  `/Applications/Manta.app/Contents/MacOS/manta --plugin-dev ~/code/my-plugin`
  (ripetibile, oppure con la variabile d’ambiente `MANTA_PLUGIN_DEV=dir1:dir2`). Per questa sessione il plug-in viene eseguito direttamente da quella cartella: considerato affidabile, attivato e contrassegnato con **Sviluppo**. Nel frattempo una copia installata con lo stesso id si fa da parte; non viene salvato nulla.
- **Provare senza Manta**: `python3 scripts/plugin-test.py percorso/del/plugin`.
- **Log**: **Mostra log** sulla scheda, oppure il file `~/Library/Application Support/Manta/plugin-data/<id>/plugin.log`.

Per saperne di più: [Develop and debug (docs/plugins.md, in inglese)](../../plugins.md#develop-and-debug).

## Pubblicare un plug-in {#veroeffentlichen}

Metti il plug-in in un repository pubblico con una licenza approvata dall’OSI, allega un file `.mantaplugin` a una release e apri una pull request che lo aggiunga a `plugins/registry.json`. Comparirà poi in **Sfoglia i plug-in**. Dettagli: [Publishing a plugin (docs/plugins.md, in inglese)](../../plugins.md#publishing-a-plugin).
