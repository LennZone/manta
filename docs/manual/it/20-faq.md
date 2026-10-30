# Domande frequenti {#faq}

Risposte brevi alle domande più frequenti. I capitoli collegati contengono i dettagli.

## Generali

### Manta sostituisce il Finder? {#finder-ersetzen}

Non del tutto. Puoi rendere Manta il tuo gestore file predefinito; allora “Mostra nel Finder” e (fino a macOS 15) le cartelle da altre app si aprono in Manta. La Scrivania, l’icona del Finder nel Dock e i doppi clic nelle finestre del Finder restano però sempre al Finder: macOS non permette a nessuna app di prenderli in carico. Vedi [Gestore file predefinito](01-erste-schritte.md#standard-dateimanager).

### Manta costa qualcosa o raccoglie dati? {#daten}

Manta non raccoglie dati di utilizzo. Si collega a Internet solo per le tue connessioni e per la verifica degli aggiornamenti, che invia solo il tuo indirizzo IP e la versione di Manta. Vedi [Privacy](17-updates.md#datenschutz).

Manta è gratuito e open source con licenza MIT: puoi usarlo, modificarlo e condividerlo liberamente. Il codice sorgente è su [GitHub](https://github.com/web-workers-com/manta) (nell’app dal menu **Aiuto** → **Codice sorgente su GitHub…**) e i contributi sono benvenuti. Novità, aiuto e contatti si trovano sul sito web [manta-filemanager.com](https://manta-filemanager.com/): nell’app dal menu **Aiuto** → **Sito web e supporto di Manta…** e nelle Impostazioni, in [Informazioni su Manta](16-einstellungen.md#ueber).

Manta si basa su componenti open source. Le relative note di licenza si trovano in **Aiuto** → **Licenze di terze parti** oppure [qui](../../third-party-licenses.md).

### Ho trovato un bug o ho un’idea: dove lo segnalo? {#feedback}

Su GitHub: **Aiuto** → **Segnala un problema…** apre il modulo per le segnalazioni di bug con le versioni di Manta e di macOS già compilate; **Aiuto** → **Suggerisci un’idea…** apre il modulo per le idee. Cosa deve contenere una buona segnalazione e dove si trova il file di log: vedi [Segnalare un bug](19-fehlerbehebung.md#fehler-melden). Segnala le vulnerabilità di sicurezza in privato, mai in una issue pubblica: vedi la [politica di sicurezza](https://github.com/web-workers-com/manta/blob/main/SECURITY.md).

### Manta è disponibile in italiano? {#sprache}

Sì. **Impostazioni** → **Generali** → **Lingua**: **Sistema** (predefinita, segue le lingue preferite di macOS) oppure una delle sette lingue dell’app: **Deutsch**, **English**, **简体中文**, **Español**, **Français**, **Italiano**, **Русский**. Il cambio ha effetto subito. Anche il manuale (**⌘?**) segue la lingua dell’app; se non è disponibile in quella lingua, Manta mostra la versione inglese.

### Quanti pannelli posso aprire? {#bereiche}

Quanti ne entrano nella finestra: ognuno ha bisogno di circa 280 punti di larghezza. Almeno uno resta sempre aperto. Vedi [Pannelli e schede](02-bereiche-tabs.md).

## File

### Dov’è il Cestino? Posso recuperare i file eliminati? {#papierkorb}

Sì, per gli elementi sul tuo Mac: **⌘⌫** li sposta nel Cestino di macOS, come il Finder. Subito dopo, **⌘Z** (**Annulla**) li rimette dov’erano; più tardi li recuperi con **Ripristina** del Finder. **⌥⌘⌫** (**Elimina immediatamente…**) elimina definitivamente dopo una conferma. Server, S3 e container non hanno un Cestino: lì l’eliminazione è sempre definitiva e Manta chiede prima conferma. Vedi [Eliminare](05-dateien.md#loeschen).

### Trascinando si copia o si sposta? {#ziehen}

Manta chiede, con **Copia** preselezionato, anche sullo stesso volume, dove il Finder sposterebbe. Così nulla sparisce per errore dalla sua cartella. Tieni premuto **⌘** mentre rilasci per preselezionare **Sposta**; **⌥** preseleziona **Copia**. Il contrassegno accanto al puntatore ti dice in anticipo cosa è preselezionato. Nelle [Impostazioni](16-einstellungen.md#kopieren-bewegen) puoi invece scegliere “Copia sempre” o “Sposta sempre” (senza finestra). Il rilascio sul Cestino sposta sempre nel Cestino. Vedi [Trascinare e rilasciare](05-dateien.md#drag-drop).

### Ho aperto un file da un server e l’ho modificato: dov’è la mia modifica? {#server-datei}

In una copia temporanea sul tuo Mac. Manta non carica automaticamente i file modificati. Copia il file in una cartella locale, modificalo lì e ricopialo sul server con **F5**. Vedi [Navigazione](04-navigation.md#oeffnen).

### Perché non vedo i miei file `.env` o `.git`? {#unsichtbar}

I file che iniziano con un punto sono nascosti. Premi **⇧⌘.**. Vedi [File nascosti](04-navigation.md#unsichtbare-dateien).

### Perché una cartella non si aggiorna quando un’altra app aggiunge file? {#aktualisieren}

Premi **⌘R**. Manta rilegge le cartelle dopo le proprie azioni, ma non quando altre app cambiano qualcosa.

### Posso modificare i file dentro gli archivi ZIP? {#archiv-aendern}

No, gli archivi sono di sola lettura. Estraili, modifica i file e comprimili di nuovo (**⌥⌘C**). Vedi [Archivi](10-archive.md).

### Cosa succede a una copia se cade il Wi-Fi o se esco da Manta? {#unterbrochen}

Non si perde nulla. Quando cade la rete, il trasferimento aspetta e riprende dallo stesso punto non appena torna la connessione. Quando esci da Manta (o se si chiude in modo anomalo), all’avvio successivo chiede “Riprendere i trasferimenti interrotti?”. Vedi [Sospendere e riprendere i trasferimenti](05-dateien.md#pausieren).

### Cosa sono i file `.NAME.manta-part`? {#manta-part}

File parziali nascosti in cui Manta scrive durante la copia. Solo il file completato riceve il suo vero nome. Vedi [File parziali](05-dateien.md#teildateien).

## Connessioni e cloud

### Le password dei miei server sono conservate in modo sicuro? {#passwoerter}

Sì. Con **Ricorda password**, Manta salva password, passphrase e chiavi segrete S3 nel Portachiavi di macOS, non in un file. Le password che le versioni precedenti salvavano senza codifica vengono spostate lì all’avvio. Senza **Ricorda password**, Manta chiede ogni volta. SFTP con ssh-agent non richiede affatto una password salvata. Vedi [Connessioni](08-verbindungen.md#favoriten).

### Posso accedere direttamente a Google Drive? {#google}

No. Installa **Google Drive per desktop** e accedi lì; Manta mostrerà poi la cartella di Google Drive. Vedi [Archiviazione cloud](09-cloud.md#google-drive).

### Manta supporta WebDAV, SMB o Dropbox senza l’app? {#protokolle}

Sì. Manta si connette direttamente a SFTP, FTP, FTPS, [WebDAV](08-verbindungen.md#webdav), [SMB](08-verbindungen.md#smb), S3 e ai container: premi **⌘K** (**Connessione al server…**). Le condivisioni montate nel Finder compaiono anche sotto **Dispositivi**. Dropbox, OneDrive e simili Manta li mostra tramite le app dei fornitori.

### Perché c’è uno scudo di avviso sull’indicatore FTP? {#ftp-warnung}

La tua rete blocca le connessioni dati FTP codificate. Manta trasferisce quindi i file senza codifica; l’accesso resta codificato. Vedi [Canale dati](08-verbindungen.md#datenkanal).

### Manta può avviare o arrestare i container? {#container}

No. Manta mostra e modifica solo i file nei container. Avvia e arresta i container con Docker Desktop o nel terminale.

## Estendere Manta

### Posso aggiungere comandi miei senza scrivere un plug-in? {#eigene-befehle}

Sì, con le **Azioni rapide**: basta una voce con un comando shell in `actions.json`. Vedi [Azioni rapide](14-plugins.md#actions-json).

### I plug-in sono sicuri? {#plugins-sicher}

I plug-in integrati fanno parte di Manta. I plug-in esterni vengono eseguiti in una sandbox di macOS e possono fare solo ciò che i loro permessi consentono; installa comunque solo ciò di cui ti fidi. Vedi [Plug-in personali](15-eigene-plugins.md#sicherheit).
