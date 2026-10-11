# Mensagens de erro do motor de cálculo. Chave: error-<código>; os códigos espelham gridcraft_engine::EngineError.
# Redação original do GridCraft (português de Portugal).

error-unknown-command = Comando desconhecido “{ $command }”.
error-command-disabled = “{ $command }” não está disponível neste momento: { $reason }
error-bad-params = Parâmetros inválidos para “{ $command }”: { $message }
error-no-document = Não está aberto nenhum livro.
error-internal = Erro interno em “{ $command }” (o livro foi mantido como estava): { $message }
error-other = { $message }
error-invalid-formula = Existe um problema com esta fórmula: { $message }
error-invalid-locale = As definições de idioma ou de região não são válidas: { $message }
error-protected-sheet = Esta alteração não é permitida: a célula ou o gráfico está numa folha protegida.
error-encrypted-workbook = “{ $name }” está protegido por palavra-passe. Remova a palavra-passe no Excel e tente novamente; o GridCraft ainda não abre livros encriptados.
error-legacy-workbook = “{ $name }” é um livro do Excel 97–2003 (.xls) ou outro ficheiro binário antigo. Guarde-o como .xlsx no Excel e tente novamente.
error-import-failed = Não foi possível importar “{ $name }”: { $message }
error-import-only-format = { $format } permite apenas a importação de dados. Guarde como .xlsx ou outro formato de exportação suportado.
error-text-box-too-long = As caixas de texto podem conter no máximo { $limit } carateres.
error-chart-range-too-large = O intervalo de origem é demasiado grande para um gráfico.
