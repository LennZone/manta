# Archiviazione cloud {#cloud}

Manta mostra iCloud Drive, Google Drive, OneDrive, Dropbox e gli altri servizi cloud che il tuo Mac sincronizza già. Puoi vedere quali file sono solo nel cloud, scaricarli o rimuovere di nuovo il download locale.

![Sezione Cloud nella barra laterale e contrassegni di sincronizzazione nell’elenco](images/cloud-sidebar-badges.png)

## Come Manta raggiunge il cloud {#prinzip}

Manta **non** accede da solo a Google, Microsoft o Dropbox. Usa le cartelle che le app ufficiali dei fornitori creano sul tuo Mac:

- **iCloud Drive** è integrato in macOS.
- **Google Drive**, **OneDrive**, **Dropbox**, **Box** e altri creano le loro cartelle in `~/Library/CloudStorage` dopo che hai installato la loro app e hai effettuato l’accesso lì.

Per Manta sono normali cartelle del tuo Mac. Qualsiasi cosa tu faccia in Manta (copiare, rinominare, eliminare), è poi l’app del fornitore a sincronizzarla con il cloud.

## Configurare Google Drive {#google-drive}

1. Scarica **Google Drive per desktop** da Google e installalo.
2. Accedi all’app Google Drive con il tuo account Google.
3. **Google Drive** compare da solo sotto **Cloud** nella barra laterale: Manta si accorge delle nuove cartelle in `~/Library/CloudStorage`. Se non succede, fai clic sulla freccia circolare accanto a **Cloud**.

OneDrive e Dropbox si configurano allo stesso modo: installa l’app del fornitore, accedi, fatto.

> [!NOTE]
> Manta **non ha un proprio accesso a Google**. Senza Google Drive per desktop non vedrai Google Drive in Manta.

