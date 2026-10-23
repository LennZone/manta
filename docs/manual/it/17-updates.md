# Aggiornamenti {#updates}

A ogni avvio Manta verifica senza disturbare se c’è una nuova versione e la installa solo se sei d’accordo. Questo capitolo spiega come funziona, cosa passa in rete e cosa fare quando un aggiornamento non riesce.

## Come vieni a sapere di un aggiornamento {#hinweis}

Quando c’è una nuova versione vedi:

- un avviso “Aggiornamento disponibile: 0.2.0” con un pulsante **Mostra**,
- un piccolo suggerimento nella barra di stato (“Aggiornamento 0.2.0”).

Non compare nulla sopra il tuo lavoro. Se Manta è offline non te ne accorgi; l’errore finisce solo nel log.

## Verificare gli aggiornamenti manualmente {#suchen}

- **Manta** → **Verifica aggiornamenti…** nella barra dei menu, oppure
- **Verifica aggiornamenti…** nella palette dei comandi, oppure
- **Impostazioni** → **Generali** → **Aggiornamenti** → **Verifica ora**.

Se tutto è aggiornato, Manta indica “Manta è aggiornato”.

## Installare un aggiornamento {#installieren}

1. Fai clic su **Mostra** nell’avviso (o sul suggerimento nella barra di stato). Si apre la finestra **Aggiornamento software**.
2. Leggi le **Novità**. Sopra trovi dimensione e data di pubblicazione.
3. Scegli:
   - **Installa e riavvia**: scarica l’aggiornamento, lo verifica, sostituisce l’app e riavvia Manta.
   - **Più tardi**: te lo ricorda di nuovo all’avvio successivo.
   - **Salta questa versione**: questa versione e quelle precedenti non vengono più proposte.

![La finestra Aggiornamento software](images/software-update.png)

Durante il download vedi l’avanzamento e puoi annullare. Se ci sono ancora trasferimenti in corso, Manta aspetta che finiscano prima di riavviarsi. macOS può chiederti la password se Manta non può scrivere nella cartella **Applicazioni**.

Se qualcosa va storto, la finestra ne mostra il motivo, con **Riprova**. La versione attuale resta invariata.

## Quando Manta non può aggiornarsi da solo {#blockiert}

| Messaggio | Soluzione |
|---|---|
| “Manta è in esecuzione da un’immagine disco” | Trascina Manta nella cartella **Applicazioni** e aprilo da lì. |
| “Manta è in esecuzione da una posizione temporanea” | macOS ha avviato Manta da una copia di sola lettura (app translocation). Sposta Manta nella cartella **Applicazioni** con il Finder e aprilo da lì. |
| “Gli aggiornamenti non sono configurati in questa build” | La tua build non può verificare gli aggiornamenti. Scarica le nuove versioni dal sito web. |

## Canali: Stabile e Beta {#kanaele}

In **Impostazioni** → **Generali** → **Aggiornamenti** → **Canale** scegli:

- **Stabile** (predefinito): solo versioni definitive.
- **Beta**: anche le versioni preliminari. Portano prima le nuove funzioni, ma possono contenere errori.

Manta non propone mai una versione più vecchia di quella installata.

## Sicurezza {#sicherheit}

Ogni aggiornamento è firmato digitalmente. Manta installa solo ciò che è firmato con la chiave del team di Manta e corrisponde esattamente alla versione annunciata. Un download manomesso viene rifiutato (“L’aggiornamento non è firmato correttamente, quindi non è stato installato nulla.”). Gli aggiornamenti arrivano solo tramite HTTPS.

## Privacy: cosa invia la verifica degli aggiornamenti {#datenschutz}

Per la verifica, Manta scarica dal server degli aggiornamenti (`manta-filemanager.com`) due piccoli file: l’elenco delle versioni e la sua firma. Ecco cosa passa in rete:

- il tuo **indirizzo IP** (come per qualsiasi richiesta web),
- l’identificativo **`Manta-Updater/<version>`**, cioè quale versione di Manta usi.

Manta **non** invia ID utente, identificativi del dispositivo, nomi di file o dati di utilizzo. Quale versione fa al caso tuo (canale, versione di macOS, versione saltata) lo decide Manta sul tuo Mac, non il server.

Se non lo desideri, disattiva **Verifica aggiornamenti all’avvio**. Manta si collegherà allora a Internet per gli aggiornamenti solo quando scegli tu **Verifica aggiornamenti…**.

Come quasi tutti i siti web, il server web registra queste richieste con l’indirizzo IP. Il provider di hosting lo fa secondo le proprie regole di conservazione; Manta stesso non analizza le richieste e non le collega a nulla.
