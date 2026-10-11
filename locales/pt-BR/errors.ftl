# Mensagens de erro do mecanismo. Chave: error-<código>; os códigos espelham gridcraft_engine::EngineError.
# Redação original do GridCraft (português do Brasil).

error-unknown-command = Comando desconhecido “{ $command }”.
error-command-disabled = “{ $command }” não está disponível no momento: { $reason }
error-bad-params = Parâmetros inválidos para “{ $command }”: { $message }
error-no-document = Nenhuma pasta de trabalho está aberta.
error-internal = Erro interno em “{ $command }” (a pasta de trabalho foi mantida como estava): { $message }
error-other = { $message }
error-invalid-formula = Há um problema com esta fórmula: { $message }
error-invalid-locale = As configurações de idioma ou de região não são válidas: { $message }
error-protected-sheet = Esta alteração não é permitida: a célula ou o gráfico está em uma planilha protegida.
error-encrypted-workbook = “{ $name }” está protegido por senha. Remova a senha no Excel e tente novamente; o GridCraft ainda não abre pastas de trabalho criptografadas.
error-legacy-workbook = “{ $name }” é uma pasta de trabalho do Excel 97–2003 (.xls) ou outro arquivo binário antigo. Salve como .xlsx no Excel e tente novamente.
error-import-failed = Não foi possível importar “{ $name }”: { $message }
error-import-only-format = { $format } permite apenas a importação de dados. Salve como .xlsx ou outro formato de exportação compatível.
error-text-box-too-long = As caixas de texto podem conter no máximo { $limit } caracteres.
error-chart-range-too-large = O intervalo de origem é grande demais para um gráfico.
