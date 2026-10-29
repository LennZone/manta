# Preferiti e barra laterale {#seitenleiste}

La barra laterale a sinistra ti porta con un clic nei posti più importanti: Preferiti, cartelle standard, unità, server connessi, container e archiviazione cloud. Un clic apre la posizione nel pannello attivo.

![Barra laterale con tutte le sezioni](images/sidebar.png)

## Le sezioni {#abschnitte}

| Sezione | Contenuto |
|---|---|
| **Preferiti** | Cartelle e connessioni ai server che hai salvato. |
| **Posizioni** | Inizio, Applicazioni, Scrivania, Documenti, Download e, in fondo, il **Cestino** (vedi sotto). |
| **Cloud** | iCloud Drive e le cartelle delle app cloud (Google Drive per desktop, OneDrive, Dropbox, Box, …), vedi [Archiviazione cloud](09-cloud.md). La sezione compare solo quando Manta trova almeno un’archiviazione cloud. |
| **Dispositivi** | Macintosh HD e ogni volume montato (unità USB, dischi, condivisioni di rete, immagini disco). |
| **Server** | I server a cui sei connesso. Senza connessione mostra **Connessione al server…**. |
| **Container** | Container Docker in esecuzione e arrestati e pod Kubernetes (solo se Docker è installato o se è impostato un contesto Kubernetes). |

Il pulsante con la freccia circolare accanto a **Cloud** e **Dispositivi** cerca di nuovo cartelle cloud e volumi. Manta si accorge anche da solo delle nuove app cloud.

Passa con il puntatore su una posizione per vederne il percorso completo in un suggerimento.

## Aggiungere una cartella ai Preferiti {#favorit-hinzufuegen}

Hai tre possibilità:

- **Tastiera**: apri la cartella nel pannello attivo e premi **⌃⌘T** (**Aggiungi cartella ai Preferiti**).
- **Mouse**: fai clic sul **+** accanto all’intestazione **Preferiti** (“Aggiungi cartella attuale”), oppure scegli **Aggiungi cartella ai Preferiti** nel menu contestuale di un punto vuoto del pannello.
- **Trascinamento**: trascina una o più cartelle da un pannello sulla sezione **Preferiti** (vedi sotto).

Le cartelle **all’interno degli archivi** non possono essere tra i Preferiti (“Le cartelle all’interno degli archivi non possono essere aggiunte ai Preferiti”).

### Trascinare cartelle nei Preferiti {#favoriten-ziehen}

1. Seleziona una o più cartelle in un pannello, in qualsiasi vista, in locale o su un server.
2. Trascinale sulla sezione **Preferiti**. La sezione viene evidenziata, una linea mostra dove finiranno le cartelle e accanto al puntatore compare ad esempio “Aggiungi 2 cartelle ai Preferiti”.
3. Rilascia. Manta conferma con “‘Progetti’ aggiunto ai Preferiti” o “2 cartelle aggiunte ai Preferiti”.

Buono a sapersi:

- **Solo cartelle**: se trascini solo file, il puntatore indica “Solo le cartelle possono essere tra i Preferiti” e non succede nulla. Con una selezione mista Manta prende solo le cartelle.
- **Niente doppioni**: se una cartella è già tra i Preferiti, la voce esistente lampeggia brevemente e Manta segnala “‘nome’ è già nei Preferiti”. Conta solo la posizione, non il nome del preferito.
- **Cartelle sui server**: trascinando una cartella da un server crei un preferito del server con quella cartella come cartella iniziale, ad esempio “files.example.com – www”. Non viene salvata una nuova password; se il Portachiavi ne contiene già una per questo account del server (da un altro preferito), il nuovo preferito usa anche quella. Altrimenti Manta te la chiede quando lo apri (a meno che tu non acceda con ssh-agent o con un file chiave senza passphrase).

![Una cartella trascinata nei Preferiti](images/favorites-drag.png)

## Riordinare e rimuovere i Preferiti {#favoriten-ordnen}

- **Riordinare**: trascina un preferito in alto o in basso nell’elenco. La riga trascinata si attenua e una linea mostra la nuova posizione. L’ordine viene salvato subito.
- **Rimuovere**: clic destro sul preferito → **Rimuovi dai Preferiti**, oppure trascina il preferito sul **Cestino** nella barra laterale. La cartella in sé, naturalmente, resta.

## Cestino {#papierkorb}

Il **Cestino** è l’ultima riga sotto **Posizioni**, come nel Finder. La sua icona mostra se è vuoto o pieno (cestino pieno = contiene qualcosa); il suggerimento indica il numero di elementi.

