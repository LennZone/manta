# Primi passi {#erste-schritte}

Manta è un gestore file per Mac con più pannelli affiancati. Questo capitolo spiega come installare Manta, cosa succede al primo avvio e perché macOS ti chiede delle autorizzazioni.

![Manta con due pannelli e la barra laterale](images/overview-two-panes.png)

## Requisiti di sistema {#voraussetzungen}

- macOS 11 (Big Sur) o versioni successive.
- Un Mac con chip Apple (M1 o successivo).

## Installare Manta {#installieren}

1. Fai doppio clic su `Manta-<version>.dmg`.
2. Nella finestra che si apre, trascina **Manta** sulla cartella **Applications** (è la cartella “Applicazioni”).

   ![La finestra dell’immagine disco: Manta viene trascinato sulla cartella Applications, evidenziata](images/install-dmg.png)

3. Espelli l’immagine disco (fai clic sull’icona di espulsione nel Finder).
4. Avvia Manta dalla cartella **Applicazioni**, ad esempio con Launchpad o Spotlight.

> [!IMPORTANT]
> Avvia sempre Manta dalla cartella **Applicazioni**, non direttamente dall’immagine disco. Solo così Manta può aggiornarsi da solo ([Aggiornamenti](17-updates.md)) e diventare il tuo gestore file predefinito.

## Il primo avvio: Gatekeeper {#gatekeeper}

Manta non è autenticato da Apple, quindi macOS blocca il primo avvio con il messaggio *Manta non è stata aperta*. È una funzione di sicurezza di macOS (Gatekeeper), non un errore di Manta. Per aprire comunque Manta (basta farlo una volta):

1. Avvia Manta. macOS mostra *Manta non è stata aperta*. Fai clic su **Fine**, non su **Sposta nel Cestino**, altrimenti dovrai scaricare di nuovo Manta.

   ![Il messaggio Manta non è stata aperta con i pulsanti Fine e Sposta nel Cestino; Fine è evidenziato](images/gatekeeper-blocked.png)

2. Apri **Impostazioni di Sistema** → **Privacy e sicurezza** e scorri fino alla sezione **Sicurezza**. Lì c’è scritto *Il Mac ha bloccato Manta per garantire la sicurezza.* Fai clic su **Apri comunque**, lì accanto.

   ![Impostazioni di Sistema, Privacy e sicurezza, sezione Sicurezza: il pulsante Apri comunque accanto all’avviso su Manta è evidenziato](images/gatekeeper-open-anyway.png)

3. macOS chiede ancora una volta: *Vuoi aprire Manta?* Fai clic su **Apri comunque** e conferma con la password o con Touch ID. Manta si avvia.

   ![La domanda Vuoi aprire Manta? con i pulsanti Apri comunque e Fine; Apri comunque è evidenziato](images/gatekeeper-confirm.png)

D’ora in poi Manta si avvia come qualsiasi altra app. Gli aggiornamenti che Manta installa da solo ([Aggiornamenti](17-updates.md)) non richiedono di ripetere questi passaggi.

Non vedi **Apri comunque**? Il pulsante compare solo dopo un tentativo di avvio e resta disponibile per circa un’ora. Avvia di nuovo Manta, fai clic su **Fine** e guarda subito in **Privacy e sicurezza**.

Nelle versioni precedenti a macOS 15 basta fare clic su Manta nel Finder tenendo premuto Ctrl e scegliere **Apri**.

> [!TIP]
> **Per utenti esperti:** invece di usare Impostazioni di Sistema puoi togliere il blocco nel Terminale: `xattr -dr com.apple.quarantine /Applications/Manta.app`. Il comando rimuove da Manta il contrassegno di download (quarantena); dopo Manta si avvia senza chiedere. Fallo solo con una copia di Manta che hai scaricato tu da manta-filemanager.com o da GitHub.

