//! Spanish translations for the interface strings in i18n.rs. Labels follow Excel's Spanish
//! wording (sentence case, «Inmovilizar paneles», «Combinar y centrar»); anything absent falls back
//! to English. Formula names and separators are localized separately (`formula_locale`).

/// Returns the Spanish translation for `text`, or `None` so callers fall back to English.
pub(super) fn translate(text: &str) -> Option<&'static str> {
    Some(match text {
        "Paste" => "Pegar",
        "Paste Values" => "Pegar valores",
        "Paste Formulas" => "Pegar fórmulas",
        "Paste Formatting" => "Pegar formato",
        "Transpose" => "Transponer",
        "Paste Link" => "Pegar vínculo",
        "Paste Special…" => "Pegado especial…",
        "Cut" => "Cortar",
        "Copy" => "Copiar",
        "Format Painter (double-click to keep it on)" => "Copiar formato (doble clic para mantenerlo activo)",
        "Bold" => "Negrita",
        "Italic" => "Cursiva",
        "Double Underline" => "Subrayado doble",
        "Strikethrough" => "Tachado",
        "Underline" => "Subrayado",
        "Increase Font Size" => "Aumentar tamaño de fuente",
        "Decrease Font Size" => "Disminuir tamaño de fuente",
        "Wrap Text" => "Ajustar texto",
        "Merge & Center" => "Combinar y centrar",
        "Merge Cells" => "Combinar celdas",
        "Merge Across" => "Combinar horizontalmente",
        "Unmerge Cells" => "Separar celdas",
        "Fill Color" => "Color de relleno",
        "Font Color" => "Color de fuente",
        "Borders" => "Bordes",
        "Top\nAlign" => "Alinear en la\nparte superior",
        "Middle\nAlign" => "Alinear en\nel medio",
        "Bottom\nAlign" => "Alinear en la\nparte inferior",
        "Left" => "Izquierda",
        "Right" => "Derecha",
        "Center" => "Centro",
        "Horizontal" => "Horizontal",
        "Vertical Text" => "Texto vertical",
        "Decrease Indent" => "Disminuir sangría",
        "Increase Indent" => "Aumentar sangría",
        "Orientation" => "Orientación",
        "Angle Counterclockwise" => "Ángulo ascendente",
        "Angle Clockwise" => "Ángulo descendente",
        "Rotate Text Up" => "Girar texto hacia arriba",
        "Rotate Text Down" => "Girar texto hacia abajo",
        "Accounting Number Format" => "Formato de número de contabilidad",
        "Percent Style" => "Estilo porcentual",
        "Comma Style" => "Estilo de millares",
        "Increase Decimal" => "Aumentar decimales",
        "Decrease Decimal" => "Disminuir decimales",
        "More Accounting Formats…" => "Más formatos de contabilidad…",
        "Conditional\nFormatting" => "Formato\ncondicional",
        "Conditional Formatting" => "Formato condicional",
        "Format\nas Table" => "Dar formato\ncomo tabla",
        "Cell\nStyles" => "Estilos\nde celda",
        "Format Cells…" => "Formato de celdas…",
        "Format Cell Alignment…" => "Alineación de celdas…",
        "Format\nPane" => "Panel\nde formato",
        "Format" => "Formato",
        "Insert" => "Insertar",
        "Header Menu|Insert" => "Insertar",
        "Insert Cells…" => "Insertar celdas…",
        "Delete" => "Eliminar",
        "Delete Cells…" => "Eliminar celdas…",
        "Insert Sheet Rows" => "Insertar filas de hoja",
        "Insert Sheet Columns" => "Insertar columnas de hoja",
        "Delete Sheet Rows" => "Eliminar filas de hoja",
        "Delete Sheet Columns" => "Eliminar columnas de hoja",
        "Row Height…" => "Alto de fila…",
        "AutoFit Row Height" => "Autoajustar alto de fila",
        "Column Width…" => "Ancho de columna…",
        "AutoFit Column Width" => "Autoajustar ancho de columna",
        "Default Width…" => "Ancho predeterminado…",
        "Hide Rows" => "Ocultar filas",
        "Unhide Rows" => "Mostrar filas",
        "Hide Columns" => "Ocultar columnas",
        "Unhide Columns" => "Mostrar columnas",
        "Lock Cell" => "Bloquear celda",
        "Clear" => "Borrar",
        "Clear All" => "Borrar todo",
        "Clear Formats" => "Borrar formatos",
        "Clear Contents" => "Borrar contenido",
        "Clear Comments and Notes" => "Borrar comentarios y notas",
        "Clear Hyperlinks" => "Borrar hipervínculos",
        "AutoSum" => "Autosuma",
        "Flash\nFill" => "Relleno\nrápido",
        "Flash Fill" => "Relleno rápido",
        "Fill" => "Rellenar",
        "Clear Print Area" => "Borrar área de impresión",
        "Find &\nSelect" => "Buscar y\nseleccionar",
        "Find…" => "Buscar…",
        "Replace…" => "Reemplazar…",
        "Go To…" => "Ir a…",
        "Go To Special…" => "Ir a Especial…",
        "PivotTable" => "Tabla dinámica",
        "Recommended\nCharts" => "Gráficos\nrecomendados",
        "Table" => "Tabla",
        "Pictures" => "Imágenes",
        "Shapes" => "Formas",
        "Icons" => "Iconos",
        "Text\nBox" => "Cuadro\nde texto",
        "Comment" => "Comentario",
        "New\nComment" => "Nuevo\ncomentario",
        "New Note" => "Nueva nota",
        "Show/Hide Note" => "Mostrar u ocultar nota",
        "Header &\nFooter" => "Encabezado y\npie de página",
        "Text to\nColumns" => "Texto en\ncolumnas",
        "Link" => "Vínculo",
        "Symbol" => "Símbolo",
        "Equation" => "Ecuación",
        "Insert New Equation..." => "Insertar nueva ecuación",
        "Sparklines" => "Minigráficos",
        "Header & Footer" => "Encabezado y pie de página",
        "Draw with ink" => "Dibujar con lápiz",
        "Eraser" => "Borrador",
        "Ink to\nShape" => "Entrada de lápiz\na forma",
        "Select\nObjects" => "Seleccionar\nobjetos",
        "Themes" => "Temas",
        "Margins" => "Márgenes",
        "Left Margin" => "Margen izquierdo",
        "Right Margin" => "Margen derecho",
        "Top Margin" => "Margen superior",
        "Bottom Margin" => "Margen inferior",
        "Header Margin" => "Margen del encabezado",
        "Footer Margin" => "Margen del pie de página",
        "Size" => "Tamaño",
        "Print\nArea" => "Área de\nimpresión",
        "Set Print Area" => "Establecer área de impresión",
        "Breaks" => "Saltos",
        "Insert Page Break" => "Insertar salto de página",
        "Remove Page Break" => "Quitar salto de página",
        "Reset All Page Breaks" => "Restablecer todos los saltos de página",
        "Print\nTitles" => "Imprimir\ntítulos",
        "Narrow" => "Estrecho",
        "Wide" => "Ancho",
        "Normal" => "Normal",
        "Portrait" => "Vertical",
        "Landscape" => "Horizontal",
        "Custom Margins…" => "Márgenes personalizados…",
        "Insert\nFunction" => "Insertar\nfunción",
        "Name\nManager" => "Administrador\nde nombres",
        "Define Name" => "Definir nombre",
        "Use in Formula" => "Utilizar en la fórmula",
        "Create from Selection" => "Crear desde la selección",
        "Trace Precedents" => "Rastrear precedentes",
        "Trace Dependents" => "Rastrear dependientes",
        "Remove Arrows" => "Quitar flechas",
        "Show Formulas" => "Mostrar fórmulas",
        "Error Checking" => "Comprobación de errores",
        "Trace Error" => "Rastrear error",
        "Circular References" => "Referencias circulares",
        "Python Mode" => "Modo Python",
        "Python Object" => "Objeto de Python",
        "Excel Value" => "Valor de Excel",
        "Copy as CSV" => "Copiar como CSV",
        "Copy as Markdown" => "Copiar como Markdown",
        "Save as .csv…" => "Guardar como .csv…",
        "Spill Here" => "Desbordar aquí",
        "Spill One Line per Row" => "Desbordar una línea por fila",
        "The table isn't kept in the file. Run the cell to see it." => "La tabla no se guarda en el archivo. Ejecute la celda para verla.",
        "This table is empty." => "Esta tabla está vacía.",
        "This table has no rows." => "Esta tabla no tiene filas.",
        "Copied only what is shown: the rest of the table isn't in the copy." => {
            "Solo se copió lo que se muestra: el resto de la tabla no está en la copia."
        }
        "Saved only what is shown: the rest of the table isn't in the file." => {
            "Solo se guardó lo que se muestra: el resto de la tabla no está en el archivo."
        }
        "The table is saved." => "La tabla se guardó.",
        "Only the first {rows} of {total} rows are shown." => "Solo se muestran las primeras {rows} de {total} filas.",
        "Only the first {cols} of {total} columns are shown." => "Solo se muestran las primeras {cols} de {total} columnas.",
        "Some long text is cut." => "Parte del texto largo está cortado.",
        "Copying and saving take only what is shown. Spill Here gives the whole table." => {
            "Copiar y guardar solo toman lo que se muestra. Desbordar aquí da la tabla completa."
        }
        "Save Python Table as CSV" => "Guardar tabla de Python como CSV",
        "Python Table" => "Tabla de Python",
        "Convert to Normal Cell" => "Convertir en celda normal",
        "Show Card" => "Mostrar tarjeta",
        "Insert\nPython" => "Insertar\nPython",
        "Insert Python (Ctrl+Alt+Shift+P)" => "Insertar Python (Ctrl+Alt+Shift+P)",
        "Python Editor" => "Editor de Python",
        "Edit in Python Editor" => "Editar en el editor de Python",
        "Unsaved changes" => "Cambios sin guardar",
        "The code for {cell} has unsaved changes." => "El código de {cell} tiene cambios sin guardar.",
        "The code for {cell} in {workbook} has unsaved changes." => "El código de {cell} en {workbook} tiene cambios sin guardar.",
        "The code can't be longer: a Python cell holds {max} characters, counted as a PY formula." => {
            "El código no puede ser más largo: una celda de Python admite {max} caracteres, contados como fórmula PY."
        }
        "{cell} changed while it was open in the Python editor. Copy the code, or close the editor and open it again on the cell." => {
            "{cell} cambió mientras estaba abierta en el editor de Python. Copie el código, o cierre el editor y vuelva a abrirlo en la celda."
        }
        "{cell} changed while it was open in the Python editor, so the code was not applied. Copy the code, or close the editor and open it again on the cell." => {
            "{cell} cambió mientras estaba abierta en el editor de Python, así que el código no se aplicó. Copie el código, o cierre el editor y vuelva a abrirlo en la celda."
        }
        "{apply} applies the code. Esc closes the editor." => "{apply} aplica el código. Esc cierra el editor.",
        "The Python editor's unsaved code was copied to the clipboard." => "El código sin guardar del editor de Python se copió en el portapapeles.",
        "Apply" => "Aplicar",
        "Discard" => "Descartar",
        "Cancel" => "Cancelar",
        "Evaluate Formula" => "Evaluar fórmula",
        "Watch Window" => "Ventana Inspección",
        "Calculation\nOptions" => "Opciones\nde cálculo",
        "Calculate Now" => "Calcular ahora",
        "Calculate Sheet" => "Calcular hoja",
        "Automatic" => "Automático",
        "Automatic Except for Data Tables" => "Automático excepto en las tablas de datos",
        "Manual" => "Manual",
        "Sort &\nFilter" => "Ordenar\ny filtrar",
        "Sort" => "Ordenar",
        "Sort A to Z" => "Ordenar de A a Z",
        "Sort Z to A" => "Ordenar de Z a A",
        "Custom Sort…" => "Orden personalizado…",
        "Filter" => "Filtro",
        "Reapply" => "Volver a aplicar",
        "Get Data\n(Text/CSV)" => "Obtener datos\n(texto/CSV)",
        "Remove\nDuplicates" => "Quitar\nduplicados",
        "Data\nValidation" => "Validación\nde datos",
        "Data Validation" => "Validación de datos",
        "Data Validation…" => "Validación de datos…",
        "Circle Invalid Data" => "Rodear con un círculo datos no válidos",
        "Clear Validation" => "Borrar círculos de validación",
        "Group" => "Agrupar",
        "Ungroup" => "Desagrupar",
        "Show Detail" => "Mostrar detalle",
        "Hide Detail" => "Ocultar detalle",
        "Subtotal" => "Subtotal",
        "What-If\nAnalysis" => "Análisis de\nhipótesis",
        "Data Table…" => "Tabla de datos…",
        "Scenario Manager…" => "Administrador de escenarios…",
        "Goal Seek…" => "Buscar objetivo…",
        "Spelling" => "Ortografía",
        "Check\nAccessibility" => "Comprobar\naccesibilidad",
        "Threaded Comments" => "Comentarios en conversación",
        "Show\nComments" => "Mostrar\ncomentarios",
        "Protect\nWorkbook" => "Proteger\nlibro",
        "Unprotect\nSheet" => "Desproteger\nhoja",
        "Protect Sheet…" => "Proteger hoja…",
        "Unprotect Sheet…" => "Desproteger hoja…",
        "Comments" => "Comentarios",
        "Notes" => "Notas",
        "Page Break\nPreview" => "Vista previa de\nsalto de página",
        "Page\nLayout" => "Diseño\nde página",
        "Formula Bar" => "Barra de fórmulas",
        "Name Box" => "Cuadro de nombre",
        "Cell Editor" => "Editor de celda",
        "Font Size" => "Tamaño de fuente",
        "Gridlines" => "Líneas de cuadrícula",
        "Headings" => "Encabezados",
        "Zoom" => "Zoom",
        "Magnification" => "Ampliación",
        "Fit selection" => "Ajustar la selección",
        "Forecast\nSheet" => "Hoja de\npronóstico",
        "Add footer" => "Agregar pie de página",
        "Add header" => "Agregar encabezado",
        "Click to add data" => "Haga clic para agregar datos",
        "Current Date" => "Fecha actual",
        "Current\nDate" => "Fecha\nactual",
        "Current\nTime" => "Hora\nactual",
        "Current Time" => "Hora actual",
        "File Name" => "Nombre de archivo",
        "File\nName" => "Nombre de\narchivo",
        "File\nPath" => "Ruta de\narchivo",
        "File Path" => "Ruta de archivo",
        "Footer" => "Pie de página",
        "Different First Page" => "Primera página diferente",
        "Different Odd & Even Pages" => "Páginas pares e impares diferentes",
        "Scale with Document" => "Escalar con el documento",
        "Align with Page Margins" => "Alinear con los márgenes de página",
        "Go to Footer" => "Ir al pie de página",
        "Go to Header" => "Ir al encabezado",
        "Go to\nFooter" => "Ir al pie\nde página",
        "Go to\nHeader" => "Ir al\nencabezado",
        "Header" => "Encabezado",
        "(none)" => "(ninguno)",
        "Number\nof Pages" => "Número\nde páginas",
        "Number of Pages" => "Número de páginas",
        "Page\nNumber" => "Número de\npágina",
        "Page Number" => "Número de página",
        "Ruler" => "Regla",
        "Sheet Name" => "Nombre de la hoja",
        "Sheet\nName" => "Nombre de\nla hoja",
        "Animations" => "Animaciones",
        "Glide the selection outline to the cell you move to instead of jumping there" => {
            "Desliza el contorno de la selección hasta la celda a la que te mueves en lugar de saltar"
        }
        "Insert Copied Cells" => "Insertar celdas copiadas",
        "Insert Copied Cells…" => "Insertar celdas copiadas…",
        "Insert Cut Cells" => "Insertar celdas cortadas",
        "Insert Paste" => "Insertar y pegar",
        "Previous" => "Anterior",
        "Next" => "Siguiente",
        "Previous Comment" => "Comentario anterior",
        "Next Comment" => "Comentario siguiente",
        "Show All Notes" => "Mostrar todas las notas",
        "Convert to Comments" => "Convertir en comentarios",
        "Checkbox" => "Casilla",
        "Insert checkboxes in the selected cells" => "Insertar casillas en las celdas seleccionadas",
        "Button" => "Botón",
        "Slider" => "Control deslizante",
        "Spin Button" => "Control de número",
        "Draw a button that runs a macro or a command" => "Dibujar un botón que ejecuta una macro o un comando",
        "Draw a slider that sets a whole number in a linked cell" => {
            "Dibujar un control deslizante que establece un número entero en una celda vinculada"
        }
        "Draw a spin button that steps a whole number in a linked cell" => {
            "Dibujar un control de número que cambia un número entero en una celda vinculada"
        }
        "Edit Text" => "Modificar texto",
        "Assign Action…" => "Asignar acción…",
        "Format Control…" => "Formato de control…",
        "Sheet\nRight-to-Left" => "Hoja de derecha\na izquierda",
        "Sheet Right-to-Left" => "Hoja de derecha a izquierda",
        "Data\nAnalysis" => "Análisis\nde datos",
        "Data Analysis" => "Análisis de datos",
        "Solver" => "Solver",
        "Allow Edit\nRanges" => "Permitir editar\nrangos",
        "Allow Edit Ranges" => "Permitir editar rangos",
        "Drag to freeze columns" => "Arrastrar para inmovilizar columnas",
        "Drag to freeze rows" => "Arrastrar para inmovilizar filas",
        "Select\nData" => "Seleccionar\ndatos",
        "File" => "Archivo",
        "Back" => "Atrás",
        "New" => "Nuevo",
        "Open" => "Abrir",
        "Info" => "Información",
        "Save" => "Guardar",
        "Save As" => "Guardar como",
        "Export" => "Exportar",
        "Close" => "Cerrar",
        "Options" => "Opciones",
        "Account" => "Cuenta",
        "Product Information" => "Información del producto",
        "About GridCraft" => "Acerca de GridCraft",
        "Version" => "Versión",
        "Platform" => "Plataforma",
        "Licence" => "Licencia",
        "Version, contributors and the AI models that helped" => "Versión, colaboradores y los modelos de IA que ayudaron",
        "Blank workbook" => "Libro en blanco",
        "Recent" => "Recientes",
        "No recent workbooks yet." => "Todavía no hay libros recientes.",
        "More workbooks" => "Más libros",
        "Modified" => "Modificado",
        "Not available" => "No disponible",
        "Just now" => "Ahora mismo",
        "Yesterday" => "Ayer",
        "{n} min ago" => "hace {n} min",
        "{n} h ago" => "hace {n} h",
        "{n} days ago" => "hace {n} días",
        "{n} weeks ago" => "hace {n} semanas",
        "{n} months ago" => "hace {n} meses",
        "{n} years ago" => "hace {n} años",
        "Browse…" => "Examinar…",
        "Not saved yet" => "Aún no se ha guardado",
        "Location" => "Ubicación",
        "Sheets" => "Hojas",
        "Protect Workbook" => "Proteger libro",
        "Sheet structure is locked: sheets can't be added, moved, renamed or deleted." => {
            "La estructura de hojas está bloqueada: no se pueden agregar, mover, cambiar de nombre ni eliminar hojas."
        }
        "Lock the sheet structure so sheets can't be added, moved, renamed or deleted." => {
            "Bloquea la estructura de hojas para que no se puedan agregar, mover, cambiar de nombre ni eliminar hojas."
        }
        "Properties" => "Propiedades",
        "Title" => "Título",
        "Subject" => "Asunto",
        "Author" => "Autor",
        "Company" => "Organización",
        "Keywords" => "Palabras clave",
        "Description" => "Descripción",
        "No workbook is open." => "No hay ningún libro abierto.",
        "Save as type" => "Tipo",
        "Excel Workbook (*.xlsx)" => "Libro de Excel (*.xlsx)",
        "CSV UTF-8 (Comma delimited) (*.csv)" => "CSV UTF-8 (delimitado por comas) (*.csv)",
        "Web Page (*.html)" => "Página web (*.html)",
        "PDF (*.pdf)" => "Archivo PDF (*.pdf)",
        "Download" => "Descargar",
        "A CSV file, web page or PDF is a copy: the workbook keeps its own file." => {
            "Un archivo CSV, una página web o un PDF son una copia: el libro conserva su propio archivo."
        }
        "Save the workbook under another name or in another folder." => "Guarda el libro con otro nombre o en otra carpeta.",
        "Print Active Sheets" => "Imprimir hojas activas",
        "Print Entire Workbook" => "Imprimir todo el libro",
        "Fit Sheet on One Page" => "Ajustar hoja en una página",
        "Page Setup…" => "Configurar página…",
        "Page {page} of {pages}" => "Página {page} de {pages}",
        "Nothing to print." => "No hay nada que imprimir.",
        "Previous page" => "Página anterior",
        "Next page" => "Página siguiente",
        "Create PDF" => "Crear PDF",
        "Save the sheet as a PDF document that keeps its page layout." => "Guarda la hoja como documento PDF conservando el diseño de página.",
        "Change File Type" => "Cambiar el tipo de archivo",
        "CSV (Comma delimited)" => "CSV (delimitado por comas)",
        "Save the active sheet as comma-separated text." => "Guarda la hoja activa como texto separado por comas.",
        "Web Page (.html)" => "Página web (.html)",
        "Save the workbook as a web page." => "Guarda el libro como página web.",
        "Options|General" => "General",
        "Options|Save" => "Guardar",
        "Show formula bar" => "Mostrar barra de fórmulas",
        "Highlight the active cell's row" => "Resaltar la fila de la celda activa",
        "Display options for this workbook" => "Opciones de presentación para este libro",
        "Show horizontal scroll bar" => "Mostrar barra de desplazamiento horizontal",
        "Show vertical scroll bar" => "Mostrar barra de desplazamiento vertical",
        "Show sheet tabs" => "Mostrar pestañas de hojas",
        "Edit Hyperlink…" => "Modificar hipervínculo…",
        "Edit Hyperlink" => "Modificar hipervínculo",
        "Open Hyperlink" => "Abrir hipervínculo",
        "Remove Hyperlink" => "Quitar hipervínculo",
        "Resize formula bar" => "Cambiar el tamaño de la barra de fórmulas",
        "Enable AutoComplete for cell values" => "Habilitar Autocompletar para valores de celda",
        "Save .xlsx workbooks automatically after every change" => "Guardar automáticamente los libros .xlsx tras cada cambio",
        "Workbook calculation" => "Cálculo del libro",
        "Enable iterative calculation" => "Habilitar cálculo iterativo",
        "Maximum Iterations:" => "Iteraciones máximas:",
        "Maximum Change:" => "Cambio máximo:",
        "Maximum Iterations must be a whole number from 1 to 32767." => "Las iteraciones máximas deben ser un número entero de 1 a 32767.",
        "Maximum Change must be a number of 0 or more." => "El cambio máximo debe ser un número igual o mayor que 0.",
        "Calculation settings are saved with the workbook." => "La configuración de cálculo se guarda con el libro.",
        "Interface size" => "Tamaño de la interfaz",
        "Auto" => "Automático",
        "100%" => "100%",
        "Zoom to\nSelection" => "Ampliar\nselección",
        "Freeze\nPanes" => "Inmovilizar\npaneles",
        "Freeze Panes" => "Inmovilizar paneles",
        "Freeze Top Row" => "Inmovilizar fila superior",
        "Freeze First Column" => "Inmovilizar primera columna",
        "Unfreeze Panes" => "Movilizar paneles",
        "Dark Mode" => "Modo oscuro",
        "Interface language" => "Idioma de la interfaz",
        "Command\nPalette" => "Paleta de\ncomandos",
        "Agent\nControl" => "Control\ndel agente",
        "Action\nJournal" => "Registro\nde acciones",
        "About\nGridCraft" => "Acerca de\nGridCraft",
        "Share" => "Compartir",
        "Add Chart\nElement" => "Agregar elemento\nde gráfico",
        "Change\nChart Type" => "Cambiar tipo\nde gráfico",
        "Switch\nRow/Column" => "Cambiar entre\nfilas y columnas",
        "Data Labels: None" => "Etiquetas de datos: ninguna",
        "Data Labels: Show" => "Etiquetas de datos: mostrar",
        "Legend: None" => "Leyenda: ninguna",
        "Legend: Bottom" => "Leyenda: abajo",
        "Legend: Top" => "Leyenda: arriba",
        "Legend: Right" => "Leyenda: derecha",
        "First slice angle" => "Ángulo del primer sector",
        "Doughnut hole size" => "Tamaño del orificio del anillo",
        "Gridlines: None" => "Líneas de cuadrícula: ninguna",
        "Gridlines: Show" => "Líneas de cuadrícula: mostrar",
        "Chart\nTitle" => "Título\ndel gráfico",
        "Delete\nChart" => "Eliminar\ngráfico",
        "Convert\nto Range" => "Convertir\nen rango",
        "Table\nStyles" => "Estilos\nde tabla",
        "Top 10 Items" => "10 superiores",
        "Top 10%" => "10% superior",
        "Bottom 10 Items" => "10 inferiores",
        "Bottom 10%" => "10% inferior",
        "Above Average" => "Por encima del promedio",
        "Below Average" => "Por debajo del promedio",
        "Clear Rules from Entire Sheet" => "Borrar reglas de toda la hoja",
        "Clear Rules from Selected Cells" => "Borrar reglas de las celdas seleccionadas",
        "Constants" => "Constantes",
        "Data Bars" => "Barras de datos",
        "Color Scales" => "Escalas de color",
        "Icon Sets" => "Conjuntos de iconos",
        "Home" => "Inicio",
        "Draw" => "Dibujar",
        "Page Layout" => "Disposición de página",
        "Formulas" => "Fórmulas",
        "Data" => "Datos",
        "Review" => "Revisar",
        "View" => "Vista",
        "Automate" => "Automatizar",
        "Table Design" => "Diseño de tabla",
        "Chart Design" => "Diseño de gráfico",
        "Ready" => "Listo",
        "Enter" => "Introducir",
        "Edit" => "Modificar",
        "Point" => "Señalar",
        "Calculate" => "Calcular",
        "Select destination and press Enter or choose Paste" => "Seleccione el destino y presione Entrar o elija Pegar",
        "General" => "General",
        "Number" => "Número",
        "Currency" => "Moneda",
        "Accounting" => "Contabilidad",
        "Date" => "Fecha",
        "Time" => "Hora",
        "Percentage" => "Porcentaje",
        "Fraction" => "Fracción",
        "Scientific" => "Científica",
        "Text" => "Texto",
        "Special" => "Especial",
        "Custom" => "Personalizada",
        "Short Date" => "Fecha corta",
        "Long Date" => "Fecha larga",
        "More Number Formats…" => "Más formatos de número…",
        "Financial" => "Financieras",
        "Recently Used" => "Usadas recientemente",
        "Logical" => "Lógicas",
        "Date & Time" => "Fecha y hora",
        "Lookup & Reference" => "Búsqueda y referencia",
        "Math & Trig" => "Matemáticas y trigonométricas",
        "More Functions" => "Más funciones",
        "Page Layout|Orientation" => "Orientación",
        "Display theme" => "Tema de la interfaz",
        "System" => "Sistema",
        "Light" => "Claro",
        "Dark" => "Oscuro",
        "Sheet Options|View" => "Ver",
        "Print" => "Imprimir",
        "Print What" => "Qué imprimir",
        "PivotTable Fields" => "Campos de tabla dinámica",
        "Choose fields to add to report:" => "Seleccione los campos para agregar al informe:",
        "Use a field's menu to move it between areas:" => "Use el menú de un campo para moverlo entre áreas:",
        "Filters" => "Filtros",
        "Columns" => "Columnas",
        "Rows" => "Filas",
        "Values" => "Valores",
        "Move to Row Labels" => "Mover a rótulos de fila",
        "Move to Column Labels" => "Mover a rótulos de columna",
        "Move to Report Filter" => "Mover al filtro de informe",
        "Move to Values" => "Mover a valores",
        "Summarize by Sum" => "Resumir por suma",
        "Summarize by Count" => "Resumir por cuenta",
        "Summarize by Average" => "Resumir por promedio",
        "Summarize by Max" => "Resumir por máximo",
        "Summarize by Min" => "Resumir por mínimo",
        "Show as No Calculation" => "Mostrar como sin cálculo",
        "Show as % of Grand Total" => "Mostrar como % del total general",
        "Show as Running Total" => "Mostrar como total acumulado",
        "Show as Rank" => "Mostrar como clasificación",
        "Group by Years" => "Agrupar por años",
        "Group by Quarters" => "Agrupar por trimestres",
        "Group by Months" => "Agrupar por meses",
        "Remove Field" => "Quitar campo",
        "Refresh" => "Actualizar",
        "Calculated Field…" => "Campo calculado…",
        "Report Layout" => "Diseño de informe",
        "Compact" => "Compacto",
        "Outline" => "Esquema",
        "Tabular" => "Tabular",
        "Insert Sheet" => "Insertar hoja",
        "Rename" => "Cambiar nombre",
        "Move or Copy…" => "Mover o copiar…",
        "Duplicate" => "Duplicar",
        "Tab Color" => "Color de etiqueta",
        "No Color" => "Sin color",
        "Hide" => "Ocultar",
        "Unhide…" => "Mostrar…",
        "Select All Sheets" => "Seleccionar todas las hojas",
        "AutoSave" => "Autoguardado",
        "AutoSave: save after every change (workbooks saved to a file)" => {
            "Autoguardado: guardar después de cada cambio (libros guardados en un archivo)"
        }
        "Previous sheet" => "Hoja anterior",
        "Next sheet" => "Hoja siguiente",
        "All sheets" => "Todas las hojas",
        "New sheet (⇧F11)" => "Nueva hoja (⇧F11)",
        "Text Box" => "Cuadro de texto",
        "Macros" => "Macros",
        "Record Macro" => "Grabar macro",
        "Stop Recording" => "Detener grabación",
        "Use Relative References" => "Usar referencias relativas",
        "Run\nMacro" => "Ejecutar macro",
        "Macros…" => "Macros…",
        "Recording" => "Grabando",
        "Stop" => "Detener",
        "New Macro" => "Nueva macro",
        "Show" => "Mostrar",
        "Collapse dialog" => "Contraer diálogo",
        "Expand dialog" => "Expandir diálogo",
        "Python|Off" => "Desactivado",
        "Python|Not Set Up" => "Sin configurar",
        "Python|Ready" => "Listo",
        "Python|Running" => "En ejecución",
        "Python|Blocked" => "Bloqueado",
        "Enable Python in cells" => "Habilitar Python en las celdas",
        "Waiting for Python cells to finish before saving..." => "Esperando a que terminen las celdas de Python antes de guardar...",
        "Save Now" => "Guardar ahora",
        "The workbook was saved, but these Python cells weren't finished, so the file has their last results, if they had any: {cells}" => {
            "El libro se guardó, pero estas celdas de Python no terminaron, así que el archivo tiene sus últimos resultados, si los tenían: {cells}"
        }
        "{cells} and {count} more" => "{cells} y {count} más",
        "{version}, too old: Python cells need 3.11 or newer" => {
            "{version}, demasiado antigua: las celdas de Python requieren la versión 3.11 o posterior"
        }
        "Looking for Python…" => "Buscando Python…",
        "Copied to the clipboard." => "Copiado en el Portapapeles.",
        "This workbook has Python code. Enable it to run its cells; until then they keep the values saved in the file." => {
            "Este libro contiene código de Python. Habilítelo para ejecutar sus celdas; hasta entonces conservan los valores guardados en el archivo."
        }
        "Enable" => "Habilitar",
        "Python: running {cell}, {seconds} s" => "Python: ejecutando {cell}, {seconds} s",
        "Python: {state}" => "Python: {state}",
        "Open the Python environment" => "Abrir el entorno de Python",
        "Nothing on this system: Python doesn't run here" => "Nada en este sistema: Python no se ejecuta aquí",
        "GridCraft starts Python with an empty environment" => "GridCraft inicia Python con un entorno vacío",
        "macOS sandbox profile" => "Perfil de sandbox de macOS",
        "Linux Landlock" => "Linux Landlock",
        "Linux seccomp filter" => "Filtro seccomp de Linux",
        "Open Python Settings" => "Abrir la configuración de Python",
        "Choose Interpreter" => "Elegir intérprete",
        "Copy Install Command" => "Copiar comando de instalación",
        "Run Again" => "Ejecutar de nuevo",
        "Reset" => "Restablecer",
        "Python Environment" => "Entorno de Python",
        "Python Packages" => "Paquetes de Python",
        "Python Problems" => "Problemas de Python",
        "Python Run Log" => "Registro de ejecución de Python",
        "What Python Can Do" => "Qué puede hacer Python",
        "GridCraft is checking Python." => "GridCraft está comprobando Python.",
        "Check Again" => "Comprobar de nuevo",
        "Interpreter" => "Intérprete",
        "Chosen by you:" => "Elegido por usted:",
        "Automatic found:" => "Encontrado automáticamente:",
        "What runs:" => "Qué se ejecuta:",
        "None found yet." => "Aún no se ha encontrado ninguno.",
        "Sandbox" => "Sandbox",
        "Package folders" => "Carpetas de paquetes",
        "What Python can read" => "Qué puede leer Python",
        "uv manages Python versions and packages. To get it, run:" => {
            "uv administra las versiones y los paquetes de Python. Para obtenerlo, ejecute:"
        }
        "Reading files outside Python: blocked" => "Lectura de archivos fuera de Python: bloqueada",
        "Writing files: blocked" => "Escritura de archivos: bloqueada",
        "Using the network: blocked" => "Uso de la red: bloqueado",
        "Starting other programs: blocked" => "Inicio de otros programas: bloqueado",
        "Each time Python starts, GridCraft tries each of these first, and doesn't run Python unless every one is refused." => {
            "Cada vez que se inicia Python, GridCraft prueba cada una de estas medidas y no ejecuta Python a menos que todas sean rechazadas."
        }
        "Install a package with uv. GridCraft only copies the command; you run it in a terminal, then choose Check Again." => {
            "Instale un paquete con uv. GridCraft solo copia el comando; usted lo ejecuta en un terminal y después elige Comprobar de nuevo."
        }
        "Package name" => "Nombre del paquete",
        "Other package managers" => "Otros administradores de paquetes",
        "For people who manage Python another way: choose any interpreter by path in File › Options › Python. With pip:" => {
            "Para quienes administran Python de otra manera: elija cualquier intérprete por su ruta en Archivo › Opciones › Python. Con pip:"
        }
        "Packages: {count}" => "Paquetes: {count}",
        "No problems." => "Sin problemas.",
        "This workbook" => "Este libro",
        "No runs yet." => "Aún no hay ejecuciones.",
        "Go To Cell" => "Ir a la celda",
        "This run printed nothing." => "Esta ejecución no imprimió nada.",
        "Python cells can" => "Las celdas de Python pueden",
        "Read cells through xl()" => "Leer celdas mediante xl()",
        "GridCraft sends only the cells that xl() names" => "GridCraft envía solo las celdas que nombra xl()",
        "Import installed packages" => "Importar paquetes instalados",
        "The sandbox lets Python read its own installation" => "El sandbox permite que Python lea su propia instalación",
        "Compute" => "Calcular",
        "Within the time and memory limits in File › Options › Python" => {
            "Dentro de los límites de tiempo y memoria de Archivo › Opciones › Python"
        }
        "Printed output is kept in the Run Log" => "La salida impresa se conserva en el registro de ejecución",
        "Use threads" => "Usar subprocesos",
        "Threads stay inside the one Python process" => "Los subprocesos permanecen dentro del único proceso de Python",
        "Python cells cannot" => "Las celdas de Python no pueden",
        "Read your files" => "Leer sus archivos",
        "Write files" => "Escribir archivos",
        "Use the network" => "Usar la red",
        "Start programs" => "Iniciar programas",
        "See environment variables" => "Ver variables de entorno",
        "Open windows" => "Abrir ventanas",
        "Draw plots. The data goes to a sheet and a chart is built from it." => {
            "Dibujar gráficos. Los datos van a una hoja y se crea un gráfico a partir de ellos."
        }
        "Run this workbook's Python cells" => "Ejecutar las celdas de Python de este libro",
        "Off by default. Only you can turn this on, here: no file, macro or agent can. A workbook opened from a file also needs its own Enable." => {
            "Desactivado de forma predeterminada. Solo usted puede activarlo, aquí: ningún archivo, macro o agente puede hacerlo. Un libro abierto desde un archivo también necesita su propia habilitación."
        }
        "Limits" => "Límites",
        "While Python is off, GridCraft starts nothing, so it can't yet say which version it would use." => {
            "Mientras Python está desactivado, GridCraft no inicia nada, por lo que aún no puede indicar qué versión usaría."
        }
        "Automatic would try first:" => "Automático probaría primero:",
        "Set up Python with uv" => "Configurar Python con uv",
        "Run these in a terminal, in order. GridCraft looks in this environment first, so there is nothing more to choose. It only copies the commands; it never runs them." => {
            "Ejecute estos comandos en un terminal, en orden. GridCraft busca primero en este entorno, así que no queda nada más por elegir. Solo copia los comandos; nunca los ejecuta."
        }
        "Any Python 3.11 or newer can be chosen by path with Browse…. To add packages to it with pip:" => {
            "Se puede elegir por su ruta cualquier Python 3.11 o posterior con Examinar…. Para agregarle paquetes con pip:"
        }
        "Time limit for one cell" => "Límite de tiempo para una celda",
        " seconds" => " segundos",
        "Memory limit" => "Límite de memoria",
        " MB" => " MB",
        "At a limit Python is restarted and its variables are cleared; the next run starts fresh." => {
            "Al llegar a un límite, Python se reinicia y se borran sus variables; la siguiente ejecución empieza de cero."
        }
        "On macOS the memory limit is a check twice a second, not a hard cap." => {
            "En macOS el límite de memoria se comprueba dos veces por segundo; no es un tope estricto."
        }
        "Editor" => "Editor",
        "Initialization" => "Inicialización",
        "The code that runs before this workbook's Python cells" => "El código que se ejecuta antes de las celdas de Python de este libro",
        "Open Initialization" => "Abrir inicialización",
        "The initialization code has unsaved changes." => "El código de inicialización tiene cambios sin guardar.",
        "The initialization code of {workbook} has unsaved changes." => "El código de inicialización de {workbook} tiene cambios sin guardar.",
        "The code can't be longer: initialization code holds {max} characters." => {
            "El código no puede ser más largo: el código de inicialización admite {max} caracteres."
        }
        "The initialization code changed while it was open in the editor. Copy your code, or close the editor and open it again." => {
            "El código de inicialización cambió mientras estaba abierto en el editor. Copie su código, o cierre el editor y vuelva a abrirlo."
        }
        "The initialization code changed while it was open in the editor, so your code was not applied. Copy it, or close the editor and open it again." => {
            "El código de inicialización cambió mientras estaba abierto en el editor, por lo que su código no se aplicó. Cópielo, o cierre el editor y vuelva a abrirlo."
        }
        "The initialization code uses a package this Python doesn't have. GridCraft skipped the lines that need it and ran the rest." => {
            "El código de inicialización usa un paquete que este Python no tiene. GridCraft omitió las líneas que lo necesitan y ejecutó el resto."
        }
        "Select a Python cell first." => "Seleccione primero una celda de Python.",
        "Output As" => "Mostrar salida como",
        "Run\nCell" => "Ejecutar\ncelda",
        "Run the selected Python cell" => "Ejecutar la celda de Python seleccionada",
        "Run\nAll" => "Ejecutar\ntodo",
        "Run every Python cell in the workbook, in order" => "Ejecutar en orden todas las celdas de Python del libro",
        "Nothing is running." => "No se está ejecutando nada.",
        "Stop the running cell" => "Detener la celda en ejecución",
        "Clear Python's variables and run every cell again" => "Borrar las variables de Python y ejecutar de nuevo todas las celdas",
        "Python Calculation" => "Cálculo de Python",
        "When this workbook's Python cells run" => "Cuándo se ejecutan las celdas de Python de este libro",
        "A Python cell is running." => "Se está ejecutando una celda de Python.",
        "Python is ready." => "Python está listo.",
        "Which Python runs cells" => "Qué Python ejecuta las celdas",
        "Packages" => "Paquetes",
        "What this Python has installed" => "Qué tiene instalado este Python",
        "Check Sandbox" => "Comprobar sandbox",
        "Check Python and the sandbox again, and show the report" => "Comprobar de nuevo Python y el sandbox, y mostrar el informe",
        "What Python\nCan Do" => "Qué puede\nhacer Python",
        "What Python cells can and can't do here" => "Qué pueden y qué no pueden hacer aquí las celdas de Python",
        "This Workbook: Enabled" => "Este libro: habilitado",
        "This Workbook: Disabled" => "Este libro: deshabilitado",
        "Only a person can change this." => "Solo una persona puede cambiar esto.",
        "Run Log" => "Registro de ejecución",
        "Each run's cell, time and printed output" => "Celda, hora y salida impresa de cada ejecución",
        "Clear the Run Log" => "Borrar el registro de ejecución",
        "Problems" => "Problemas",
        "Every Python cell with something to say" => "Todas las celdas de Python con algo que informar",
        "Diagnostics" => "Diagnóstico",
        "uv environment" => "entorno de uv",
        "uv Python" => "Python de uv",
        "virtual environment" => "entorno virtual",
        "conda environment" => "entorno de conda",
        "system" => "sistema",
        "Python cells need a sandbox, and GridCraft has none for this system." => {
            "Las celdas de Python necesitan un sandbox y GridCraft no tiene ninguno para este sistema."
        }
        "The sandbox didn't hold when GridCraft checked it, so Python won't run." => {
            "El sandbox no resistió la comprobación de GridCraft, por lo que Python no se ejecutará."
        }
        "Python is off. Turn it on in Settings to run Python cells." => {
            "Python está desactivado. Actívelo en la configuración para ejecutar celdas de Python."
        }
        "GridCraft can't find Python 3.11 or newer. Set one up with uv, or choose one." => {
            "GridCraft no encuentra Python 3.11 o posterior. Configure uno con uv o elija uno."
        }
        "This Python is too old: Python cells need 3.11 or newer." => {
            "Este Python es demasiado antiguo: las celdas de Python requieren la versión 3.11 o posterior."
        }
        "This is Apple's placeholder for Python, which GridCraft doesn't use. Set up Python with uv, or choose another." => {
            "Este es el marcador de posición de Python de Apple, que GridCraft no usa. Configure Python con uv o elija otro."
        }
        "GridCraft couldn't work out what this Python needs to run." => "GridCraft no pudo determinar qué necesita este Python para ejecutarse.",
        "Python didn't start." => "Python no se inició.",
        "Python isn't on for this workbook, so its cells keep the values saved in the file." => {
            "Python no está activado en este libro, por lo que sus celdas conservan los valores guardados en el archivo."
        }
        "This session doesn't run Python: cells keep the values saved in the file." => {
            "Esta sesión no ejecuta Python: las celdas conservan los valores guardados en el archivo."
        }
        "This workbook imports packages this Python doesn't have." => "Este libro importa paquetes que este Python no tiene.",
        "pandas isn't installed in this Python, so ranges come back as lists of rows." => {
            "pandas no está instalado en este Python, por lo que los rangos se devuelven como listas de filas."
        }
        "The initialization code failed, so Python cells can't run." => {
            "El código de inicialización falló, por lo que las celdas de Python no pueden ejecutarse."
        }
        "This code isn't valid Python." => "Este código no es Python válido.",
        "The code raised an error." => "El código produjo un error.",
        "The code uses a name that isn't defined." => "El código usa un nombre que no está definido.",
        "This name is defined in a later cell. Python cells run in row order, sheet by sheet." => {
            "Este nombre se define en una celda posterior. Las celdas de Python se ejecutan por filas, hoja por hoja."
        }
        "This Python doesn't have that package." => "Este Python no tiene ese paquete.",
        "This package needs a library outside what the sandbox lets Python read." => {
            "Este paquete necesita una biblioteca fuera de lo que el sandbox permite leer a Python."
        }
        "Python cells can't read or write files on your computer. Put the data on a sheet and read it with xl()." => {
            "Las celdas de Python no pueden leer ni escribir archivos de su equipo. Ponga los datos en una hoja y léalos con xl()."
        }
        "Python cells can't use the network. Put the data on a sheet and read it with xl()." => {
            "Las celdas de Python no pueden usar la red. Ponga los datos en una hoja y léalos con xl()."
        }
        "Python cells can't start other programs or processes." => "Las celdas de Python no pueden iniciar otros programas ni procesos.",
        "This picture is too large to draw. Make figsize or dpi smaller." => "Esta imagen es demasiado grande para dibujarla. Reduzca figsize o dpi.",
        "The picture appears in the cell by itself. Python cells can't write files, so this line would fail." => {
            "La imagen aparece sola en la celda. Las celdas de Python no pueden escribir archivos, así que esta línea fallaría."
        }
        "This package draws in a web browser, which Python cells can't open. Use matplotlib or seaborn." => {
            "Este paquete dibuja en un navegador web, que las celdas de Python no pueden abrir. Use matplotlib o seaborn."
        }
        "Python Output" => "Salida de Python",
        "Display Plot over Cells" => "Mostrar gráfico sobre las celdas",
        "Create Reference" => "Crear referencia",
        "Copy Image" => "Copiar imagen",
        "Save Image…" => "Guardar imagen…",
        "The image is saved." => "La imagen se ha guardado.",
        "The image is on the clipboard." => "La imagen está en el portapapeles.",
        "Image" => "Imagen",
        "size" => "tamaño",
        "Resize" => "Cambiar tamaño",
        "This picture shows the last result that cell gave." => "Esta imagen muestra el último resultado que dio esa celda.",
        "Select a Python cell that gave a picture first." => "Seleccione primero una celda de Python que haya devuelto una imagen.",
        "xl() needs its reference written as text in the code, such as xl(\"A1:B5\")." => {
            "xl() necesita su referencia escrita como texto en el código, por ejemplo xl(\"A1:B5\")."
        }
        "This range is too large to send to Python." => "Este rango es demasiado grande para enviarlo a Python.",
        "xl() names something that isn't a cell, range, table or defined name here." => {
            "xl() nombra algo que aquí no es una celda, un rango, una tabla ni un nombre definido."
        }
        "The code ran past the time limit. Python was restarted, so its variables were cleared." => {
            "El código superó el límite de tiempo. Python se reinició, por lo que se borraron sus variables."
        }
        "The code went over the memory limit. Python was restarted, so its variables were cleared." => {
            "El código superó el límite de memoria. Python se reinició, por lo que se borraron sus variables."
        }
        "The result is too large to bring back to the sheet." => "El resultado es demasiado grande para traerlo a la hoja.",
        "The printed output was cut off at its limit." => "La salida impresa se cortó al llegar a su límite.",
        "Python stopped unexpectedly and was restarted, so its variables were cleared." => {
            "Python se detuvo inesperadamente y se reinició, por lo que se borraron sus variables."
        }
        "The result is empty." => "El resultado está vacío.",
        "The result needs cells that aren't empty." => "El resultado necesita celdas que no estén vacías.",
        "The result isn't a number a cell can hold." => "El resultado no es un número que una celda pueda contener.",
        "This date or length of time is outside what a cell can hold." => "Esta fecha o duración está fuera de lo que una celda puede contener.",
        "This cell is running." => "Esta celda se está ejecutando.",
        "What this cell reads has changed since it last ran." => "Lo que lee esta celda ha cambiado desde su última ejecución.",
        "This cell was stopped before it finished." => "Esta celda se detuvo antes de terminar.",
        "xl() needs its reference written as text, such as xl(\"A1:B5\"). A computed reference can't be read." => {
            "xl() necesita su referencia escrita como texto, por ejemplo xl(\"A1:B5\"). No se puede leer una referencia calculada."
        }
        "This module needs the network or other programs, which Python cells can't use." => {
            "Este módulo necesita la red u otros programas, que las celdas de Python no pueden usar."
        }
        "This Python doesn't have this package." => "Este Python no tiene este paquete.",
        "Python cells can't open windows or ask for input." => "Las celdas de Python no pueden abrir ventanas ni pedir datos.",
        "Python cells don't see your environment variables." => "Las celdas de Python no ven sus variables de entorno.",
        "Open Settings" => "Abrir configuración",
        "Show Printed Output" => "Mostrar salida impresa",
        "Hide Printed Output" => "Ocultar salida impresa",
        "Line {line}" => "Línea {line}",
        "Line {line}: {message}" => "Línea {line}: {message}",
        "Python {version}, ready" => "Python {version}, listo",
        "Python {version}, running" => "Python {version}, en ejecución",
        _ => return None,
    })
}
