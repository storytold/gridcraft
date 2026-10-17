# Conexões ODBC: PostgreSQL como primeiro banco

Data: 2026-10-13. Estado em 2026-10-15: implementação e revisão concluídas;
as oito etapas de `cargo xtask ci` passaram. Integração com driver psqlODBC real pendente.
Base examinada: GridCraft 0.6.1, commit `84acf62`.

> Este documento registra o desenho inicial, com PostgreSQL como primeiro adaptador. O estado
> atual, as seis famílias suportadas e o fluxo para contribuir estão em [`odbc.md`](../../odbc.md).

## Objetivo e escopo

O usuário quer buscar dados de bancos por ODBC, com uma experiência semelhante à
importação de dados do Excel, e escolheu PostgreSQL como primeiro banco.

A primeira entrega será funcional no Windows desktop e na CLI, usando o driver
psqlODBC instalado pelo usuário. Ela permitirá escolher um DSN ou informar uma
string de conexão, executar SQL, visualizar uma amostra e carregar os resultados
como tabela em uma nova folha. A consulta poderá ser atualizada manualmente.

Windows é a plataforma inicial assumida a partir do ambiente atual. O conector
será separado da interface para permitir expansão a outros sistemas nativos;
builds existentes de Linux, macOS, BSD e WASM devem continuar funcionando sem
exigir um gerenciador ODBC adicional. O navegador poderá abrir e editar os
resultados salvos, mas não conectar diretamente a drivers ODBC locais.

Esta entrega cobre consulta SQL e atualização manual. Navegação de catálogo,
editor de transformações Power Query, agendamento, edição de dados no banco e
compatibilidade das definições de consulta com o refresh do Excel ficam fora do
primeiro escopo.

## Abordagens consideradas

1. **ODBC nativo com `odbc-api` — recomendada.** Atende ao pedido e permite
   DSN e strings de conexão. Requer driver compatível com a arquitetura do
   aplicativo; usa a API segura da biblioteca, sem `unsafe` no GridCraft.
2. **Driver PostgreSQL nativo.** Reduz dependências do sistema, mas implementa
   uma integração específica de PostgreSQL e não atende à interface ODBC pedida.
3. **Serviço intermediário.** Permitiria acesso pelo navegador, mas acrescentaria
   servidor, implantação e autenticação à contribuição.

## Fluxo do usuário

- Dados > Obter Dados > ODBC abre o diálogo do conector.
- O diálogo oferece DSNs disponíveis, uma opção de string de conexão, usuário,
  senha, SQL e nome da folha de destino. Credenciais têm campos próprios.
- Testar conexão verifica o acesso sem executar a consulta do usuário.
- Visualizar mostra até 100 linhas, nomes e tipos das colunas. A indicação de
  amostra é explícita e não representa a importação completa.
- Carregar busca o resultado completo dentro dos limites e cria uma nova folha
  com uma tabela, cabeçalhos únicos e dados literais.
- Consultas e Conexões permite selecionar a tabela, revisar SQL e atualizar.
- Atualizar Tudo atualiza fontes ODBC antes das tabelas dinâmicas existentes,
  preservando a funcionalidade atual e relatando falhas por fonte.

Operações demoram em um worker; a interface mostra estado e permanece utilizável.
Cancelar descarta o resultado e impede sua aplicação. Não será apresentado como
garantia de interrupção instantânea do driver ou do servidor.

## Arquitetura

### Conector e execução

Uma crate `gridcraft-odbc` contém a integração nativa e depende apenas de tipos
portáteis de dados, sem egui. A tabela de camadas em `xtask` será atualizada.
Dependências ODBC serão condicionais para não entrar no build WASM nem alterar
os requisitos das plataformas ainda não habilitadas.

O conector executa descoberta, teste e consulta; retorna metadados e valores
tipados com erros estruturados. O engine mantém o estado dos trabalhos, a
referência ao documento e a aplicação transacional dos resultados. UI, CLI e
MCP usam comandos do engine; chamadas programáticas nunca abrem diálogos.

Os workers não modificam o workbook. Resultados prontos são aplicados por um
comando separado do engine, garantindo validação e histórico também na CLI e no
MCP. Panics escapados no worker são convertidos em falha do trabalho; essa
proteção não promete recuperar falhas internas de um driver nativo.

