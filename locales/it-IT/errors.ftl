# Mensaggi di errore del motore. Chiave: error-<codice>; i codici rispecchiano gridcraft_engine::EngineError.
# Testo originale di GridCraft in italiano, tradotto da en-US; non copiato da documentazione di terzi.

error-unknown-command = Comando sconosciuto “{ $command }”.
error-command-disabled = “{ $command }” non è disponibile in questo momento: { $reason }
error-bad-params = Parametri non validi per “{ $command }”: { $message }
error-no-document = Nessuna cartella di lavoro aperta.
error-internal = Errore interno in “{ $command }” (la cartella di lavoro è rimasta invariata): { $message }
error-other = { $message }
error-invalid-formula = Si è verificato un problema con questa formula: { $message }
error-invalid-locale = Le impostazioni di lingua o di area geografica non sono valide: { $message }
error-protected-sheet = Questa modifica non è consentita: la cella o il grafico si trova in un foglio protetto.
error-encrypted-workbook = “{ $name }” è protetto da password. Rimuovi la password in Excel e riprova; GridCraft non può ancora aprire cartelle di lavoro crittografate.
error-legacy-workbook = “{ $name }” è una cartella di lavoro di Excel 97–2003 (.xls) o un altro file binario precedente. Salvalo come .xlsx in Excel e riprova.
error-import-failed = Impossibile importare “{ $name }”: { $message }
error-import-only-format = { $format } consente solo l’importazione dei dati. Salva come .xlsx o in un altro formato di esportazione supportato.
error-text-box-too-long = Le caselle di testo possono contenere al massimo { $limit } caratteri.
error-chart-range-too-large = L’intervallo di origine è troppo grande per un grafico.
