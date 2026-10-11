# Messages d’erreur du moteur. Clé : error-<code> ; les codes reflètent gridcraft_engine::EngineError.
# Texte original de GridCraft en français (France).

error-unknown-command = Commande inconnue « { $command } ».
error-command-disabled = « { $command } » n’est pas disponible pour le moment : { $reason }
error-bad-params = Paramètres non valides pour « { $command } » : { $message }
error-no-document = Aucun classeur n’est ouvert.
error-internal = Erreur interne dans « { $command } » (le classeur est resté tel qu’il était) : { $message }
error-other = { $message }
error-invalid-formula = Cette formule comporte un problème : { $message }
error-invalid-locale = Les paramètres de langue ou de région ne sont pas valides : { $message }
error-protected-sheet = Cette modification n’est pas autorisée : la cellule ou le graphique se trouve dans une feuille protégée.
error-encrypted-workbook = « { $name } » est protégé par un mot de passe. Supprimez le mot de passe dans Excel et réessayez ; GridCraft ne peut pas encore ouvrir les classeurs chiffrés.
error-legacy-workbook = « { $name } » est un classeur Excel 97–2003 (.xls) ou un autre ancien fichier binaire. Enregistrez-le au format .xlsx dans Excel et réessayez.
error-import-failed = Impossible d’importer « { $name } » : { $message }
error-import-only-format = { $format } permet uniquement l’importation des données. Enregistrez au format .xlsx ou dans un autre format d’exportation pris en charge.
error-text-box-too-long = Les zones de texte peuvent contenir au maximum { $limit } caractères.
error-chart-range-too-large = La plage source est trop grande pour un graphique.
