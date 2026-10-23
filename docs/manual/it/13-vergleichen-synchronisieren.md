# Confronta e sincronizza {#vergleichen}

**Confronta e sincronizza** mostra in cosa differiscono due cartelle e, se vuoi, le rende uguali: sul tuo Mac, tra il tuo Mac e un server, o tra due server. La finestra confronta due pannelli.

![La finestra Confronta e sincronizza](images/compare-sync.png)

## Confrontare due cartelle {#zwei-bereiche}

1. Apri una cartella nel pannello sinistro e l’altra nel pannello destro.
2. Premi **⇧⌘S** (**Confronta e sincronizza…**) o fai clic sul relativo pulsante nella barra degli strumenti.
3. Scegli le opzioni (vedi sotto) e fai clic su **Confronta di nuovo**.

L’elenco mostra ogni elemento con il suo stato:

| Stato | Significato |
|---|---|
| **Solo a sinistra** / **Solo a destra** | Esiste solo da un lato. |
| **Sinistra più recente** / **Destra più recente** | Su entrambi i lati, ma uno è più recente (data di modifica). |
| **Diversi** | Stessa data, ma dimensione diversa. |
| **Identici** | Stessa data e stessa dimensione. |

Manta confronta **data di modifica e dimensione**, non il contenuto byte per byte.

La legenda degli stati sopra l’elenco funziona anche da filtro: fai clic su uno stato per nasconderlo (“Fai clic per nascondere”), fai clic di nuovo per mostrarlo.

## Con tre o più pannelli {#a-b}

Con più di due pannelli scegli tu quali due confrontare:

- Invece di “Sinistra” e “Destra” i due lati della finestra si chiamano **Pannello A** e **Pannello B**. Nell’angolo in alto a destra di ogni riquadro un menu a comparsa sceglie di quale pannello si tratta (“Scegli quale pannello confrontare”). I pannelli sono numerati da sinistra a destra, ad esempio “2 · Download”; il pannello dell’altro lato è contrassegnato con “(A)” o “(B)”. Sceglierlo scambia A e B.
- Sono preselezionati il pannello attivo e il suo [pannello di destinazione](02-bereiche-tabs.md#zielbereich).
- Mentre la finestra è aperta, nella barra del percorso di ogni pannello compaiono piccoli pulsanti **A**/**B** (“Confronta questo pannello come A”). Il ruolo scelto è riempito di colore.

Gli stati si chiamano allora **Solo in A**, **Solo in B**, **A più recente** e **B più recente**; le colonne delle dimensioni **Pannello A** e **Pannello B**.

![Confronto con tre pannelli: scelta di Pannello A e Pannello B](images/compare-sync-ab.png)

**A è sempre a sinistra nella finestra, B a destra.** Così direzione e azioni mantengono i loro nomi: **Rispecchia da sinistra a destra** significa “da A a B”, “Copia →” copia da A a B, “Elimina a destra” elimina in B.

## Opzioni {#optionen}

- **Includi sottocartelle**: confronta anche il contenuto di tutte le sottocartelle.
- **File nascosti**: include file come `.env` o `.DS_Store`.
- **Direzione** (i tre pulsanti → ↔ ←):
  - **→ Rispecchia da sinistra a destra**: il lato destro diventa uguale al sinistro.
  - **↔ Prevale il file più recente**: in entrambe le direzioni; viene trasferito il file più recente.
  - **← Rispecchia da destra a sinistra**: il lato sinistro diventa uguale al destro.
- **Elimina file in più nella destinazione**: solo per la sincronizzazione unidirezionale. Elimina nella destinazione ciò che non esiste nell’origine.

Prima di iniziare, la colonna **Azione** mostra cosa succederà: “Copia →”, “← Copia”, “Elimina a destra” o “Elimina a sinistra”.

## Sincronizzare {#synchronisieren}

1. Confronta le cartelle come descritto sopra.
2. Scegli la direzione e controlla la colonna **Azione**.
3. Fai clic su **Sincronizza N elementi**.

I trasferimenti avvengono in background (“Sincronizzazione avviata: 12 trasferimenti in coda”). Segui l’avanzamento in **Trasferimenti** (**⌥⌘L**).

Se le cartelle coincidono, la finestra indica “Le due cartelle sono sincronizzate”.

> [!WARNING]
> La sincronizzazione **sovrascrive** i file nella destinazione senza chiedere di nuovo, ed **Elimina file in più nella destinazione** elimina definitivamente (vedi [Eliminare](05-dateien.md#loeschen)). Controlla la colonna **Azione** prima di iniziare.

Regole che ti proteggono:

- Durante il rispecchiamento, un file **più recente** nella destinazione non viene mai sovrascritto.
- Con **Prevale il file più recente**, i file con la stessa data ma dimensione diversa vengono saltati, perché non è chiaro quale sia quello giusto. **Elimina file in più nella destinazione** qui non si applica.
- Se un nome è una cartella da un lato e un file dall’altro, viene saltato.
- Se una cartella non può essere letta, il confronto si interrompe invece di eliminare le cose sbagliate sulla base di dati incompleti.

## La finestra {#fenster}

**Confronta e sincronizza** è una finestra di macOS a sé. Chiudila con il pulsante rosso della finestra o con **⌘W** (non con esc). Manta ricorda la sua posizione finché l’app è in esecuzione.

Con un solo pannello il comando è disattivato (“Servono due pannelli – ⌘D divide il pannello”).
