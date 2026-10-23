# Archivi {#archive}

Manta apre gli archivi ZIP e TAR come cartelle: ne vedi subito il contenuto, senza doverli prima estrarre. Questo capitolo spiega anche come estrarre gli archivi e come crearne di nuovi.

## Formati supportati {#formate}

- `.zip`
- `.tar`, `.tar.gz` / `.tgz`, `.tar.bz2`, `.tar.xz`, `.tar.zst`
- singoli file compressi: `.gz`, `.bz2`, `.xz`, `.zst`

## Aprire un archivio come cartella {#oeffnen}

1. Seleziona l’archivio.
2. Premi **↩** o fai doppio clic.

La barra del percorso mostra l’archivio come una cartella, ad esempio `Download › backup.zip › docs`. Un indicatore con un lucchetto contrassegna il pannello come **archivio di sola lettura**.

- **⌫** o **⌘↑** nella cartella principale dell’archivio torna alla cartella che contiene l’archivio, con l’archivio selezionato.
- **Indietro** e **Avanti** funzionano anche attraverso l’archivio.

![Un archivio ZIP aperto come cartella](images/archive-as-folder.png)

Altre possibilità:

- **⌥↩** (**Apri con l’app predefinita**) apre l’archivio con la sua app predefinita (ad esempio Utility Compressione di macOS).
- I documenti che internamente sono file ZIP (`.docx`, `.xlsx`, `.jar`, `.apk` …) continuano ad aprirsi nella loro app. Clic destro → **Sfoglia archivio** ne mostra il contenuto.
- Se non vuoi mai che gli archivi si aprano come cartelle, disattiva **Impostazioni** → **Generali** → **Apri gli archivi come cartelle**.

Gli archivi sui server e gli archivi dentro altri archivi vengono prima scaricati in un file temporaneo (con avanzamento e **Annulla** nel pannello). Gli archivi grandi vengono letti in background.

## Cosa funziona in un archivio, e cosa no {#schreibgeschuetzt}

Gli archivi sono di **sola lettura**. Manta rifiuta, con un suggerimento, nuove cartelle, rinomina, eliminazione, spostamento e rilascio di file nell’archivio.

**Copiare all’esterno** funziona sempre:

- con **F5** nell’altro pannello,
- trascinando in un altro pannello,
- con **Estrai qui** o **Estrai nell’altro pannello**.

## Estrarre un archivio {#entpacken}

**Un archivio intero:**

1. Seleziona il file dell’archivio.
2. Premi **⌥⌘E** (**Estrai qui**), oppure clic destro → **Estrai qui** o **Estrai nell’altro pannello**.

**Estrai qui** estrae accanto all’archivio: in una cartella con il nome dell’archivio, oppure direttamente se l’archivio contiene un solo elemento.

**Singoli elementi da un archivio aperto:**

1. Apri l’archivio come cartella e seleziona gli elementi.
2. **⌥⌘E** li estrae nella cartella che contiene l’archivio. **F5** o clic destro → **Estrai nell’altro pannello…** li estrae nell’altro pannello.

L’estrazione avviene come trasferimento in background (vedi [Trasferimenti](05-dateien.md#uebertragungen)).

## Creare un archivio {#komprimieren}

1. Seleziona file e cartelle.
2. Premi **⌥⌘C** (**Comprimi…**) o scegli il comando nel menu contestuale.
3. Scegli il **Formato**: zip, tar.gz, tar.xz o tar.zst.
4. Controlla il **Nome**. Manta ne propone uno, ad esempio “site.zip” o “Archivio 2.zip”.
5. Con il cursore **Livello** scegli tra **Più veloce** e **Più piccolo**.
6. **Escludi .DS_Store e __MACOSX** è attivo, così nessun file ausiliario del Mac finisce nell’archivio.
7. Fai clic su **Comprimi**.

L’archivio viene creato nella cartella attuale. Funziona anche sui server: Manta crea l’archivio in un file temporaneo locale e lo carica.

![Finestra Comprimi](images/compress-sheet.png)

> [!TIP]
> Per chi usa un Mac, **zip** è il formato più comodo. **tar.zst** è molto veloce e compatto, ma richiede uno strumento adatto dall’altra parte.

## Sicurezza {#sicherheit}

- Le voci con `..` o con percorsi assoluti potrebbero, durante l’estrazione, scrivere fuori dalla cartella di destinazione. Manta non le mostra né le estrae mai e segnala “N elementi non sicuri … saltati”.
- Le voci ZIP codificate hanno un contrassegno con un lucchetto. Manta non può aprirle né estrarle e non chiede una password.

## Limiti {#grenzen}

- Nessuna modifica all’interno degli archivi (aggiungere, eliminare, rinominare).
- Nessuna voce ZIP protetta da password.
- I link simbolici vengono mostrati come link; l’estrazione copia il file di destinazione e i link alle cartelle vengono saltati.
- I TAR compressi oltre i 2 GB diventano più lenti, perché Manta li rilegge dall’inizio per ogni file.
- Gli archivi ZIP suddivisi in più parti e i file sparse nei TAR non sono supportati.