Vedi anche [Risoluzione dei problemi](19-fehlerbehebung.md#gatekeeper).

## Perché macOS chiede le autorizzazioni {#berechtigungen}

macOS protegge determinate cartelle e funzioni. Ogni app deve chiedere il permesso una volta prima di usarle, anche un gestore file. Le richieste non arrivano tutte all’avvio: ognuna compare la prima volta che apri quella cartella o usi quella funzione.

| macOS chiede di … | Quando | Perché serve a Manta |
|---|---|---|
| Le cartelle Scrivania, Documenti e Download | La prima volta che apri la cartella | Per mostrare e gestire i file che contiene. |
| Volumi rimovibili, volumi di rete | La prima volta che apri un’unità USB o una condivisione di rete | Lo stesso, per unità esterne e condivisioni su server. |
| Rete locale | La prima volta che ti connetti a un server nella tua rete | Per server SFTP, FTP e S3 nella rete locale (ad esempio un NAS). |
| Automazione (Finder, Terminale) | Quando una funzione controlla il Finder o il Terminale | Ad esempio per spostare elementi nel Cestino (Duplicati), mostrare file nel Finder o eseguire azioni rapide come “Apri in Terminal.app”. |

Se fai clic su “Non consentire”, Manta mostra un avviso quando apri la cartella. Puoi concedere l’accesso in seguito: **Impostazioni di Sistema** → **Privacy e sicurezza** → **File e cartelle** (oppure **Automazione**, **Rete locale**).

> [!TIP]
> Alcune cartelle (come `~/Library/Mail` o i dati di Time Machine) si aprono solo con l’**Accesso completo al disco**. Puoi concederlo in **Impostazioni di Sistema** → **Privacy e sicurezza** → **Accesso completo al disco**. È facoltativo: alla maggior parte delle persone non serve.

> [!NOTE]
> Manta non raccoglie dati di utilizzo. Si collega a Internet solo per le connessioni che apri tu e per verificare la disponibilità di aggiornamenti (vedi [Aggiornamenti e privacy](17-updates.md#datenschutz)).

## Gestore file predefinito {#standard-dateimanager}

Ai primi avvii Manta chiede: **Vuoi usare Manta come gestore file predefinito?**

- **Imposta come predefinito**: “Mostra nel Finder” nelle altre app apre Manta, e lo stesso vale per le cartelle aperte da altre app o dal Terminale (ad esempio `open ~/Downloads`).
- **Non ora**: Manta te lo chiede di nuovo a un avvio successivo (al massimo tre volte).
- **Non chiedere più**: Manta non te lo chiede più.

Puoi cambiare idea in qualsiasi momento in **Impostazioni** → **Generali** → **Gestore file predefinito**, oppure con **Gestore file predefinito…** nella palette dei comandi.

![La domanda sul gestore file predefinito](images/default-file-manager-prompt.png)

Cosa permette macOS, e cosa no:

- La Scrivania, l’icona del Finder nel Dock e i doppi clic nelle finestre del Finder restano **sempre** al Finder. macOS non permette a nessuna app di sostituire completamente il Finder.
- Da macOS 26 in poi, macOS affida le cartelle aperte da altre app solo al Finder. In quel caso Manta prende in carico solo “Mostra nel Finder”; le Impostazioni te lo indicano.
- Se disattivi l’opzione, il ruolo torna all’app precedente (ad esempio un altro gestore file) o al Finder.
- L’opzione è disponibile solo se Manta viene eseguito da **Applicazioni** (o da `~/Applications`).

## La finestra in breve {#ueberblick}

- **Barra degli strumenti** in alto: Nuova cartella, Rinomina, Elimina, Copia/Sposta nell’altro pannello, file nascosti, Confronta e sincronizza, Trasferimenti, Connetti, Comandi e Impostazioni.
- **Barra laterale** a sinistra: Preferiti, Posizioni, Dispositivi, Server e Container (vedi [Preferiti e barra laterale](07-favoriten-seitenleiste.md)).
- **Pannelli**: uno o più elenchi di file affiancati, ognuno con schede e barra del percorso (vedi [Pannelli e schede](02-bereiche-tabs.md)).
- **Barra di stato** in basso: il numero di elementi, la selezione, lo spazio libero (vedi [Spazio libero](09-cloud.md#freier-speicher)), i trasferimenti in corso, il carico di sistema (CPU, GPU, RAM; vedi [Carico di sistema](14-plugins.md#systemauslastung)) e un suggerimento per la palette dei comandi.

## La palette dei comandi {#befehlspalette}

Ogni comando di Manta si trova nella **palette dei comandi**, anche quelli dei plug-in.

1. Premi **⇧⌘P** (oppure **⌘P**).
2. Digita qualche lettera, ad esempio `nuova cart` o `sftp`.
3. Scegli con ↑ e ↓ e premi ↩.

Accanto a ogni comando la palette mostra la sua scorciatoia da tastiera, così le impari strada facendo.

![Palette dei comandi con una ricerca](images/command-palette.png)

> [!NOTE]
> Nella barra dei menu Manta ha solo il menu **Manta** (Informazioni su Manta, Verifica aggiornamenti…, Impostazioni…, Esci). Tutti gli altri comandi si trovano nella barra degli strumenti, nel menu contestuale (clic destro) e nella palette dei comandi.

## Prossimi passi

- [Pannelli e schede](02-bereiche-tabs.md): lavorare con più pannelli.
- [Gestire i file](05-dateien.md): copiare, spostare, rinominare, eliminare.
- [Scorciatoie da tastiera](18-tastenkuerzel.md): tutte le scorciatoie in un colpo d’occhio.
