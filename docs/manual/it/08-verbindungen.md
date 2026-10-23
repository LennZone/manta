# Connessioni {#verbindungen}

Manta apre le cartelle sui server (SFTP, FTP, FTPS), negli archivi S3 e nei container Docker e Kubernetes come se fossero cartelle locali. Lì copi, sposti e rinomini esattamente come sul tuo Mac.

## Connettersi a un server {#verbinden}

1. Premi **⌘K** (**Connessione al server…**), fai clic su **Connetti** nella barra degli strumenti oppure sul **+** accanto a **Server** nella barra laterale.
2. In alto scegli il **Protocollo**: **SFTP**, **FTP**, **FTPS**, **S3** o **Container**.
3. Compila i campi (vedi sotto).
4. Facoltativo: fai clic su **Prova connessione**. Manta segnala ad esempio “Accesso eseguito. Cartella Inizio: /home/anna”.
5. Fai clic su **Connetti**.

Il server si apre nel pannello attivo (la finestra ti dice quale). La connessione compare sotto **Server** nella barra laterale.

![La finestra Connessione al server per SFTP](images/connect-sftp.png)

> [!TIP]
> Puoi digitare tutto in una volta nel campo **Server**: `utente@host:porta`. Manta lo suddivide nei vari campi.

## SFTP {#sftp}

SFTP è lo standard sicuro per i server Linux e per molti piani di hosting.

| Campo | Significato |
|---|---|
| **Server**, **Porta** | L’indirizzo del server; la porta è 22, a meno che tu non la cambi. |
| **Utente** | Il tuo nome utente sul server. |
| **Accedi con** | **Password**, **File chiave** (con una **Passphrase** facoltativa, “solo se la chiave è codificata”) o **Agente SSH** (“Usa le chiavi caricate nell’ssh-agent in esecuzione.”). |
| **Cartella** | Facoltativa: in assenza di indicazioni, la tua cartella Inizio sul server. |

### Verifica della chiave del server {#server-schluessel}

> [!WARNING]
> Manta confronta la chiave del server con il tuo file `~/.ssh/known_hosts`, lo stesso che usa `ssh` nel Terminale.
>
> - **Server noto, chiave corrispondente:** Manta si connette senza chiedere.
> - **Server sconosciuto** (non presente in `known_hosts`): Manta mostra la finestra **Server sconosciuto** con il server, il tipo di chiave e l’impronta (`SHA256:…`, come la stampa `ssh-keygen -lf`). Confronta l’impronta con quella che ti fornisce l’amministratore o il provider di hosting. **Connetti e ricorda** aggiunge la chiave a `known_hosts` (`host`, oppure `[host]:port` per una porta diversa da 22) e si connette; **Annulla** non si connette.
> - **Chiave cambiata:** Manta **non** si connette e mostra un avviso rosso (possibile attacco man-in-the-middle). Di proposito non c’è un pulsante “connetti comunque”. Succede anche quando un server viene reinstallato: chiedi all’amministratore l’impronta corretta. Se corrisponde, rimuovi la vecchia voce nel Terminale con il comando indicato nella finestra (ad esempio `ssh-keygen -R host`, **Copia comando**) e connettiti di nuovo.
>
> I trasferimenti che si riconnettono in background dopo un riavvio non chiedono mai nulla: con una chiave sconosciuta aspettano **Riconnetti**, con una chiave cambiata non riescono.

## FTP e FTPS {#ftp}

**FTP** invia tutto senza codifica, password compresa. Usalo solo nella tua rete locale o per server pubblici. **Anonimo** accede senza password, se il server lo consente.

**FTPS** è FTP con codifica (TLS esplicito, di solito sulla porta 21). Manta non supporta l’FTPS implicito sulla porta 990.

### Il canale dati FTPS {#datenkanal}

FTP usa due connessioni: una per l’accesso e i comandi, e una seconda (il “canale dati”) per gli elenchi delle cartelle e i file. Alcuni firewall aziendali bloccano le connessioni dati codificate. Per questo puoi scegliere un **Canale dati**:

