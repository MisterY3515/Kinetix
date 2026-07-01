# CLAUDE.md

Linee guida comportamentali per il workspace. Questo file unisce le linee guida generali per ridurre gli errori comuni di un LLM durante la scrittura di codice con le regole operative specifiche del progetto.

**Compromesso:** queste linee guida privilegiano la cautela rispetto alla velocità. Per attività banali, usa il buon senso.

## 1. Pensa prima di scrivere codice

Non fare supposizioni. Non nascondere l'incertezza. Esponi i compromessi.

Prima di implementare:
- Dichiara esplicitamente le tue assunzioni. Se sei incerto, chiedi.
- Se esistono più interpretazioni, presentale tutte — non scegliere in silenzio.
- Verifica sempre la correttezza delle affermazioni creative dell'utente.
- Se esiste un approccio più semplice, dillo. Fai notare quando è opportuno.
- Se qualcosa non è chiaro, fermati. Indica cosa ti confonde. Chiedi.
- In caso di idee migliori, consulta l'utente in merito.

## 2. Semplicità prima di tutto

Il minimo codice che risolve il problema. Niente di speculativo.

- Nessuna funzionalità oltre a quanto richiesto.
- Nessuna astrazione per codice a uso singolo.
- Nessuna "flessibilità" o "configurabilità" non richiesta.
- Nessuna gestione di errori per scenari impossibili.
- Cerca sempre la soluzione più semplice ma efficace.
- Se scrivi 200 righe e potrebbero essere 50, riscrivi.

Chiediti: "Un ingegnere senior direbbe che è troppo complicato?" Se sì, semplifica.

## 3. Modifiche chirurgiche

Tocca solo ciò che è necessario. Pulisci solo il tuo stesso disordine.

Quando modifichi codice esistente:
- Non "migliorare" codice, commenti o formattazione adiacenti non richiesti.
- Non refactorizzare cose che non sono rotte.
- Rispetta lo stile esistente, anche se tu faresti diversamente.
- Non rimuovere funzionalità esistenti a meno che non sia richiesto, specialmente durante il refactoring.
- Se una funzionalità viene rimossa per errore, deve essere ripristinata.
- Presta attenzione al contesto e alla logica del codice: non danneggiare il codice esistente. Verifica sempre il codice esistente per capire come funziona prima di modificarlo, e controlla la zona modificata per verificarne la correttezza dopo la modifica.
- In caso di errore, non ripristinare da Git o da backup il codice esistente, a meno che non venga richiesto esplicitamente: correggi piuttosto il codice esistente.
- Se noti codice morto non collegato alle tue modifiche, segnalalo — non eliminarlo.

Quando le tue modifiche creano elementi orfani:
- Rimuovi import/variabili/funzioni che le TUE modifiche hanno reso inutilizzati.
- Non rimuovere codice morto preesistente a meno che non venga richiesto.

Il test: ogni riga modificata deve essere direttamente collegabile alla richiesta dell'utente.

## 4. Sicurezza prima di tutto

- A parità di altre condizioni, dai sempre priorità alla sicurezza.
- Mantieni sempre il codice sicuro, prestando particolare attenzione a non danneggiare o svalutare le funzioni di sicurezza.
- Durante il troubleshooting, non compromettere l'obiettivo finale, la sicurezza del codice o la sua stabilità.
- Non fare nulla di simulato (mock, placeholder, dati finti) a meno che non sia esplicitamente richiesto.

## 5. Licenze e copyright

- Rispetta sempre il copyright delle dipendenze e le relative licenze, senza entrare in conflitto con esse.
- Usa licenze semplici, libere, non virali e open source.
- Non usare mai sorgenti o librerie che non siano al 100% open source e libere nell'utilizzo, nella distribuzione e nella modifica, e che non impongano la pubblicazione del codice sorgente o l'adozione di licenze specifiche.
- Segui sempre le richieste previste dal copyright delle dipendenze (es. registrando le THIRD PARTY LICENSES in un file dedicato).

## 6. Esecuzione guidata dagli obiettivi

Definisci criteri di successo verificabili. Itera finché non sono verificati.

Trasforma i compiti in obiettivi verificabili:
- "Aggiungi validazione" → "Scrivi test per input non validi, poi falli passare"
- "Correggi il bug" → "Scrivi un test che lo riproduce, poi fallo passare"
- "Refactorizza X" → "Assicurati che i test passino prima e dopo"

Per attività multi-step, dichiara un breve piano:
```
1. [Passo] → verifica: [controllo]
2. [Passo] → verifica: [controllo]
3. [Passo] → verifica: [controllo]
```

Prima procedi con l'implementazione di una funzionalità senza fermarti, poi procedi al troubleshooting.

Criteri di successo solidi permettono di iterare in autonomia. Criteri deboli ("falla funzionare") richiedono chiarimenti costanti.

## 7. Regole del workspace e rilettura

- Segui sempre queste regole e rileggile a ogni attività.
- Dai priorità all'aggiornamento dei file nella cartella "Gestione" a ogni passo.
- Aggiorna i file in "Gestione" quando ti vengono fornite nuove istruzioni importanti.

## 8. Struttura della cartella "Gestione"

- **`Gestione/linee_guida.md`**: contiene le istruzioni fondamentali per il workspace. Se non esiste, crealo; altrimenti seguine le indicazioni. Aggiorna questo file (o altri .md nella cartella) quando vengono fornite nuove istruzioni importanti.
- **`Gestione/Funzioni/`**: contiene tutti i file .md con la documentazione dettagliata di funzioni, oggetti e azioni — implementate, sperimentali e pianificate — del progetto. Con "funzioni" si intendono sia le azioni eseguibili dall'utente sia quelle eseguibili dal programma (incluse impostazioni e simili). Le funzioni sono raggruppate in sotto cartelle per facilitare l'indicizzazione e la ricerca. Aggiorna o crea automaticamente i .md delle varie funzioni (ed eventuali sotto cartelle). Contiene anche un `README.md` con le informazioni e le istruzioni per documentare le funzioni (crealo se non esiste) e un `INDICE.md` con l'elenco di tutte le funzioni e la loro posizione (crealo se non esiste).
- **`Gestione/Istruzioni/`**: contiene le istruzioni per documentare le varie funzioni in modo preciso. Se non esiste, crealo; aggiornalo a ogni modifica.
- **`Gestione/Attività/`**: contiene le operazioni eseguite. Aggiorna automaticamente quando vengono assegnati nuovi compiti, segnando data, ora e descrizione di ogni operazione, oltre al suo stato. Raggruppa le attività in quattro sotto cartelle: "In corso", "Completate", "Annullate" e "Pianificate", a loro volta suddivise in altre categorie per facilitare l'indicizzazione e la ricerca. Ignora il troubleshooting in questo tracciamento.

## 9. Coerenza multipiattaforma

Le impostazioni devono essere sincronizzate/equivalenti tra le piattaforme.

## 10. Commit e push

Quando fai commit e push, scrivi le descrizioni in inglese, a meno che non sia esplicitamente richiesto diversamente dalle regole del workspace o dall'utente.

---

**Queste linee guida funzionano se:** meno modifiche non necessarie nei diff, meno riscritture dovute a eccessiva complessità, le domande di chiarimento arrivano prima dell'implementazione (non dopo gli errori), la documentazione in "Gestione" resta aggiornata, e nessuna funzionalità esistente viene persa.
