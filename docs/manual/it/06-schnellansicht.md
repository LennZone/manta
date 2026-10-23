# Visualizzazione rapida e anteprima {#schnellansicht}

Premi la **barra spaziatrice** per guardare un file senza aprirlo. Per i file sul tuo Mac Manta usa **Visualizzazione rapida** di Apple, come il Finder; per i file sui server e negli archivi c’è un’**anteprima integrata**.

## Guardare i file al volo {#leertaste}

1. Seleziona un file.
2. Premi la **barra spaziatrice** (o **⌘Y**, come nel Finder).
3. Usa **↑** e **↓** per passare al file precedente o successivo; il pannello di anteprima segue.
4. La **barra spaziatrice** o **esc** chiude l’anteprima.

Con più file selezionati, le frecce **‹** e **›** in Visualizzazione rapida scorrono la selezione.

La tastiera resta in Manta: puoi continuare a muoverti nell’elenco con i tasti freccia mentre il pannello di Visualizzazione rapida è aperto.

![Visualizzazione rapida sopra un pannello](images/quick-look.png)

## Visualizzazione rapida (macOS) {#quick-look}

Visualizzazione rapida è il pannello di anteprima di Apple. Mostra tutto ciò che mostra il Finder: PDF, immagini, video, documenti Office, file di testo e altro ancora. Funziona con file e cartelle **locali**.

## Anteprima integrata {#integrierte-vorschau}

I file sui **server** (SFTP, FTP, S3, container) e **negli archivi** si aprono nella finestra di anteprima di Manta, perché Visualizzazione rapida di Apple conosce solo i file locali.

- L’anteprima integrata mostra **file di testo** e **immagini**.
- Per i file di testo grandi mostra l’inizio (“Vengono mostrati i primi … del file.”).
- Per altri tipi di file (ad esempio PDF o video) indica di che file si tratta e propone di aprirlo con ↩. Manta allora lo scarica e lo apre con l’app predefinita.
- Non mostra le cartelle; premi ↩ per aprirne una. (Visualizzazione rapida di Apple mostra le cartelle locali.)
- Per chiuderla: **barra spaziatrice** o **⌘W** (è una finestra a sé con i soliti pulsanti delle finestre).

![Anteprima integrata di un file di testo da un server](images/preview-builtin.png)

## Cambiare cosa fa la barra spaziatrice {#einstellung}

In **Impostazioni** → **Generali** → **Anteprima** → **Barra spaziatrice** scegli:

- **Visualizzazione rapida (macOS)**: l’impostazione predefinita.
- **Anteprima integrata**: anche per i file locali.

I file sui server e negli archivi usano sempre l’anteprima integrata.

## Altre anteprime dai plug-in {#plugins}

- **Anteprima Markdown** (**⌥⌘M**): mostra i file `.md` ben impaginati in una barra laterale; vedi [Anteprima Markdown](14-plugins.md#markdown).
- **Strumenti immagine**: mostrano dimensioni, profilo colore e dati EXIF di un’immagine; vedi [Strumenti immagine](14-plugins.md#bildwerkzeuge).

## Limiti {#grenzen}

- Visualizzazione rapida funziona solo con i file locali. Manta non scarica (ancora) i file dei server per mostrarli in Visualizzazione rapida.
- L’anteprima integrata mostra solo testo e immagini.
