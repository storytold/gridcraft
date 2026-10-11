# Mensajes de error del motor. Clave: error-<código>; los códigos reflejan gridcraft_engine::EngineError.
# Redacción original de GridCraft en español de España; no copiada de documentación de terceros.

error-unknown-command = Comando desconocido «{ $command }».
error-command-disabled = «{ $command }» no está disponible en este momento: { $reason }
error-bad-params = Parámetros no válidos para «{ $command }»: { $message }
error-no-document = No hay ningún libro abierto.
error-internal = Error interno en «{ $command }» (el libro se ha conservado tal como estaba): { $message }
error-other = { $message }
error-invalid-formula = Hay un problema con esta fórmula: { $message }
error-invalid-locale = La configuración de idioma o de región no es válida: { $message }
error-protected-sheet = Este cambio no está permitido: la celda o el gráfico está en una hoja protegida.
error-encrypted-workbook = «{ $name }» está protegido con contraseña. Quite la contraseña en Excel y vuelva a intentarlo; GridCraft todavía no abre libros cifrados.
error-legacy-workbook = «{ $name }» es un libro de Excel 97–2003 (.xls) u otro archivo binario antiguo. Guárdelo como .xlsx en Excel y vuelva a intentarlo.
error-import-failed = No se puede importar «{ $name }»: { $message }
error-import-only-format = { $format } solo permite importar datos. Guarde como .xlsx u otro formato de exportación compatible.
error-text-box-too-long = Los cuadros de texto admiten como máximo { $limit } caracteres.
error-chart-range-too-large = El rango de origen es demasiado grande para un gráfico.