| Impostazione | Comportamento |
|---|---|
| **Automatico** (predefinito) | Codificato. Se la rete blocca le connessioni dati codificate, i trasferimenti proseguono senza codifica; l’accesso resta codificato. |
| **Obbligatorio** | Sempre codificato. Non riesce se la rete blocca le connessioni dati codificate. |
| **Disattivato** | Elenchi e file viaggiano senza codifica; l’accesso resta codificato. |

Quando **Automatico** ripiega su un canale dati non codificato, te ne accorgi così:

- L’indicatore della posizione nella barra del percorso mostra uno scudo di avviso.
- Un avviso, mostrato una sola volta, dice “Connessione dati non codificata (la rete blocca TLS) – l’accesso resta codificato”.

> [!WARNING]
> In questa modalità di ripiego **il contenuto e i nomi dei file viaggiano senza codifica**; restano protetti solo nome utente e password. Per i dati riservati imposta il canale dati su **Obbligatorio** e usa un’altra rete, oppure SFTP.

L’impostazione viene salvata con il preferito. Vedi anche [Risoluzione dei problemi: canale dati FTP](19-fehlerbehebung.md#ftp-datenkanal).

![Finestra FTPS con la scelta del canale dati](images/connect-ftps-data-channel.png)

## S3 e archivi compatibili con S3 {#s3}

**S3** si connette ad Amazon S3 e ai servizi compatibili. Bucket e prefissi compaiono come cartelle.

| Campo | Significato |
|---|---|
| **Fornitore** | Amazon S3, Cloudflare R2, MinIO, Backblaze B2, Wasabi, DigitalOcean Spaces, Hetzner Object Storage o **Altro compatibile S3**. La preimpostazione compila endpoint e regione. |
| **ID account** | Solo Cloudflare R2 (dashboard Cloudflare › R2 › Account ID). |
| **Regione**, **Endpoint** | Di solito l’endpoint segue la regione. Se un bucket si trova in un’altra regione, Manta corregge da solo. |
| **Accedi con** | **Profilo AWS** (chiavi da `~/.aws/credentials`), **Chiave di accesso** (ID chiave di accesso e chiave segreta) o **Pubblico** (per i bucket pubblici). |
| **Bucket** | Facoltativo: altrimenti vedi tutti i bucket. |
| **Indirizzamento basato sul percorso** | Necessario per MinIO e per la maggior parte dei server in self-hosting. |

![Finestra S3 con una preimpostazione del fornitore](images/connect-s3.png)

Buono a sapersi:

- Rinominare e spostare all’interno di un bucket corrisponde, dietro le quinte, a “copia ed elimina”. Gli oggetti oltre i 5 GB non possono essere rinominati.
- In S3 non si possono impostare date di modifica e permessi.
- Eliminando l’ultimo file di una “cartella”, la cartella scompare se non ha un proprio segnaposto di cartella.
- I profili AWS che usano SSO, `credential_process` o assume-role non funzionano, solo le chiavi statiche.
- Se l’orologio del tuo Mac è sfasato di più di 15 minuti, S3 rifiuta le richieste (“L’orologio del Mac non è corretto”).

## Docker e Kubernetes {#container}

Manta apre i file **all’interno dei container** come una cartella. Usa i comandi `docker` e `kubectl`, che devono essere installati sul tuo Mac.

### Dalla barra laterale

Il modo più rapido: fai clic su un container nella sezione **Container** della barra laterale. Si apre nella sua directory di lavoro.

### Dalla finestra di connessione

1. Premi **⌘K** e scegli **Container**.
2. Scegli il **Motore**: **Docker** o **Kubernetes**.
3. Docker: scegli il container dall’elenco o digitane il nome o l’ID. Kubernetes: scegli **Contesto**, **Namespace**, **Pod** e, se serve, il container.
4. Fai clic su **Connetti**.

I Preferiti salvano il container **per nome**, quindi un preferito continua a funzionare anche dopo che il container è stato ricreato.

![Selezione del container nella finestra di connessione](images/connect-container.png)

### Container arrestati e immagini senza shell

- I **container arrestati** si aprono in **sola lettura** (indicatore **Arrestato**). Puoi copiare i file all’esterno; avvia il container per apportare modifiche.
- Anche i container **senza shell** (immagini distroless o scratch) sono di sola lettura.
- Manta legge una cartella di un container arrestato tutta in una volta; se supera i 512 MB segnala che è troppo grande. In quel caso avvia il container.

### Una shell nel container

Clic destro → **Apri shell nel container** (disponibile anche nella palette dei comandi) apre una shell all’interno del container nel terminale sotto il pannello, direttamente nella cartella che stai guardando. Vedi [Terminale](11-terminal.md#container-ssh).

> [!NOTE]
> Manta modifica solo i file su cui agisci tu. Non avvia, non arresta e non rimuove mai i container.

## Connessioni nei Preferiti {#favoriten}

1. Attiva **Aggiungi ai Preferiti** nella finestra di connessione.
2. Se vuoi, inserisci un nome per la barra laterale.
3. Facoltativo: **Ricorda password** (oppure **Ricorda passphrase**, **Ricorda chiave segreta**).

> [!NOTE]
> Password, passphrase e chiavi segrete S3 memorizzate vengono salvate nel **Portachiavi di macOS** (portachiavi login, elemento “com.manta-filemanager.manta.server”, account come `sftp://user@host:22`), non in `favorites.json`. Il file annota solo che esiste un elemento. Manta legge la password quando ti connetti; macOS può chiederti una volta se Manta può usare il Portachiavi.
>
> Le versioni precedenti salvavano le password memorizzate senza codifica in `favorites.json`. Al primo avvio Manta le sposta nel Portachiavi e riscrive il file senza di esse. Se l’operazione non riesce (Portachiavi bloccato o accesso negato), il file resta invariato e Manta riprova all’avvio successivo.
>
> Rimuovendo un preferito viene eliminato anche il suo elemento del Portachiavi (a meno che un altro preferito per lo stesso account del server lo usi ancora). Quando modifichi un preferito, il campo della password indica “salvata nel Portachiavi”; lascialo vuoto per mantenere la password salvata.

Per modificare un preferito: clic destro → **Modifica connessione…** (la finestra “Modifica connessione”).

## Disconnettersi {#trennen}

Passa con il puntatore sulla connessione sotto **Server** nella barra laterale e fai clic sull’icona di espulsione (**Disconnetti**). I pannelli che mostravano il server indicano “Disconnesso”.

## Quando non funziona {#fehler}

Manta trasforma gli errori tecnici in messaggi chiari con un suggerimento. I più frequenti:

| Messaggio | Cosa puoi fare |
|---|---|
| “Impossibile trovare “host”” | Controlla che l’indirizzo del server non contenga errori di battitura. |
| “host ha rifiutato la connessione sulla porta …” | Porta e protocollo sono giusti? Il server è in funzione? |
| “host non ha risposto” | Controlla la rete, la VPN e il firewall. |
| “Il server non ha accettato queste credenziali” | Controlla nome utente e password, oppure prova un altro metodo di accesso. |
| “Nessuna chiave utilizzabile in ssh-agent” | Aggiungi una chiave con `ssh-add`. |
| “Impossibile stabilire una connessione sicura” | Il server non supporta FTPS su questa porta, oppure il suo certificato non è attendibile. |
| “Docker non è in esecuzione” | Avvia Docker Desktop. |
| “kubectl non è installato” | `brew install kubectl` |

Altro aiuto: [Risoluzione dei problemi](19-fehlerbehebung.md).

## Limiti {#grenzen}

- Niente FTPS implicito (porta 990).
- Kubernetes: niente port forwarding, niente container di debug effimeri; i pod devono avere `sh`.
- La scrittura nei container passa da `cat`; i nuovi file appartengono all’utente con cui viene eseguito `exec`.
- Google Drive e simili non si collegano qui, ma tramite le cartelle cloud del tuo Mac, vedi [Archiviazione cloud](09-cloud.md). Manta non ha un proprio accesso a Google; solo se inserisci nelle Impostazioni un tuo client OAuth **personale** compare qui anche “Google Drive”, vedi [Google Drive per esperti](09-cloud.md#experte).