- **Aprire**: fai clic su **Cestino**. Si apre nel pannello attivo come qualsiasi cartella. **Elimina immediatamente…** (o **⌘⌫**) elimina per sempre singoli elementi al suo interno; Manta chiede prima conferma.
- **Svuotare**: clic destro su **Cestino** → **Svuota il Cestino…**, oppure premi **⇧⌘⌫**, oppure scegli **Svuota il Cestino…** nella palette dei comandi. Manta chiede conferma (“Vuoi davvero eliminare definitivamente gli elementi nel Cestino?”) e mostra il numero di elementi e la loro dimensione. L’operazione non si può annullare. Gli elementi che non possono essere eliminati (ad esempio quelli bloccati) vengono saltati e segnalati.
- **Trascinarci sopra**: trascina elementi da un pannello su **Cestino**. La riga viene evidenziata e accanto al puntatore compare ad esempio “Sposta 3 elementi nel Cestino”. Gli elementi locali vanno nel Cestino senza domande (come con **⌘⌫**). Gli elementi su un **server**, in **S3** o in un **container** non possono finire in un Cestino: Manta chiede se eliminarli definitivamente. Gli elementi di un **archivio** non possono essere eliminati, e il puntatore lo indica. Puoi anche trascinare sul Cestino file da altre app (ad esempio dal Finder).
- **Ripristina**: solo nel Finder (clic destro → **Apri nel Finder**, poi **Ripristina**). Il Finder conserva la posizione originale nei propri dati, che Manta non può leggere.

### Accesso completo al disco {#papierkorb-zugriff}

macOS protegge il Cestino: per mostrarne il contenuto, Manta ha bisogno dell’**Accesso completo al disco**. Senza di esso il pannello indica “Manta richiede l’accesso completo al disco per mostrare il Cestino” e offre due pulsanti:

1. **Apri Accesso completo al disco** apre **Impostazioni di Sistema › Privacy e sicurezza › Accesso completo al disco**. Lì attiva **Manta** e riapri il Cestino (**⌘R**). A volte per questo bisogna riavviare Manta.
2. **Apri nel Finder** mostra il Cestino nel Finder, anche quando Manta è il tuo gestore file predefinito.

Senza Accesso completo al disco, il trascinamento sul Cestino e **⌘⌫** funzionano comunque, e la riga indica ancora se il Cestino è vuoto o pieno. **Svuota il Cestino…** viene allora eseguito dal Finder (macOS chiede una volta se Manta può controllare il Finder); il Finder svuota anche il Cestino degli altri volumi.

## Server nei Preferiti {#server-favoriten}

Una connessione a un server diventa un preferito quando attivi **Aggiungi ai Preferiti** nella finestra **Connessione al server**. Puoi darle un nome a tua scelta (“Nome nella barra laterale (facoltativo)”) e una cartella iniziale.

Clic destro su un preferito del server:

- **Apri**: si connette e apre la cartella iniziale.
- **Modifica connessione…**: apre la finestra di connessione con i dati salvati.
- **Rimuovi dai Preferiti**.

Premendo **⌃⌘T** in una cartella su un server crei un preferito del server con quella cartella come cartella iniziale. La password non viene copiata.

Vedi [Connessioni](08-verbindungen.md#favoriten).

## Unità {#geraete}

Le unità appena collegate e le immagini disco aperte (ad esempio un DMG da cui installi un’app) compaiono da sole sotto **Dispositivi**. Se una non compare, fai clic sull’icona a freccia accanto a **Dispositivi** (**Rileggi volumi**).

Come nel Finder, immagini disco, chiavette USB e unità esterne sono elencate in una sezione propria, **Espellibili**, con l’icona del volume (o dell’app che contiene) se ne ha una. L’icona di espulsione accanto espelle l’unità. Il menu contestuale offre inoltre **Apri**, **Apri in una nuova scheda**, **Mostra nella cartella superiore** (per un’immagine disco, il file DMG) e **Espelli “Nome”**. I pannelli che mostrano l’unità passano a **Inizio** quando viene espulsa. Se un’app sta ancora usando l’unità, Manta segnala che non è stato possibile espellerla.

## Server connessi {#server}

Ogni connessione aperta è elencata sotto **Server** con un punto verde. Un clic la apre nel pannello attivo. L’icona di espulsione che compare al passaggio del puntatore chiude la connessione (**Disconnetti**).

## Container {#container}

La sezione **Container** elenca i tuoi container Docker, raggruppati per progetto Compose come in Docker Desktop. I container in esecuzione hanno un punto colorato; quelli arrestati sono attenuati e raccolti sotto “Arrestati (N)”. Per il contesto Kubernetes attuale c’è un selettore di namespace con i pod e i loro container.

**Avvia, riavvia, arresta**: fai clic con il tasto destro su un container Docker. Un container arrestato offre **Avvia**, uno in esecuzione **Riavvia** e **Arresta**. Manta esegue `docker start`, `docker restart` o `docker stop`; nel frattempo la riga indica “Avvio…” (ecc.) e un messaggio conferma il risultato. Se un container si arresta mentre è aperto in un pannello, il pannello offre **Avvia container** invece di **Riprova** e mostra di nuovo la cartella non appena è in esecuzione.

L’elenco si aggiorna in background ogni 5 secondi. L’icona a freccia (**Aggiorna container**) lo rilegge subito. Vedi [Connessioni](08-verbindungen.md#container).

## Dove vengono salvati i Preferiti {#speicherort}

In `~/Library/Application Support/Manta/favorites.json`. Le password non sono lì: con **Ricorda password** finiscono nel Portachiavi di macOS, vedi [Connessioni](08-verbindungen.md#favoriten).