As consultas capturam identidade e revisão do documento e identidade da tabela
de destino. Antes de aplicar resultados, o engine confirma que esses alvos
continuam válidos. Fechar ou editar o destino durante uma consulta não permite
aplicar o resultado em outro documento ou sobrescrever mudanças posteriores.

### Modelo e arquivos

A definição da fonte fica associada à tabela, com campo opcional e `serde`
compatível com arquivos anteriores. Contém SQL, limites, referência ao DSN ou
perfil de conexão e uma impressão dos dados da última carga para reconhecer
edições locais. Renomear ou mover a tabela mantém essa associação; remover a
tabela remove sua fonte. Copiar a folha mantém a definição sem disparar acesso
ao banco.

XLSX guarda os dados como uma tabela comum e a definição sem segredos em uma
extensão identificada do GridCraft. JSON também guarda a definição. Abrir um
arquivo nunca executa SQL automaticamente. Excel poderá consumir os valores
salvos; atualizar essa consulta no próprio Excel não é prometido nesta entrega.

Para DSN, o arquivo guarda o nome do DSN. Strings de conexão livres e
credenciais ficam somente na sessão; a extensão guarda uma referência, e após
reabrir o usuário informa a conexão novamente. Não se tentará persistir strings
arbitrárias removendo apenas algumas chaves de senha.

## Contrato inicial dos comandos

Os nomes seguem o padrão existente de comandos com parâmetros JSON.

| Comando | Entrada | Resultado |
|---|---|---|
| `data.odbcSources` | `{}` | Trabalho de descoberta de drivers e DSNs |
| `data.odbcTest` | DSN ou string de conexão, credenciais opcionais | Trabalho de teste de conexão |
| `data.odbcPreview` | Conexão, SQL e limites | Trabalho com metadados e até 100 linhas |
| `data.fromOdbc` | Conexão, SQL, nome opcional da folha e limites | Trabalho que importa como nova tabela |
| `data.refreshQuery` | Nome da tabela e credenciais opcionais | Trabalho de atualização da tabela |
| `data.odbcStatus` | Identificador do trabalho | Estado, contagens, avisos ou erro |
| `data.odbcApply` | Identificador do trabalho pronto | Aplica dados, definição e tabela como uma edição desfazível |
| `data.odbcCancel` | Identificador do trabalho | Cancelamento da aplicação do resultado |

DSN e string de conexão são mutuamente exclusivos. Parâmetros ausentes,
tipos inválidos e limites inválidos são rejeitados antes de iniciar o trabalho.
Estados: `pending`, `running`, `ready`, `completed`, `failed`, `cancelled`.
Descoberta, teste e preview terminam sem editar a planilha. Carga e refresh
passam por `ready` antes de `data.odbcApply`; aplicar novamente é rejeitado.
Resultados de consulta não são repetidos indefinidamente nos polls de estado;
trabalhos terminados são mantidos em uma coleção limitada.

`data.queriesConnections` continua listando tabelas e acrescenta informação de
fontes conectadas. `data.refreshAll` preserva os campos atuais `refreshed` e
`errors`; acrescenta a identificação do trabalho quando envolve ODBC. O refresh
em lote coleta os resultados e aplica as fontes bem-sucedidas juntas, mantendo
as fontes que falharam, antes de recalcular as tabelas dinâmicas. A CLI acompanha
e aplica os trabalhos antes de salvar a saída ou encerrar o processo. O diálogo
faz o mesmo; clientes MCP podem acompanhar e aplicar pelos comandos documentados.

## Fidelidade, falhas e limites

- `NULL` vira célula vazia; booleanos continuam booleanos; textos entram como
  `Value::Text`, inclusive `=SUM(...)`, códigos com zeros e textos parecidos com
  datas. Não usar o caminho atual de `range.setValues` que interpreta fórmulas.
- Números representáveis permanecem numéricos. Inteiros ou decimais que perderiam
  precisão na representação da planilha permanecem texto, com aviso resumido.
- Datas e timestamps entram inicialmente como texto ISO, preservando precisão e
  fuso quando disponíveis. Tipos não suportados geram erro ou conversão explícita
  documentada; não serão descartados silenciosamente.
