//! French translations for the interface strings in i18n.rs.

/// Returns the French translation for `text`, or `None` so callers fall back to English.
pub(super) fn translate(text: &str) -> Option<&'static str> {
    Some(match text {
        "Different First Page" => "Première page différente",
        "Different Odd & Even Pages" => "Pages paires et impaires différentes",
        "Scale with Document" => "Mettre à l'échelle avec le document",
        "Align with Page Margins" => "Aligner sur les marges de la page",
        "Paste" => "Coller",
        "Paste Values" => "Coller les valeurs",
        "Paste Formulas" => "Coller les formules",
        "Paste Formatting" => "Coller la mise en forme",
        "Transpose" => "Transposer",
        "Paste Link" => "Coller la liaison",
        "Paste Special…" => "Collage spécial…",
        "Insert Copied Cells" => "Insérer les cellules copiées",
        "Insert Copied Cells…" => "Insérer les cellules copiées…",
        "Insert Cut Cells" => "Insérer les cellules coupées",
        "Insert Paste" => "Insérer en collant",
        "Cut" => "Couper",
        "Copy" => "Copier",
        "Format Painter (double-click to keep it on)" => "Reproduire la mise en forme (double-clic pour la conserver)",
        "Bold" => "Gras",
        "Italic" => "Italique",
        "Double Underline" => "Double soulignement",
        "Strikethrough" => "Barré",
        "Underline" => "Souligné",
        "Increase Font Size" => "Augmenter la taille de police",
        "Decrease Font Size" => "Réduire la taille de police",
        "Wrap Text" => "Renvoyer à la ligne automatiquement",
        "Merge & Center" => "Fusionner et centrer",
        "Merge Cells" => "Fusionner les cellules",
        "Merge Across" => "Fusionner horizontalement",
        "Unmerge Cells" => "Annuler la fusion des cellules",
        "Fill Color" => "Couleur de remplissage",
        "Font Color" => "Couleur de police",
        "Borders" => "Bordures",
        "Top\nAlign" => "Aligner en\nhaut",
        "Middle\nAlign" => "Aligner au\nmilieu",
        "Bottom\nAlign" => "Aligner en\nbas",
        "Left" => "Gauche",
        "Right" => "Droite",
        "Center" => "Centré",
        "Horizontal" => "Horizontal",
        "Vertical Text" => "Texte vertical",
        "Decrease Indent" => "Diminuer le retrait",
        "Increase Indent" => "Augmenter le retrait",
        "Orientation" => "Orientation",
        "Page Layout|Orientation" => "Orientation",
        "Angle Counterclockwise" => "Inclinaison vers la gauche",
        "Angle Clockwise" => "Inclinaison vers la droite",
        "Rotate Text Up" => "Pivoter le texte vers le haut",
        "Rotate Text Down" => "Pivoter le texte vers le bas",
        "Accounting Number Format" => "Format de nombre Comptabilité",
        "Percent Style" => "Style de pourcentage",
        "Comma Style" => "Style de milliers",
        "Increase Decimal" => "Ajouter une décimale",
        "Decrease Decimal" => "Retirer une décimale",
        "More Accounting Formats…" => "Autres formats comptables…",
        "Conditional\nFormatting" => "Mise en forme\nconditionnelle",
        "Conditional Formatting" => "Mise en forme conditionnelle",
        "Format\nas Table" => "Mettre sous forme\nde tableau",
        "Cell\nStyles" => "Styles de\ncellules",
        "Format Cells…" => "Format de cellules…",
        "Format Cell Alignment…" => "Format d'alignement de cellule…",
        "Format\nPane" => "Volet\nFormat",
        "Format" => "Format",
        "Insert" => "Insertion",
        "Header Menu|Insert" => "Insertion",
        "Insert Cells…" => "Insérer des cellules…",
        "Delete" => "Supprimer",
        "Delete Cells…" => "Supprimer des cellules…",
        "Insert Sheet Rows" => "Insérer des lignes de feuille",
        "Insert Sheet Columns" => "Insérer des colonnes de feuille",
        "Delete Sheet Rows" => "Supprimer des lignes de feuille",
        "Delete Sheet Columns" => "Supprimer des colonnes de feuille",
        "Row Height…" => "Hauteur de ligne…",
        "AutoFit Row Height" => "Ajustement automatique de la hauteur de ligne",
        "Column Width…" => "Largeur de colonne…",
        "AutoFit Column Width" => "Ajustement automatique de la largeur de colonne",
        "Default Width…" => "Largeur par défaut…",
        "Hide Rows" => "Masquer les lignes",
        "Unhide Rows" => "Afficher les lignes",
        "Hide Columns" => "Masquer les colonnes",
        "Unhide Columns" => "Afficher les colonnes",
        "Lock Cell" => "Verrouiller la cellule",
        "Clear" => "Effacer",
        "Clear All" => "Effacer tout",
        "Clear Formats" => "Effacer les formats",
        "Clear Contents" => "Effacer le contenu",
        "Clear Comments and Notes" => "Effacer les commentaires et les notes",
        "Clear Hyperlinks" => "Effacer les liens hypertexte",
        "AutoSum" => "Somme automatique",
        "Flash\nFill" => "Remplissage\ninstantané",
        "Flash Fill" => "Remplissage instantané",
        "Fill" => "Remplir",
        "Clear Print Area" => "Effacer la zone d'impression",
        "Find &\nSelect" => "Rechercher et\nsélectionner",
        "Find…" => "Rechercher…",
        "Replace…" => "Remplacer…",
        "Go To…" => "Atteindre…",
        "Go To Special…" => "Atteindre spécial…",
        "PivotTable" => "Tableau croisé dynamique",
        "Recommended\nCharts" => "Graphiques\nrecommandés",
        "Table" => "Tableau",
        "Pictures" => "Images",
        "Shapes" => "Formes",
        "Icons" => "Icônes",
        "Text\nBox" => "Zone de\ntexte",
        "Comment" => "Commentaire",
        "New\nComment" => "Nouveau\ncommentaire",
        "New Note" => "Nouvelle note",
        "Show/Hide Note" => "Afficher/masquer la note",
        "Header &\nFooter" => "En-tête et\npied de page",
        "Text to\nColumns" => "Convertir le texte\nen colonnes",
        "Link" => "Lien",
        "Symbol" => "Symbole",
        "Equation" => "Équation",
        "Insert New Equation..." => "Insérer une nouvelle équation",
        "Sparklines" => "Courbes de tendance",
        "Checkbox" => "Case à cocher",
        "Insert checkboxes in the selected cells" => "Insérer des cases à cocher dans les cellules sélectionnées",
        "Button" => "Bouton",
        "Slider" => "Curseur",
        "Spin Button" => "Toupie",
        "Draw a button that runs a macro or a command" => "Dessiner un bouton qui exécute une macro ou une commande",
        "Draw a slider that sets a whole number in a linked cell" => "Dessiner un curseur qui définit un nombre entier dans une cellule liée",
        "Draw a spin button that steps a whole number in a linked cell" => {
            "Dessiner une toupie qui modifie par pas un nombre entier dans une cellule liée"
        }
        "Edit Text" => "Modifier le texte",
        "Assign Action…" => "Affecter une action…",
        "Format Control…" => "Format de contrôle…",
        "Header & Footer" => "En-tête et pied de page",
        "Draw with ink" => "Dessiner à l'encre",
        "Eraser" => "Gomme",
        "Ink to\nShape" => "Encre en\nforme",
        "Select\nObjects" => "Sélectionner les\nobjets",
        "Themes" => "Thèmes",
        "Margins" => "Marges",
        "Left Margin" => "Marge gauche",
        "Right Margin" => "Marge droite",
        "Top Margin" => "Marge haute",
        "Bottom Margin" => "Marge basse",
        "Header Margin" => "Marge de l'en-tête",
        "Footer Margin" => "Marge du pied de page",
        "Size" => "Taille",
        "Print\nArea" => "Zone\nd'impression",
        "Set Print Area" => "Définir la zone d'impression",
        "Breaks" => "Sauts",
        "Insert Page Break" => "Insérer un saut de page",
        "Remove Page Break" => "Supprimer le saut de page",
        "Reset All Page Breaks" => "Réinitialiser tous les sauts de page",
        "Print\nTitles" => "Titres à\nimprimer",
        "Sheet\nRight-to-Left" => "Feuille de\ndroite à gauche",
        "Sheet Right-to-Left" => "Feuille de droite à gauche",
        "Narrow" => "Étroit",
        "Wide" => "Large",
        "Normal" => "Normal",
        "Portrait" => "Portrait",
        "Landscape" => "Paysage",
        "Custom Margins…" => "Marges personnalisées…",
        "Insert\nFunction" => "Insérer une\nfonction",
        "Name\nManager" => "Gestionnaire\nde noms",
        "Define Name" => "Définir un nom",
        "Use in Formula" => "Utiliser dans la formule",
        "Create from Selection" => "Créer à partir de la sélection",
        "Trace Precedents" => "Repérer les antécédents",
        "Trace Dependents" => "Repérer les dépendants",
        "Remove Arrows" => "Supprimer les flèches",
        "Show Formulas" => "Afficher les formules",
        "Error Checking" => "Vérification des erreurs",
        "Trace Error" => "Repérer une erreur",
        "Circular References" => "Références circulaires",
        "Python Mode" => "Mode Python",
        "Python Object" => "Objet Python",
        "Excel Value" => "Valeur Excel",
        "Copy as CSV" => "Copier en CSV",
        "Copy as Markdown" => "Copier en Markdown",
        "Save as .csv…" => "Enregistrer en .csv…",
        "Spill Here" => "Déverser ici",
        "Spill One Line per Row" => "Déverser une ligne de texte par ligne",
        "The table isn't kept in the file. Run the cell to see it." => {
            "Le tableau n'est pas conservé dans le fichier. Exécutez la cellule pour le voir."
        }
        "This table is empty." => "Ce tableau est vide.",
        "This table has no rows." => "Ce tableau n'a aucune ligne.",
        "Copied only what is shown: the rest of the table isn't in the copy." => {
            "Seul ce qui est affiché a été copié : le reste du tableau n'est pas dans la copie."
        }
        "Saved only what is shown: the rest of the table isn't in the file." => {
            "Seul ce qui est affiché a été enregistré : le reste du tableau n'est pas dans le fichier."
        }
        "The table is saved." => "Le tableau est enregistré.",
        "Only the first {rows} of {total} rows are shown." => "Seules les {rows} premières lignes sur {total} sont affichées.",
        "Only the first {cols} of {total} columns are shown." => "Seules les {cols} premières colonnes sur {total} sont affichées.",
        "Some long text is cut." => "Une partie du texte long est coupée.",
        "Copying and saving take only what is shown. Spill Here gives the whole table." => {
            "La copie et l'enregistrement ne prennent que ce qui est affiché. Déverser ici donne le tableau entier."
        }
        "Save Python Table as CSV" => "Enregistrer le tableau Python en CSV",
        "Python Table" => "Tableau Python",
        "Convert to Normal Cell" => "Convertir en cellule normale",
        "Show Card" => "Afficher la carte",
        "Insert\nPython" => "Insérer\nPython",
        "Insert Python (Ctrl+Alt+Shift+P)" => "Insérer Python (Ctrl+Alt+Shift+P)",
        "Python Editor" => "Éditeur Python",
        "Edit in Python Editor" => "Modifier dans l'éditeur Python",
        "Unsaved changes" => "Modifications non enregistrées",
        "The code for {cell} has unsaved changes." => "Le code de {cell} contient des modifications non enregistrées.",
        "The code for {cell} in {workbook} has unsaved changes." => "Le code de {cell} dans {workbook} contient des modifications non enregistrées.",
        "The code can't be longer: a Python cell holds {max} characters, counted as a PY formula." => {
            "Le code ne peut pas être plus long : une cellule Python contient {max} caractères, comptés comme formule PY."
        }
        "{cell} changed while it was open in the Python editor. Copy the code, or close the editor and open it again on the cell." => {
            "{cell} a changé pendant son ouverture dans l'éditeur Python. Copiez le code, ou fermez l'éditeur et rouvrez-le sur la cellule."
        }
        "{cell} changed while it was open in the Python editor, so the code was not applied. Copy the code, or close the editor and open it again on the cell." => {
            "{cell} a changé pendant son ouverture dans l'éditeur Python, le code n'a donc pas été appliqué. Copiez le code, ou fermez l'éditeur et rouvrez-le sur la cellule."
        }
        "{apply} applies the code. Esc closes the editor." => "{apply} applique le code. Échap ferme l'éditeur.",
        "The Python editor's unsaved code was copied to the clipboard." => {
            "Le code non enregistré de l'éditeur Python a été copié dans le Presse-papiers."
        }
        "Apply" => "Appliquer",
        "Discard" => "Ignorer",
        "Cancel" => "Annuler",
        "Evaluate Formula" => "Évaluation de formule",
        "Watch Window" => "Fenêtre Espion",
        "Calculation\nOptions" => "Options de\ncalcul",
        "Calculate Now" => "Calculer maintenant",
        "Calculate Sheet" => "Calculer la feuille",
        "Automatic" => "Automatique",
        "Automatic Except for Data Tables" => "Automatique sauf pour les tables de données",
        "Manual" => "Manuel",
        "Sort &\nFilter" => "Trier et\nfiltrer",
        "Sort" => "Trier",
        "Sort A to Z" => "Trier de A à Z",
        "Sort Z to A" => "Trier de Z à A",
        "Custom Sort…" => "Tri personnalisé…",
        "Filter" => "Filtrer",
        "Reapply" => "Réappliquer",
        "Get Data\n(Text/CSV)" => "Obtenir des données\n(Texte/CSV)",
        "Remove\nDuplicates" => "Supprimer les\ndoublons",
        "Data\nValidation" => "Validation des\ndonnées",
        "Data Validation" => "Validation des données",
        "Data Validation…" => "Validation des données…",
        "Circle Invalid Data" => "Cerner les données non valides",
        "Clear Validation" => "Effacer la validation",
        "Group" => "Grouper",
        "Ungroup" => "Dissocier",
        "Show Detail" => "Afficher le détail",
        "Hide Detail" => "Masquer le détail",
        "Subtotal" => "Sous-total",
        "Data\nAnalysis" => "Analyse de\ndonnées",
        "Data Analysis" => "Analyse de données",
        "Solver" => "Solveur",
        "What-If\nAnalysis" => "Analyse\nde scénarios",
        "Data Table…" => "Table de données…",
        "Scenario Manager…" => "Gestionnaire de scénarios…",
        "Goal Seek…" => "Valeur cible…",
        "Forecast\nSheet" => "Feuille de\nprévision",
        "Spelling" => "Orthographe",
        "Check\nAccessibility" => "Vérifier\nl'accessibilité",
        "Threaded Comments" => "Commentaires en fil de discussion",
        "Show\nComments" => "Afficher les\ncommentaires",
        "Previous" => "Précédent",
        "Next" => "Suivant",
        "Previous Comment" => "Commentaire précédent",
        "Next Comment" => "Commentaire suivant",
        "Show All Notes" => "Afficher toutes les notes",
        "Convert to Comments" => "Convertir en commentaires",
        "Protect\nWorkbook" => "Protéger le\nclasseur",
        "Allow Edit\nRanges" => "Autoriser la modification\ndes plages",
        "Allow Edit Ranges" => "Autoriser la modification des plages",
        "Unprotect\nSheet" => "Ôter la protection\nde la feuille",
        "Protect Sheet…" => "Protéger la feuille…",
        "Unprotect Sheet…" => "Ôter la protection de la feuille…",
        "Comments" => "Commentaires",
        "Notes" => "Notes",
        "Page Break\nPreview" => "Aperçu des sauts\nde page",
        "Page\nLayout" => "Mise en\npage",
        "Formula Bar" => "Barre de formule",
        "Name Box" => "Zone de nom",
        "Cell Editor" => "Éditeur de cellule",
        "Font Size" => "Taille de police",
        "Gridlines" => "Quadrillage",
        "Headings" => "En-têtes",
        "Zoom" => "Zoom",
        "Magnification" => "Zoom",
        "Fit selection" => "Ajuster la sélection",
        "100%" => "100 %",
        "Zoom to\nSelection" => "Zoom sur la\nsélection",
        "Freeze\nPanes" => "Figer les\nvolets",
        "Freeze Panes" => "Figer les volets",
        "Freeze Top Row" => "Figer la ligne supérieure",
        "Freeze First Column" => "Figer la première colonne",
        "Unfreeze Panes" => "Libérer les volets",
        "Dark Mode" => "Mode sombre",
        "Display theme" => "Thème d'affichage",
        "Animations" => "Animations",
        "Glide the selection outline to the cell you move to instead of jumping there" => {
            "Faire glisser le contour de la sélection jusqu'à la cellule de destination au lieu d'y sauter directement"
        }
        "System" => "Système",
        "Light" => "Clair",
        "Dark" => "Sombre",
        "Interface language" => "Langue de l'interface",
        "Interface size" => "Taille de l'interface",
        "Auto" => "Automatique",
        "Drag to freeze columns" => "Faites glisser pour figer les colonnes",
        "Drag to freeze rows" => "Faites glisser pour figer les lignes",
        "Command\nPalette" => "Palette de\ncommandes",
        "Agent\nControl" => "Contrôle de\nl'agent",
        "Action\nJournal" => "Journal des\nactions",
        "About\nGridCraft" => "À propos de\nGridCraft",
        "Share" => "Partager",
        "Add Chart\nElement" => "Ajouter un élément\nde graphique",
        "Change\nChart Type" => "Modifier le type\nde graphique",
        "Switch\nRow/Column" => "Permuter\nligne/colonne",
        "Select\nData" => "Sélectionner les\ndonnées",
        "Data Labels: None" => "Étiquettes de données : Aucune",
        "Data Labels: Show" => "Étiquettes de données : Afficher",
        "Legend: None" => "Légende : Aucune",
        "Legend: Bottom" => "Légende : Bas",
        "Legend: Top" => "Légende : Haut",
        "Legend: Right" => "Légende : Droite",
        "First slice angle" => "Angle du premier secteur",
        "Doughnut hole size" => "Taille du trou de l'anneau",
        "Gridlines: None" => "Quadrillage : Aucun",
        "Gridlines: Show" => "Quadrillage : Afficher",
        "Chart\nTitle" => "Titre du\ngraphique",
        "Delete\nChart" => "Supprimer le\ngraphique",
        "Convert\nto Range" => "Convertir\nen plage",
        "Table\nStyles" => "Styles de\ntableau",
        "Top 10 Items" => "10 premiers éléments",
        "Top 10%" => "10 % les plus élevés",
        "Bottom 10 Items" => "10 derniers éléments",
        "Bottom 10%" => "10 % les moins élevés",
        "Above Average" => "Au-dessus de la moyenne",
        "Below Average" => "En dessous de la moyenne",
        "Clear Rules from Entire Sheet" => "Effacer les règles de la feuille entière",
        "Clear Rules from Selected Cells" => "Effacer les règles des cellules sélectionnées",
        "Constants" => "Constantes",
        "Data Bars" => "Barres de données",
        "Color Scales" => "Échelles de couleurs",
        "Icon Sets" => "Jeux d'icônes",
        "Home" => "Accueil",
        "Draw" => "Dessin",
        "Page Layout" => "Mise en page",
        "Formulas" => "Formules",
        "Data" => "Données",
        "Review" => "Révision",
        "View" => "Affichage",
        "Automate" => "Automatiser",
        "Table Design" => "Création de tableau",
        "Chart Design" => "Création de graphique",
        "Ready" => "Prêt",
        "Enter" => "Entrée",
        "Edit" => "Modifier",
        "Point" => "Point",
        "Calculate" => "Calculer",
        "Select destination and press Enter or choose Paste" => "Sélectionnez la destination et appuyez sur Entrée ou choisissez Coller",
        "General" => "Standard",
        "Number" => "Nombre",
        "Currency" => "Monétaire",
        "Accounting" => "Comptabilité",
        "Date" => "Date",
        "Time" => "Heure",
        "Percentage" => "Pourcentage",
        "Fraction" => "Fraction",
        "Scientific" => "Scientifique",
        "Text" => "Texte",
        "Special" => "Spécial",
        "Custom" => "Personnalisée",
        "Short Date" => "Date courte",
        "Long Date" => "Date longue",
        "More Number Formats…" => "Autres formats numériques…",
        "Financial" => "Financières",
        "Recently Used" => "Récemment utilisées",
        "Logical" => "Logiques",
        "Date & Time" => "Date et heure",
        "Lookup & Reference" => "Recherche et référence",
        "Math & Trig" => "Maths et trigonométrie",
        "More Functions" => "Autres fonctions",
        "Sheet Options|View" => "Affichage",
        "Print" => "Impression",
        "Print What" => "Quoi imprimer",
        "Click to add data" => "Cliquez pour ajouter des données",
        "Add header" => "Ajouter un en-tête",
        "Add footer" => "Ajouter un pied de page",
        "(none)" => "(aucun)",
        "Header" => "En-tête",
        "Footer" => "Pied de page",
        "Page\nNumber" => "Numéro de\npage",
        "Number\nof Pages" => "Nombre\nde pages",
        "Current\nDate" => "Date\nactuelle",
        "Current\nTime" => "Heure\nactuelle",
        "File\nPath" => "Chemin\ndu fichier",
        "File\nName" => "Nom du\nfichier",
        "Sheet\nName" => "Nom de la\nfeuille",
        "Go to\nHeader" => "Aller à\nl'en-tête",
        "Go to\nFooter" => "Aller au\npied de page",
        "Page Number" => "Numéro de page",
        "Number of Pages" => "Nombre de pages",
        "Current Date" => "Date actuelle",
        "Current Time" => "Heure actuelle",
        "File Path" => "Chemin du fichier",
        "File Name" => "Nom du fichier",
        "Sheet Name" => "Nom de la feuille",
        "Go to Header" => "Aller à l'en-tête",
        "Go to Footer" => "Aller au pied de page",
        "Ruler" => "Règle",
        "File" => "Fichier",
        "Back" => "Retour",
        "New" => "Nouveau",
        "Open" => "Ouvrir",
        "Info" => "Informations",
        "Save" => "Enregistrer",
        "Save As" => "Enregistrer sous",
        "Export" => "Exporter",
        "Close" => "Fermer",
        "Options" => "Options",
        "Account" => "Compte",
        "Product Information" => "Informations sur le produit",
        "About GridCraft" => "À propos de GridCraft",
        "Version" => "Version",
        "Platform" => "Plateforme",
        "Licence" => "Licence",
        "Version, contributors and the AI models that helped" => "Version, contributeurs et les modèles d'IA qui ont aidé",
        "Blank workbook" => "Classeur vide",
        "Recent" => "Récent",
        "No recent workbooks yet." => "Aucun classeur récent pour le moment.",
        "More workbooks" => "Plus de classeurs",
        "Modified" => "Modifié",
        "Not available" => "Indisponible",
        "Just now" => "À l'instant",
        "Yesterday" => "Hier",
        "{n} min ago" => "il y a {n} min",
        "{n} h ago" => "il y a {n} h",
        "{n} days ago" => "il y a {n} jours",
        "{n} weeks ago" => "il y a {n} semaines",
        "{n} months ago" => "il y a {n} mois",
        "{n} years ago" => "il y a {n} ans",
        "Browse…" => "Parcourir…",
        "Not saved yet" => "Pas encore enregistré",
        "Location" => "Emplacement",
        "Sheets" => "Feuilles",
        "Protect Workbook" => "Protéger le classeur",
        "Sheet structure is locked: sheets can't be added, moved, renamed or deleted." => {
            "La structure de la feuille est verrouillée : les feuilles ne peuvent pas être ajoutées, déplacées, renommées ou supprimées."
        }
        "Lock the sheet structure so sheets can't be added, moved, renamed or deleted." => {
            "Verrouiller la structure des feuilles pour qu'elles ne puissent pas être ajoutées, déplacées, renommées ou supprimées."
        }
        "Properties" => "Propriétés",
        "Title" => "Titre",
        "Subject" => "Sujet",
        "Author" => "Auteur",
        "Company" => "Société",
        "Keywords" => "Mots-clés",
        "Description" => "Description",
        "No workbook is open." => "Aucun classeur n'est ouvert.",
        "Save as type" => "Type de fichier",
        "Excel Workbook (*.xlsx)" => "Classeur Excel (*.xlsx)",
        "CSV UTF-8 (Comma delimited) (*.csv)" => "CSV UTF-8 (séparateur point-virgule) (*.csv)",
        "Web Page (*.html)" => "Page web (*.html)",
        "PDF (*.pdf)" => "PDF (*.pdf)",
        "Download" => "Télécharger",
        "A CSV file, web page or PDF is a copy: the workbook keeps its own file." => {
            "Un fichier CSV, une page web ou un PDF est une copie : le classeur conserve son propre fichier."
        }
        "Save the workbook under another name or in another folder." => "Enregistrer le classeur sous un autre nom ou dans un autre dossier.",
        "Print Active Sheets" => "Imprimer les feuilles actives",
        "Print Entire Workbook" => "Imprimer tout le classeur",
        "Fit Sheet on One Page" => "Ajuster la feuille sur une page",
        "Page Setup…" => "Mise en page…",
        "Page {page} of {pages}" => "Page {page} sur {pages}",
        "Nothing to print." => "Rien à imprimer.",
        "Previous page" => "Page précédente",
        "Next page" => "Page suivante",
        "Create PDF" => "Créer un PDF",
        "Save the sheet as a PDF document that keeps its page layout." => {
            "Enregistrer la feuille en tant que document PDF qui conserve sa mise en page."
        }
        "Change File Type" => "Changer le type de fichier",
        "CSV (Comma delimited)" => "CSV (séparateur point-virgule)",
        "Save the active sheet as comma-separated text." => "Enregistrer la feuille active sous forme de texte séparé par des points-virgules.",
        "Web Page (.html)" => "Page web (.html)",
        "Save the workbook as a web page." => "Enregistrer le classeur en tant que page web.",
        "Options|General" => "Général",
        "Options|Save" => "Enregistrer",
        "Show formula bar" => "Afficher la barre de formule",
        "Enable AutoComplete for cell values" => "Activer la saisie semi-automatique des valeurs de cellule",
        "Save .xlsx workbooks automatically after every change" => "Enregistrer automatiquement les classeurs .xlsx après chaque modification",
        "Workbook calculation" => "Calcul du classeur",
        "Enable iterative calculation" => "Activer le calcul itératif",
        "Maximum Iterations:" => "Nombre maximal d'itérations :",
        "Maximum Change:" => "Écart maximal :",
        "Maximum Iterations must be a whole number from 1 to 32767." => {
            "Le nombre maximal d'itérations doit être un entier compris entre 1 et 32767."
        }
        "Maximum Change must be a number of 0 or more." => "L'écart maximal doit être un nombre supérieur ou égal à 0.",
        "Calculation settings are saved with the workbook." => "Les paramètres de calcul sont enregistrés avec le classeur.",
        "Insert Sheet" => "Insérer une feuille",
        "Rename" => "Renommer",
        "Move or Copy…" => "Déplacer ou copier…",
        "Duplicate" => "Dupliquer",
        "Tab Color" => "Couleur de l'onglet",
        "No Color" => "Sans couleur",
        "Hide" => "Masquer",
        "Unhide…" => "Afficher…",
        "Select All Sheets" => "Sélectionner toutes les feuilles",
        "AutoSave" => "Enregistrement automatique",
        "AutoSave: save after every change (workbooks saved to a file)" => {
            "Enregistrement automatique : enregistrer après chaque modification (classeurs enregistrés dans un fichier)"
        }
        "Previous sheet" => "Feuille précédente",
        "Next sheet" => "Feuille suivante",
        "All sheets" => "Toutes les feuilles",
        "New sheet (⇧F11)" => "Nouvelle feuille (⇧F11)",
        "Text Box" => "Zone de texte",
        "Macros" => "Macros",
        "Record Macro" => "Enregistrer une macro",
        "Stop Recording" => "Arrêter l’enregistrement",
        "Use Relative References" => "Utiliser les références relatives",
        "Run\nMacro" => "Exécuter la macro",
        "Macros…" => "Macros…",
        "Recording" => "Enregistrement",
        "Stop" => "Arrêter",
        "New Macro" => "Nouvelle macro",
        "Show" => "Afficher",
        "Collapse dialog" => "Réduire la boîte de dialogue",
        "Expand dialog" => "Développer la boîte de dialogue",
        "Python|Off" => "Désactivé",
        "Python|Not Set Up" => "Non configuré",
        "Python|Ready" => "Prêt",
        "Python|Running" => "En cours",
        "Python|Blocked" => "Bloqué",
        "Enable Python in cells" => "Activer Python dans les cellules",
        "Waiting for Python cells to finish before saving..." => "Attente de la fin des cellules Python avant l'enregistrement...",
        "Save Now" => "Enregistrer maintenant",
        "The workbook was saved, but these Python cells weren't finished, so the file has their last results, if they had any: {cells}" => {
            "Le classeur a été enregistré, mais ces cellules Python n'étaient pas terminées ; le fichier contient donc leurs derniers résultats, s'il y en avait : {cells}"
        }
        "{cells} and {count} more" => "{cells} et {count} de plus",
        "{version}, too old: Python cells need 3.11 or newer" => {
            "{version}, trop ancienne : les cellules Python nécessitent la version 3.11 ou ultérieure"
        }
        "Looking for Python…" => "Recherche de Python…",
        "Copied to the clipboard." => "Copié dans le presse-papiers.",
        "This workbook has Python code. Enable it to run its cells; until then they keep the values saved in the file." => {
            "Ce classeur contient du code Python. Activez-le pour exécuter ses cellules ; en attendant, elles conservent les valeurs enregistrées dans le fichier."
        }
        "Enable" => "Activer",
        "Python: running {cell}, {seconds} s" => "Python : exécution de {cell}, {seconds} s",
        "Python: {state}" => "Python : {state}",
        "Open the Python environment" => "Ouvrir l'environnement Python",
        "Nothing on this system: Python doesn't run here" => "Rien sur ce système : Python ne s'exécute pas ici",
        "GridCraft starts Python with an empty environment" => "GridCraft démarre Python avec un environnement vide",
        "macOS sandbox profile" => "Profil de bac à sable macOS",
        "Linux Landlock" => "Linux Landlock",
        "Linux seccomp filter" => "Filtre seccomp Linux",
        "Open Python Settings" => "Ouvrir les paramètres Python",
        "Choose Interpreter" => "Choisir l'interpréteur",
        "Copy Install Command" => "Copier la commande d'installation",
        "Run Again" => "Réexécuter",
        "Reset" => "Réinitialiser",
        "Python Environment" => "Environnement Python",
        "Python Packages" => "Paquets Python",
        "Python Problems" => "Problèmes Python",
        "Python Run Log" => "Journal d'exécution Python",
        "What Python Can Do" => "Ce que Python peut faire",
        "GridCraft is checking Python." => "GridCraft vérifie Python.",
        "Check Again" => "Vérifier à nouveau",
        "Interpreter" => "Interpréteur",
        "Chosen by you:" => "Choisi par vous :",
        "Automatic found:" => "Trouvé automatiquement :",
        "What runs:" => "Ce qui s'exécute :",
        "None found yet." => "Aucun trouvé pour le moment.",
        "Sandbox" => "Bac à sable",
        "Package folders" => "Dossiers de paquets",
        "What Python can read" => "Ce que Python peut lire",
        "uv manages Python versions and packages. To get it, run:" => "uv gère les versions et les paquets Python. Pour l'obtenir, exécutez :",
        "Reading files outside Python: blocked" => "Lecture de fichiers hors de Python : bloquée",
        "Writing files: blocked" => "Écriture de fichiers : bloquée",
        "Using the network: blocked" => "Utilisation du réseau : bloquée",
        "Starting other programs: blocked" => "Démarrage d'autres programmes : bloqué",
        "Each time Python starts, GridCraft tries each of these first, and doesn't run Python unless every one is refused." => {
            "À chaque démarrage de Python, GridCraft essaie chacune de ces mesures et n'exécute Python que si toutes sont refusées."
        }
        "Install a package with uv. GridCraft only copies the command; you run it in a terminal, then choose Check Again." => {
            "Installez un paquet avec uv. GridCraft ne fait que copier la commande ; vous l'exécutez dans un terminal, puis vous choisissez Vérifier à nouveau."
        }
        "Package name" => "Nom du paquet",
        "Other package managers" => "Autres gestionnaires de paquets",
        "For people who manage Python another way: choose any interpreter by path in File › Options › Python. With pip:" => {
            "Pour ceux qui gèrent Python autrement : choisissez n'importe quel interpréteur par son chemin dans Fichier › Options › Python. Avec pip :"
        }
        "Packages: {count}" => "Paquets : {count}",
        "No problems." => "Aucun problème.",
        "This workbook" => "Ce classeur",
        "No runs yet." => "Aucune exécution pour le moment.",
        "Go To Cell" => "Atteindre la cellule",
        "This run printed nothing." => "Cette exécution n'a rien imprimé.",
        "Python cells can" => "Les cellules Python peuvent",
        "Read cells through xl()" => "Lire des cellules avec xl()",
        "GridCraft sends only the cells that xl() names" => "GridCraft n'envoie que les cellules que xl() désigne",
        "Import installed packages" => "Importer les paquets installés",
        "The sandbox lets Python read its own installation" => "Le bac à sable permet à Python de lire sa propre installation",
        "Compute" => "Calculer",
        "Within the time and memory limits in File › Options › Python" => "Dans les limites de temps et de mémoire de Fichier › Options › Python",
        "Printed output is kept in the Run Log" => "La sortie imprimée est conservée dans le journal d'exécution",
        "Use threads" => "Utiliser des threads",
        "Threads stay inside the one Python process" => "Les threads restent dans l'unique processus Python",
        "Python cells cannot" => "Les cellules Python ne peuvent pas",
        "Read your files" => "Lire vos fichiers",
        "Write files" => "Écrire des fichiers",
        "Use the network" => "Utiliser le réseau",
        "Start programs" => "Démarrer des programmes",
        "See environment variables" => "Voir les variables d'environnement",
        "Open windows" => "Ouvrir des fenêtres",
        "Draw plots. The data goes to a sheet and a chart is built from it." => {
            "Tracer des graphiques. Les données vont dans une feuille et un graphique est créé à partir de celles-ci."
        }
        "Run this workbook's Python cells" => "Exécuter les cellules Python de ce classeur",
        "Off by default. Only you can turn this on, here: no file, macro or agent can. A workbook opened from a file also needs its own Enable." => {
            "Désactivé par défaut. Vous seul pouvez l'activer, ici : aucun fichier, macro ou agent ne le peut. Un classeur ouvert depuis un fichier a aussi besoin de sa propre activation."
        }
        "Limits" => "Limites",
        "While Python is off, GridCraft starts nothing, so it can't yet say which version it would use." => {
            "Tant que Python est désactivé, GridCraft ne démarre rien et ne peut donc pas encore dire quelle version il utiliserait."
        }
        "Automatic would try first:" => "Automatique essaierait d'abord :",
        "Set up Python with uv" => "Configurer Python avec uv",
        "Run these in a terminal, in order. GridCraft looks in this environment first, so there is nothing more to choose. It only copies the commands; it never runs them." => {
            "Exécutez ces commandes dans un terminal, dans l'ordre. GridCraft cherche d'abord dans cet environnement, il n'y a donc rien d'autre à choisir. Il ne fait que copier les commandes ; il ne les exécute jamais."
        }
        "Any Python 3.11 or newer can be chosen by path with Browse…. To add packages to it with pip:" => {
            "Tout Python 3.11 ou ultérieur peut être choisi par son chemin avec Parcourir…. Pour y ajouter des paquets avec pip :"
        }
        "Time limit for one cell" => "Limite de temps pour une cellule",
        " seconds" => " secondes",
        "Memory limit" => "Limite de mémoire",
        " MB" => " MB",
        "At a limit Python is restarted and its variables are cleared; the next run starts fresh." => {
            "À une limite, Python est redémarré et ses variables sont effacées ; l'exécution suivante repart de zéro."
        }
        "On macOS the memory limit is a check twice a second, not a hard cap." => {
            "Sous macOS, la limite de mémoire est vérifiée deux fois par seconde ; ce n'est pas un plafond strict."
        }
        "Editor" => "Éditeur",
        "Initialization" => "Initialisation",
        "The code that runs before this workbook's Python cells" => "Le code exécuté avant les cellules Python de ce classeur",
        "Open Initialization" => "Ouvrir l'initialisation",
        "The initialization code has unsaved changes." => "Le code d'initialisation contient des modifications non enregistrées.",
        "The initialization code of {workbook} has unsaved changes." => {
            "Le code d'initialisation de {workbook} contient des modifications non enregistrées."
        }
        "The code can't be longer: initialization code holds {max} characters." => {
            "Le code ne peut pas être plus long : le code d'initialisation contient au plus {max} caractères."
        }
        "The initialization code changed while it was open in the editor. Copy your code, or close the editor and open it again." => {
            "Le code d'initialisation a changé alors qu'il était ouvert dans l'éditeur. Copiez votre code, ou fermez l'éditeur et rouvrez-le."
        }
        "The initialization code changed while it was open in the editor, so your code was not applied. Copy it, or close the editor and open it again." => {
            "Le code d'initialisation a changé alors qu'il était ouvert dans l'éditeur : votre code n'a donc pas été appliqué. Copiez-le, ou fermez l'éditeur et rouvrez-le."
        }
        "The initialization code uses a package this Python doesn't have. GridCraft skipped the lines that need it and ran the rest." => {
            "Le code d'initialisation utilise un package que ce Python n'a pas. GridCraft a ignoré les lignes qui en ont besoin et a exécuté le reste."
        }
        "Select a Python cell first." => "Sélectionnez d'abord une cellule Python.",
        "Output As" => "Sortie en tant que",
        "Run\nCell" => "Exécuter\nla cellule",
        "Run the selected Python cell" => "Exécuter la cellule Python sélectionnée",
        "Run\nAll" => "Tout\nexécuter",
        "Run every Python cell in the workbook, in order" => "Exécuter dans l'ordre toutes les cellules Python du classeur",
        "Nothing is running." => "Rien ne s'exécute.",
        "Stop the running cell" => "Arrêter la cellule en cours d'exécution",
        "Clear Python's variables and run every cell again" => "Effacer les variables de Python et réexécuter toutes les cellules",
        "Python Calculation" => "Calcul Python",
        "When this workbook's Python cells run" => "Quand les cellules Python de ce classeur s'exécutent",
        "A Python cell is running." => "Une cellule Python s'exécute.",
        "Python is ready." => "Python est prêt.",
        "Which Python runs cells" => "Quel Python exécute les cellules",
        "Packages" => "Paquets",
        "What this Python has installed" => "Ce que ce Python a installé",
        "Check Sandbox" => "Vérifier le bac à sable",
        "Check Python and the sandbox again, and show the report" => "Vérifier à nouveau Python et le bac à sable, et afficher le rapport",
        "What Python\nCan Do" => "Ce que Python\npeut faire",
        "What Python cells can and can't do here" => "Ce que les cellules Python peuvent et ne peuvent pas faire ici",
        "This Workbook: Enabled" => "Ce classeur : activé",
        "This Workbook: Disabled" => "Ce classeur : désactivé",
        "Only a person can change this." => "Seule une personne peut modifier ceci.",
        "Run Log" => "Journal d'exécution",
        "Each run's cell, time and printed output" => "Cellule, heure et sortie imprimée de chaque exécution",
        "Clear the Run Log" => "Effacer le journal d'exécution",
        "Problems" => "Problèmes",
        "Every Python cell with something to say" => "Toutes les cellules Python ayant quelque chose à signaler",
        "Diagnostics" => "Diagnostic",
        "uv environment" => "environnement uv",
        "uv Python" => "Python uv",
        "virtual environment" => "environnement virtuel",
        "conda environment" => "environnement conda",
        "system" => "système",
        "Python cells need a sandbox, and GridCraft has none for this system." => {
            "Les cellules Python nécessitent un bac à sable, et GridCraft n'en a aucun pour ce système."
        }
        "The sandbox didn't hold when GridCraft checked it, so Python won't run." => {
            "Le bac à sable n'a pas tenu lors de la vérification de GridCraft, donc Python ne s'exécutera pas."
        }
        "Python is off. Turn it on in Settings to run Python cells." => {
            "Python est désactivé. Activez-le dans les paramètres pour exécuter des cellules Python."
        }
        "GridCraft can't find Python 3.11 or newer. Set one up with uv, or choose one." => {
            "GridCraft ne trouve pas Python 3.11 ou ultérieur. Configurez-en un avec uv, ou choisissez-en un."
        }
        "This Python is too old: Python cells need 3.11 or newer." => {
            "Ce Python est trop ancien : les cellules Python nécessitent la version 3.11 ou ultérieure."
        }
        "This is Apple's placeholder for Python, which GridCraft doesn't use. Set up Python with uv, or choose another." => {
            "Il s'agit du substitut Python d'Apple, que GridCraft n'utilise pas. Configurez Python avec uv, ou choisissez-en un autre."
        }
        "GridCraft couldn't work out what this Python needs to run." => "GridCraft n'a pas pu déterminer ce dont ce Python a besoin pour s'exécuter.",
        "Python didn't start." => "Python n'a pas démarré.",
        "Python isn't on for this workbook, so its cells keep the values saved in the file." => {
            "Python n'est pas activé pour ce classeur ; ses cellules conservent donc les valeurs enregistrées dans le fichier."
        }
        "This session doesn't run Python: cells keep the values saved in the file." => {
            "Cette session n'exécute pas Python : les cellules conservent les valeurs enregistrées dans le fichier."
        }
        "This workbook imports packages this Python doesn't have." => "Ce classeur importe des paquets que ce Python n'a pas.",
        "pandas isn't installed in this Python, so ranges come back as lists of rows." => {
            "pandas n'est pas installé dans ce Python ; les plages sont donc renvoyées sous forme de listes de lignes."
        }
        "The initialization code failed, so Python cells can't run." => {
            "Le code d'initialisation a échoué ; les cellules Python ne peuvent donc pas s'exécuter."
        }
        "This code isn't valid Python." => "Ce code n'est pas du Python valide.",
        "The code raised an error." => "Le code a généré une erreur.",
        "The code uses a name that isn't defined." => "Le code utilise un nom qui n'est pas défini.",
        "This name is defined in a later cell. Python cells run in row order, sheet by sheet." => {
            "Ce nom est défini dans une cellule ultérieure. Les cellules Python s'exécutent par ligne, feuille par feuille."
        }
        "This Python doesn't have that package." => "Ce Python n'a pas ce paquet.",
        "This package needs a library outside what the sandbox lets Python read." => {
            "Ce paquet a besoin d'une bibliothèque située hors de ce que le bac à sable permet à Python de lire."
        }
        "Python cells can't read or write files on your computer. Put the data on a sheet and read it with xl()." => {
            "Les cellules Python ne peuvent ni lire ni écrire de fichiers sur votre ordinateur. Placez les données dans une feuille et lisez-les avec xl()."
        }
        "Python cells can't use the network. Put the data on a sheet and read it with xl()." => {
            "Les cellules Python ne peuvent pas utiliser le réseau. Placez les données dans une feuille et lisez-les avec xl()."
        }
        "Python cells can't start other programs or processes." => "Les cellules Python ne peuvent pas démarrer d'autres programmes ni processus.",
        "This picture is too large to draw. Make figsize or dpi smaller." => {
            "Cette image est trop grande pour être dessinée. Réduisez figsize ou dpi."
        }
        "The picture appears in the cell by itself. Python cells can't write files, so this line would fail." => {
            "L'image apparaît d'elle-même dans la cellule. Les cellules Python ne peuvent pas écrire de fichiers, cette ligne échouerait donc."
        }
        "This package draws in a web browser, which Python cells can't open. Use matplotlib or seaborn." => {
            "Ce paquet dessine dans un navigateur web, que les cellules Python ne peuvent pas ouvrir. Utilisez matplotlib ou seaborn."
        }
        "Python Output" => "Sortie Python",
        "Display Plot over Cells" => "Afficher le graphique sur les cellules",
        "Create Reference" => "Créer une référence",
        "Copy Image" => "Copier l'image",
        "Save Image…" => "Enregistrer l'image…",
        "The image is saved." => "L'image est enregistrée.",
        "The image is on the clipboard." => "L'image est dans le presse-papiers.",
        "Image" => "Image",
        "size" => "taille",
        "Resize" => "Redimensionner",
        "This picture shows the last result that cell gave." => "Cette image montre le dernier résultat donné par cette cellule.",
        "Select a Python cell that gave a picture first." => "Sélectionnez d'abord une cellule Python qui a renvoyé une image.",
        "xl() needs its reference written as text in the code, such as xl(\"A1:B5\")." => {
            "xl() a besoin que sa référence soit écrite sous forme de texte dans le code, par exemple xl(\"A1:B5\")."
        }
        "This range is too large to send to Python." => "Cette plage est trop grande pour être envoyée à Python.",
        "xl() names something that isn't a cell, range, table or defined name here." => {
            "xl() désigne quelque chose qui n'est ici ni une cellule, ni une plage, ni un tableau, ni un nom défini."
        }
        "The code ran past the time limit. Python was restarted, so its variables were cleared." => {
            "Le code a dépassé la limite de temps. Python a été redémarré ; ses variables ont donc été effacées."
        }
        "The code went over the memory limit. Python was restarted, so its variables were cleared." => {
            "Le code a dépassé la limite de mémoire. Python a été redémarré ; ses variables ont donc été effacées."
        }
        "The result is too large to bring back to the sheet." => "Le résultat est trop grand pour être ramené dans la feuille.",
        "The printed output was cut off at its limit." => "La sortie imprimée a été tronquée à sa limite.",
        "Python stopped unexpectedly and was restarted, so its variables were cleared." => {
            "Python s'est arrêté de façon inattendue et a été redémarré ; ses variables ont donc été effacées."
        }
        "The result is empty." => "Le résultat est vide.",
        "The result needs cells that aren't empty." => "Le résultat nécessite des cellules qui ne sont pas vides.",
        "The result isn't a number a cell can hold." => "Le résultat n'est pas un nombre qu'une cellule peut contenir.",
        "This date or length of time is outside what a cell can hold." => "Cette date ou cette durée dépasse ce qu'une cellule peut contenir.",
        "This cell is running." => "Cette cellule s'exécute.",
        "What this cell reads has changed since it last ran." => "Ce que lit cette cellule a changé depuis sa dernière exécution.",
        "This cell was stopped before it finished." => "Cette cellule a été arrêtée avant la fin.",
        "xl() needs its reference written as text, such as xl(\"A1:B5\"). A computed reference can't be read." => {
            "xl() a besoin que sa référence soit écrite sous forme de texte, par exemple xl(\"A1:B5\"). Une référence calculée ne peut pas être lue."
        }
        "This module needs the network or other programs, which Python cells can't use." => {
            "Ce module a besoin du réseau ou d'autres programmes, que les cellules Python ne peuvent pas utiliser."
        }
        "This Python doesn't have this package." => "Ce Python n'a pas ce paquet.",
        "Python cells can't open windows or ask for input." => "Les cellules Python ne peuvent pas ouvrir de fenêtres ni demander de saisie.",
        "Python cells don't see your environment variables." => "Les cellules Python ne voient pas vos variables d'environnement.",
        "Open Settings" => "Ouvrir les paramètres",
        "Show Printed Output" => "Afficher la sortie imprimée",
        "Hide Printed Output" => "Masquer la sortie imprimée",
        "Line {line}" => "Ligne {line}",
        "Line {line}: {message}" => "Ligne {line} : {message}",
        "Python {version}, ready" => "Python {version}, prêt",
        "Python {version}, running" => "Python {version}, en cours d’exécution",
        _ => return None,
    })
}