Finché non c’è una cartella di Google Drive, **Cloud** mostra una riga **Collega Google Drive…**. Fai clic per vedere i passaggi di configurazione e il link **Scarica Google Drive per desktop** (<https://www.google.com/drive/download/>). **Non mostrare più** nasconde il suggerimento; **Impostazioni › Generali › Cloud** lo fa ricomparire.

## La sezione Cloud nella barra laterale {#seitenleiste}

La sezione **Cloud** si trova tra **Posizioni** e **Dispositivi**. Ogni archivio cloud trovato ha una riga propria con l’icona del fornitore. Se un fornitore compare più di una volta (due account Google, il OneDrive della tua azienda e le librerie condivise), l’account segue in grigio; altrimenti lo mostra il suggerimento insieme al percorso:

- iCloud Drive (sempre per primo, quando è attivo)
- Google Drive (con più account: Google Drive *nome@gmail.com*)
- OneDrive *Personale* o OneDrive *nome della tua azienda*, OneDrive *Librerie condivise*
- Dropbox, Box e altri (ad esempio Nextcloud, Proton Drive, Synology Drive)

Un clic apre l’archivio cloud nel pannello attivo.

## iCloud Drive come nel Finder {#icloud-apps}

In Manta iCloud Drive appare come nel Finder: la cartella principale mostra le tue cartelle e i tuoi file **e** le cartelle delle tue app, ad esempio **Obsidian**, **Pages**, **Numbers** o **Comandi Rapidi**. Manta mostra le stesse cartelle delle app del Finder (le app che condividono i loro documenti in iCloud Drive e ne hanno già lì).

La barra del percorso inizia con l’indicatore **iCloud Drive**; in una cartella di un’app segue il nome dell’app, ad esempio **iCloud Drive › Obsidian › Vault**. I nomi delle cartelle con cui macOS salva tutto sul disco (`~/Library/Mobile Documents/com~apple~CloudDocs`, `iCloud~md~obsidian/Documents`) li vedi solo quando modifichi il percorso.

Una cartella di un’app appartiene alla sua app: non puoi rinominarla, spostarla o eliminarla; se ci provi, Manta te lo dice. Il suo contenuto lo modifichi normalmente.

## Spazio libero {#freier-speicher}

In basso nella barra di stato, dopo il numero di elementi, Manta mostra quanto spazio è ancora libero, ad esempio “12 elementi · 18 GB disponibili”. Il suggerimento mostra anche la dimensione totale, se è nota.

| Dove | Cosa significa “disponibili” |
|---|---|
| Cartelle sul tuo Mac | spazio libero sul disco, come nel Finder, quindi compreso lo spazio che macOS libera quando serve |
| iCloud Drive | quanto è ancora libero nel tuo spazio di archiviazione iCloud |
| Google Drive (connessione propria) | quanto è ancora libero nel tuo spazio di archiviazione Google; nei Drive condivisi niente |
| SFTP, SMB | spazio libero sul server |
| WebDAV (ad esempio Nextcloud) | la tua quota sul server, se il server la comunica |

Google Drive per desktop, OneDrive e Dropbox salvano i loro file sul tuo Mac; lì Manta mostra lo spazio libero sul disco. S3, FTP e i container non comunicano lo spazio libero: in quel caso l’indicazione manca. Dopo una copia, uno spostamento o un’eliminazione il valore viene riletto.


## Cosa significano i contrassegni {#abzeichen}

Nelle cartelle cloud Manta mostra lo stato di sincronizzazione: nell’elenco come icona all’estremità destra della colonna Nome, nella vista Icone come contrassegno rotondo in alto a destra sull’icona. Il suggerimento lo spiega a parole (ad esempio “Solo nel cloud – non scaricato”).

| Contrassegno | Significato | Azioni possibili |
|---|---|---|
| (nessuno) | Scaricato e aggiornato | Rimuovi download |
| Nuvola con freccia verso il basso | Solo nel cloud, non su questo Mac | Scarica ora |
| Nuvola con punto esclamativo | Scaricato, ma nel cloud c’è una versione più recente | Scarica ora, Rimuovi download |
| Cerchio con freccia verso il basso | Download in corso | – |
| Nuvola con freccia verso l’alto (blu, pulsante) | Caricamento in corso o in attesa di caricamento, ad esempio subito dopo una copia in Google Drive | – |
| Punto esclamativo rosso in un cerchio | Errore di caricamento: il fornitore non è riuscito a caricare l’elemento; il suggerimento indica il motivo (ad esempio “Errore di caricamento: spazio esaurito”) | – |

Dopo una copia in una cartella cloud, l’app del fornitore carica i file in background. Nel frattempo anche la barra di stato indica “Google Drive sta caricando 2 elementi” (in rosso in caso di errori). iCloud Drive può riportare anche l’avanzamento (“Caricamento in corso… 42%” nel suggerimento); Google Drive, OneDrive e Dropbox riportano solo “caricamento in corso”. I file che i fornitori non caricano mai (`.DS_Store`, file di blocco di Office `~$…`) non ricevono alcun contrassegno.

Leggere lo stato non scarica nulla, quindi puoi sfogliare grandi cartelle cloud senza che il tuo Mac scarichi tutto.

Manta legge lo stato dagli attributi dei file (il flag `SF_DATALESS` e i valori `NSURLUbiquitousItem…` di macOS, che iCloud Drive e le app File Provider comunicano) e lo aggiorna ogni pochi secondi mentre la cartella è visibile.

## Scaricare file dal cloud {#jetzt-laden}

1. Seleziona file o cartelle che si trovano solo nel cloud.
2. Clic destro → **Scarica ora**.

Le cartelle vengono scaricate con tutto il loro contenuto. Il contrassegno passa a “download in corso” e scompare quando tutto è arrivato.

Se apri o copi un file che si trova solo nel cloud, macOS lo scarica comunque automaticamente.

## Liberare spazio {#download-entfernen}

1. Seleziona file o cartelle scaricati.
2. Clic destro → **Rimuovi download**.

I file restano nel cloud e visibili in Manta, ma non occupano più spazio sul tuo Mac.

> [!IMPORTANT]
> **Rimuovi download** funziona in modo affidabile con **iCloud Drive**. Per Google Drive, OneDrive, Dropbox e altri decide l’app del fornitore se permetterlo. Se lo rifiuta, Manta te lo segnala. In quel caso usa il **Finder**: clic destro → **Rimuovi download**. Il Finder ha il permesso di farlo, le altre app no.

## Google Drive per esperti: un client OAuth personale {#experte}

Manta contiene anche una propria connessione a Google Drive (Drive API), che è **nascosta**: Manta non include un client OAuth per essa. Solo se non puoi usare Google Drive per desktop e hai un client tuo:

1. Nella Google Cloud Console, crea un client OAuth di tipo **App desktop** e attiva la Google Drive API.
2. Apri **Impostazioni › Generali › Cloud › Esperti: client OAuth Google personale**, inserisci l’ID client e il client secret e fai clic su **Salva**.
3. La finestra di connessione ora ha un tipo **Google Drive**. Se lasci vuoto **Account**, Manta apre il browser per l’accesso quando ti connetti.

**Rimuovi** toglie di nuovo Google Drive dalla finestra di connessione. Il client viene conservato in `~/Library/Application Support/Manta/gdrive-client.json`.

## Eliminare nelle cartelle cloud {#loeschen}

> [!IMPORTANT]
> Le cartelle cloud sono cartelle sul tuo Mac. **⌘⌫** sposta gli elementi che contengono nel **Cestino**, come ovunque (vedi [Eliminare](05-dateien.md#loeschen) e [Cestino](07-favoriten-seitenleiste.md#papierkorb)); **⌥⌘⌫** elimina definitivamente dopo una conferma. L’app del fornitore sincronizza poi l’eliminazione **nel cloud e su tutti i tuoi dispositivi**. A seconda del fornitore, il file resta per un po’ nel cestino del suo sito web.

Se vuoi solo risparmiare spazio, usa **Rimuovi download** invece di eliminare.

## Limiti {#grenzen}

- Manta mostra solo gli archivi cloud che il tuo Mac sincronizza già. Senza l’app del fornitore non funziona.
- **Rimuovi download** per i fornitori di terze parti funziona solo dove la loro app lo consente (altrimenti nel Finder).
- La velocità con cui le modifiche arrivano nel cloud dipende dall’app del fornitore, non da Manta.