- Limites iniciais: 100 mil linhas, 256 colunas, 128 MiB de dados materializados
  e 1 MiB por célula. Também respeitar o tamanho da grade e aritmética verificada.
  Ultrapassar um limite na carga completa falha antes de modificar a planilha.
- Login pede timeout de 15 segundos; execução pede 60 segundos. O driver pode
  limitar ou não oferecer timeout; o worker e o cancelamento não dependem de
  prometer que todo driver cumprirá esses valores.
- Importação e atualização são uma única edição desfazível. Falhas mantêm
  planilha, fonte e seleção anteriores. Uma atualização menor remove valores
  antigos somente da região pertencente à consulta.
- Atualizar com esquema diferente, região protegida, fórmulas adicionadas ao
  corpo importado ou expansão sobre dados vizinhos retorna erro, preservando a
  tabela anterior. Mudanças locais de dados da tabela precisam de confirmação
  explícita de substituição antes do refresh.
- A primeira importação aceita resultado com zero linhas e cria uma tabela com
  cabeçalhos, sem inventar uma linha de dados. Consulta sem result set é erro.

## Credenciais e acesso

Senha e string de conexão não entram no journal, scripts gravados, inspecções,
estado dos trabalhos, preferências, JSON ou XLSX. A filtragem atual do journal
só conhece `password` e chaves terminadas em `Password`; deve cobrir também os
novos parâmetros secretos, inclusive objetos aninhados.

Erros expostos ao usuário incluem categoria e SQLSTATE quando disponíveis;
diagnósticos que possam conter segredos precisam ser saneados ou substituídos.
Também revisar o logger da dependência, pois a biblioteca registra diagnósticos.
Credenciais são eliminadas do estado de sessão ao fechar a fonte ou documento.

O recurso tem finalidade de leitura. A validação PostgreSQL usa uma conta com
permissões de leitura. Um teste lexical de SQL ou ausência de result set não
será tratado como garantia de impedir escritas. Abrir arquivos e repetir
consultas por refresh sempre são ações distintas.

## Validação para aceitar a implementação

Testes sem banco cobrem conversões, texto literal, segredos em journal e erros,
parâmetros, limites, fontes, cancelamento e resultados tardios. Testes de engine
cobrem importação atômica, undo/redo, refresh menor, esquema alterado, proteção,
conflitos de destino, remoção e cópia de fontes. Testes de arquivos cobrem
XLSX/JSON com definições sem credenciais e leitura de arquivos antigos.

Testes UI cobrem diálogo, estados, carga e erros. A integração real opt-in com
PostgreSQL/psqlODBC deve verificar Unicode, NULL, booleanos, precisão, zero
linhas, autenticação recusada e mudança de dados entre carga e refresh. Usar
banco de teste e credenciais fora do repositório.

Executar os testes afetados, `cargo xtask layers`, build WASM e `cargo xtask ci`
antes de preparar uma contribuição. Atualizar documentação de uso, arquitetura,
gaps, roadmap e checklist gerado. A abertura no Excel deve ser verificada antes
de afirmar fidelidade externa da extensão XLSX.

## Referências verificadas

- Fluxos existentes: `crates/engine/src/cmd/extra.rs`, `cmd/pivot.rs`,
  `cmd/edit.rs`, `crates/ui-egui/src/ribbon.rs` e `dialogs.rs`.
- [Conector ODBC do Power Query](https://learn.microsoft.com/en-us/power-query/connectors/odbc):
  DSN, string de conexão, credenciais e SQL.
- [odbc-api](https://github.com/pacman82/odbc-api): suporte a PostgreSQL,
  dependência do gerenciador ODBC e licença MIT.
- [API de conexão](https://docs.rs/odbc-api/latest/odbc_api/struct.Connection.html):
  execução, metadados e limites dos timeouts por driver.
- [psqlODBC](https://odbc.postgresql.org/): driver oficial PostgreSQL ODBC.

## Premissas que a revisão deve confirmar

O primeiro alvo é Windows com PostgreSQL, consulta SQL informada pelo usuário e
atualização manual. Não foi informado um DSN ou banco de teste disponível. O
próximo passo após aprovar este desenho é detalhar o plano de implementação e
validar as dependências de compilação e o ambiente de testes.
