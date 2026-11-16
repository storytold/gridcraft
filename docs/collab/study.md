# Study: GridCraft — модель документа и пригодность к совместному редактированию

> Артефакт Phase 0. Код фичи не содержит; все утверждения — из чтения репозитория.
> Где число взято из документации upstream, а не измерено мной, это сказано явно.

- Проект: `gridcraft`
- Upstream: `https://github.com/storytold/gridcraft`
- Коммит: `6a8eb4ff` (ветка `collab/study`, ответвлена от `main`)
- Дата: 2026-11-06
- Автор: `gridcraft/envoy`

## 1. Карта репозитория

Workspace, edition 2024, rust-version 1.95. Слои проверяются `cargo xtask layers`:
ниже L6 нет egui/eframe/winit. LOC — Rust-строки вместе с тестами (посчитано `wc -l`,
2026-11-06).

| Crate / модуль | Слой | Назначение | LOC |
|---|---|---|---|
| `core` | L0 | адреса A1/R1C1 (1 048 576 × 16 384), `Value`, `CellError`, даты 1900/1904 | 2 073 |
| `numfmt` | L0 | коды числовых форматов Excel, `TEXT()` | 3 437 |
| `formula` | L1 | лексер, парсер, AST, принтер, `adjust` (пересчёт ссылок при copy/fill/structural) | 3 029 |
| `functions` | L1 | реестр `FnSpec` + реализации; 513 из 523 имён Excel, 504 рабочие (по `docs/function-parity.md`) | 17 245 |
| `math` | L1 | LaTeX-уравнения (RaTeX, pinned) | 1 709 |
| `model` | L2 | `Workbook`, `Sheet`, COW-хранилище ячеек, интернированные стили, names, tables, CF, validation, comments, charts, shapes, images, sparklines, protection | 5 742 |
| `calc` | L2 | вычислитель + граф зависимостей, пересчёт, spills, циклы | 7 034 |
| `xlsx` | L2 | XLSX read/write, CSV/TSV, XLSB/ODS/XLS import, pass-through, PivotTable caches, DrawingML | 32 131 |
| `pdf` | L2 | минимальный PDF 1.7 writer | 1 657 |
| `chart` | L3 | layout графиков в примитивы + растеризация | 5 408 |
| `engine` | L5 | `Session`, `DocState`, undo, clipboard, sort/filter, pivot, what-if, I/O, реестр команд `cmd/*` | 67 398 |
| `ui-egui` | L6 | Excel-подобный UI + control channel | 82 491 |
| `mcp` | L6 | MCP over stdio, headless или bridge к окну | 1 972 |
| `apps/gridcraft` | L7 | desktop (eframe/wgpu) | 2 519 |
| `apps/gridcraft-cli` | L7 | `info/convert/eval/cat/check/run/commands/functions/mcp/send` | 2 899 |
| `apps/gridcraft-web` | L7 | WASM (trunk, wgpu/WebGL2), host-bridge | 866 |

Точки входа:
- GUI: `apps/gridcraft/src/main.rs`;
- WASM: `apps/gridcraft-web/src/{main,web,host}.rs`, сборка `trunk build --release`;
- CLI: `apps/gridcraft-cli/src/main.rs` (`run --cmd 'id={json}'`, `--script file.jsonl`);
- MCP: `gridcraft-cli mcp` (headless) или `--connect PORT` (bridge к запущенному окну);
- control channel десктопа: TCP JSON-lines на `127.0.0.1:<port>` с обязательным токеном.

## 2. Модель документа

Корень — `Workbook` (`crates/model/src/workbook.rs:199`), целиком `Serialize/Deserialize`:

- `sheets: Vec<Arc<Sheet>>` — сами листы в `Arc`, порядок = порядок вкладок;
- `names`, `styles: StyleTable` (интернированные стили, `StyleId` = компактный ключ),
  `theme`, `date_system`, `calc: CalcSettings` (mode, iterative, precision_as_displayed);
- `active_sheet: ActiveSheet(AtomicUsize)` — единственное, что меняется через общий `Arc`
  (это view-state окна, а не содержимое);
- `preserved: Option<Arc<Preserved>>` — непромоделированное содержимое файла;
  `vba_project: Option<Arc<Vec<u8>>>` — VBA-blob как есть; `macros`, `phonetics`, `props`.

`Sheet` (`crates/model/src/sheet.rs:89`) — плоская структура, без дерева:
`cells: CellStore`, `merges`, `cond_formats`, `validations`, `tables`, `autofilter`,
`comments`/`hyperlinks` (BTreeMap по `CellRef`), `charts`, `images`, `shapes`,
`sparklines`, `slicers`, `controls`, `pivots`, `protection`, `edit_ranges`, `print`,
`rows`/`cols: BTreeMap<u32, LineInfo>`, `spill`/`spill_ranges` (производные от calc,
`#[serde(skip)]`), `preserved`.

`Cell` (`crates/model/src/cell.rs:63`): `{ value: Value, formula: Option<Arc<Formula>>,
style: StyleId, py: Option<Arc<PyCode>> }`. Пустая ячейка со стилем по умолчанию не хранится.

Хранилище ячеек (`crates/model/src/store.rs`) — copy-on-write: полосы по 64 строки,
каждая за `Arc<Band>`, внутри `BTreeMap<row, BTreeMap<col, Cell>>`. Клонирование
workbook (undo-снапшот) копирует только указатели полос; правка копирует одну полосу.

**Стабильные ID:**

| Сущность | Есть ли ID | Что это |
|---|---|---|
| ячейка | нет | идентичность = адрес `CellRef` (лист + строка + столбец); адрес меняется при вставке/удалении строк |
| лист | `SheetUid(u64)` — только в рантайме, `#[serde(skip)]`, никогда не пишется | новый лист/копия/чтение = свежий uid; переживает переименование и перемещение |
| лист (в файле) | `file_id: u32` (`sheetId` из файла) | пишется назад при save; 0 у листа, которого в файле не было |
| таблица, график, картинка, фигура, спарклайн, слайсер, control, pivot | `id: u32` на весь workbook | выдаётся `next_object_id()` / `free_object_ids()` — первый свободный выше максимального |
| имя (defined name) | нет | идентичность = текст имени |
| стиль | `StyleId` (интернированный) | не ID узла, а ключ в `StyleTable` |

Ссылки в формулах — по **имени листа** (`SheetSel::Named`) или `Current`; при
переименовании листа формулы переписываются. Ordinal-ссылок на лист в модели нет.

## 3. Команды и мутации

Единственный путь правки — команда: UI, CLI, MCP и control channel зовут
`Session::execute(id, params)` (`crates/engine/src/lib.rs:670`).

- Реестр: `cmd::command_specs()` собирает спеку из 25 модулей `crates/engine/src/cmd/*`
  (file, edit, format, sheet, insert, controls, equation, data, formulas, review, view,
  inspect, extra, macros, draw, spelling, solver, pivot, python, slicer, whatif,
  forecast, analysis, print, stash). `docs/architecture.md` пишет «313 команд»;
  статических регистраций через макрос `cmd!` — 377 (286 undoable, 48 `noundo`,
  43 `query`), плюс генерируемые списки (`data.analysisToolPak`, forecast). Точное число
  даёт `gridcraft-cli commands` (в Phase 0 не запускал: сборки нет).
- `CommandSpec { id, label, menu, shortcut, params, enabled, run, journal, undoable }`.
  `params` — строка-документация JSON-схемы; сами параметры — `serde_json::Value`.
- Классы команд: `query` (`journal: false, undoable: false`) не меняет документ;
  `noundo` журналируется, но не создаёт шаг undo (навигация, выбор листа);
  обычная — журналируется и undoable.
- `Session::execute` ловит панику (`catch_unwind`), восстанавливает workbook, selection,
  undo и redo, закрывает открытые команды группы. Ошибка команды ничего не оставляет.
- Мутация идёт через `Ctx` (`crates/engine/src/cmd.rs:365`):
  `wb: Workbook` (клон `Arc`-снапшота — COW, дёшево), `changed: Vec<Key>`, `structural: bool`,
  `sel`, `fit_rows`, `edits: Vec<(sheet, adjust::Edit)>`. `commit()` применяет
  `refresh_equations`, проверки защиты и массивов, затем пересчёт:
  `calc.cells_changed(&mut wb, &changed)` для обычной правки,
  `calc.recalc_all(&mut wb)` при `structural`.
- **Инверсий нет.** Откат — снапшот предыдущего `Arc<Workbook>`, не обратная команда.
- Часть параметров неявна: `range` по умолчанию = текущее выделение, `sheet` = активный
  лист. Выделение живёт в `DocState.selection` и в окне (`WindowState`), а не в документе.

## 4. Undo/redo

- Стек — на документ: `DocState.undo/redo: Vec<HistoryEntry>`
  (`crates/engine/src/lib.rs:73`), запись = `{ label, wb: Arc<Workbook>, selection,
  retained_bytes }`.
- Лимиты: `UNDO_LIMIT = 100` шагов, `UNDO_BYTES_LIMIT = 128 MiB` удержанного хранилища;
  самый новый шаг остаётся откатываемым, даже если один больше бюджета. Это политика
  истории, а не замер памяти процесса.
- Группы: `Session::grouped(label, f)`, `group_begin/group_end/group_rollback` — макрос,
  скрипт или отлаживаемый шаг = один шаг undo; группа атомарно откатывается при ошибке.
- Undo/redo глобальны для документа (не per-user, не per-range). История не
  сериализуется в файл; `DocState.revision` — счётчик изменений, не версия для merge.

## 5. Сериализация и round-trip

- Читает/пишет `crates/xlsx` — clean-room реализация по ECMA-376/ISO 29500, **только
  байтовые срезы** (без ФС), поэтому собирается под wasm. Точки входа:
  `read_xlsx`, `write_xlsx`, `write_xlsx_as`, `read_csv`/`write_csv`, `read_xlsb`,
  `read_ods`, `read_xls` (BIFF8), `decrypt`.
- Чтение лояльное: битые необязательные части пропускаются с `ReadReport::warnings`;
  ошибка только при отсутствии/порче пакета или workbook-части. Лимиты: 512 MB на часть,
  1 GB на пакет, ячейки вне сетки Excel отбрасываются, ограничена вложенность XML.
- Непромоделированное содержимое не теряется: `Preserved`/`passthrough.rs` хранит
  custom XML, внешние ссылки, connections, query tables, form/ActiveX controls, OLE,
  фон листа, printer settings, `<fileVersion>`. VBA-project хранится blob'ом как есть
  (макросы не исполняются).
- **Границы pass-through:** содержимое, чей граф связей доходит до части, которую GridCraft
  читает и перезаписывает (worksheets, drawings, charts, tables, PivotTables, comments,
  slicers), не сохраняется как pass-through; strict Open XML не проходит pass-through
  (другие namespace); ссылки внутри сохранённых частей (linked cell контрола, диапазон
  query table) **не пересчитываются** при вставке/удалении строк.
- Есть ещё «свой» формат: `FileKind::Json` (`serde_json` для `Workbook`), `.scjson`/`.json`;
  он же основа макросов `json_bytes`. Практически это готовая сериализация модели
  (без `preserved`, `spill`, uid).
- Round-trip тесты: `crates/xlsx/src/tests/roundtrip.rs` (1749 строк, «build a workbook
  using every feature, write, read back, compare»), `passthrough_tests.rs` (1005 строк).
  Открытый риск upstream: `docs/gaps.md` §1 — round-trip не проверен в настоящем Excel,
  возможен repair prompt.
- Что теряется/не моделируется: часть графика (оси, error bars, per-point formatting),
  slicers/timelines/data model при записи, SmartArt/WordArt/3-D, часть `extLst`.
  «Прочитать → записать без правок» сохраняет модель и pass-through, но упрощает
  перечисленное; для макросов, подписей и шифрования поведение отдельное (VBA — blob,
  шифрованный файл открывается с паролем и сохраняется без пароля).

## 6. Структурные правки и их цена

`Edit` (`crates/formula/src/adjust.rs:81`) — шесть видов: `Insert`, `Delete` (строки/столбцы),
`Move` (cut-paste в блок), `InsertCells`, `DeleteCells` (со сдвигом вниз/вправо),
`MoveToSheet`. Структурная команда вызывает `shift_sheet_features` + `rewrite`
(`crates/engine/src/cmd/sheet.rs:125`, `:219`), затем ставит `cx.structural = true` →
**полный пересчёт всей книги** (`calc.recalc_all`).

| Операция | Что правится | Что ломается при конкурентной правке | Цена |
|---|---|---|---|
| вставка/удаление строк/столбцов | `rows`/`cols` LineInfo, `merges`, `cond_formats.ranges`, `edit_ranges`, `validations.ranges`, `tables.range` (+ `shift_table_columns`), `autofilter`, `cell_pictures`, `comments`, `hyperlinks`, `sparklines.cell`, `print.print_area`, anchors графиков/картинок/фигур/слайсеров/контролов (по `AnchorMode`), `pivots.anchor`/`last_range`, **все формулы книги**, источники спарклайнов и pivots | две структурные правки подряд дают разные адреса одних и тех же данных; formula-references надо переписывать в том же порядке; pass-through ссылки не переписываются | полный пересчёт книги, O(формулы + объекты листа) |
| вставка/удаление ячеек со сдвигом | то же, но в пределах колонок/строк диапазона (`InsertCells`/`DeleteCells`) | то же | полный пересчёт |
| таблица: строка/столбец/ячейка | `Table.range`, `columns`, `sync_headers` после каждой правки; вставка без заголовка сдвигает данные вниз | диапазон таблицы и её заголовки меняются одновременно с ячейками | пересчёт зависимых |
| copy/paste, fill, sort, filter | формулы переписываются `adjust`; `check_arrays` запрещает частично затирать spill | вставка в чужой spill/массив отклоняется (`#SPILL!`-инвариант) | пересчёт затронутых |
| стили/CF/validation | `StyleTable` интернирует, CF хранит приоритеты и правила с формулами | приоритет CF и ranges меняются вместе | дёшево |
| pivot refresh | пересчёт pivot-кэша из источника | источник мог сдвинуться структурной правкой другого клиента | O(источник) |
| смена порядка/имени листа, добавить/удалить лист | `Vec<Arc<Sheet>>`, формулы по имени листа переписываются | имя листа — часть адреса в формулах | O(формулы) |

Ключевой факт для протокола: **любая структурная правка — глобальная операция над книгой**
(переписывает формулы на всех листах и сдвигает все объекты листа), а не локальная дельта.

## 7. Существующие хуки для collab

Есть и годится:
- единая точка мутации `Session::execute(id, params)` и headless-вариант
  `execute_headless` (отбрасывает UI-запросы, ошибка вместо молчаливого диалога);
- **журнал команд** `Session.journal: Vec<(String, Json)>` — сами команды и параметры
  без паролей, лимиты 10 000 записей / 64 MB, старые вытесняются;
  `engine.journal` отдаёт его наружу, `automate.runScript`/`macro.run` проигрывают
  списки команд;
- `CellStore::diff(&other) -> Vec<CellRef>` — точный список изменённых ячеек
  (используется dry-run «footprint» макросов: прогон на копии, diff, откат);
- `Ctx.changed` — минимальный набор изменённых ячеек, который команда уже сообщает
  пересчёту (но наружу пока не отдаётся);
- COW-снапшоты: `Arc<Workbook>` в `HistoryEntry` — дешёвый снимок состояния для
  снапшота сессии/отката;
- protection/lock (`SheetProtection`, `check_protection`) и `Trust` — есть готовый слой
  прав, на который можно опереться для viewer;
- control channel и MCP — внешний транспорт команд уже существует;
- host-bridge web-сборки: `gridcraft:ready/open/save/dirty/failed`, `?host=parent&author=`.

Чего нет (надо добавлять):
- шины событий/наблюдателей: `Session` не оповещает о правках, кроме `UiRequest`;
- применения «чужой» операции: нет API «применить команду без журнала и без своего undo»;
- per-user undo и истории операций вне процесса;
- стабильных ID ячеек и листов на время сессии (см. §2);
- сохранения/восстановления `Calc`-графа (пересчёт перестраивается `rebuild`);
- WebSocket/WASM-транспорта (web-сборка — `wasm32-unknown-unknown`, без TCP control channel).

## 8. Производительность

Из `docs/target-app-parity.md` §Performance и `docs/gaps.md` §2 (замеры upstream на машине
владельца; **я сам не мерил** — сборки в Phase 0 нет):

| Мера | Значение |
|---|---|
| потоки пересчёта | 1 (в `crates/calc` нет threading; WASM остаётся однопоточным и по плану) |
| правка → пересчёт, 100k зависимых | ~0.4 s (2026-10-07), цель < 100 ms; после #66 пересчёт диапазонных формул стал линейным, заново не замерялось |
| структурная правка | полный пересчёт книги, не инкрементальный |
| открыть ~100 MB XLSX | не работает (#175, нет потокового чтения) |
| undo | ≤ 100 шагов и ≤ 128 MiB удержанного хранилища |
| журнал команд | ≤ 10 000 записей и ≤ 64 MB |
| лист | 1 048 576 × 16 384; сетка виртуализирована |
| лимиты чтения | 512 MB на часть, 1 GB на пакет |

WASM-ограничения: `wasm32-unknown-unknown` (однопоточно, без ФС); на wasm нет часов
(`GRIDCRAFT_PROFILE` и лимит времени макроса отключены: `cfg!(target_arch = "wasm32")`),
поэтому ограничения там по числу шагов/байт. Web-сборка ~13 MB (≈5 MB gzip), рендер
WebGPU с fallback WebGL2. `crates/xlsx` и `crates/calc` собраны под wasm (байтовые срезы).

## 9. Три варианта протокола (кратко)

| Вариант | Суть | Плюсы | Минусы | Риск |
|---|---|---|---|---|
| A | сервер авторитетно выполняет **команду** `(id, params)` и рассылает подтверждённую команду + дельту изменённых ячеек | использует существующий реестр и журнал; undo сервера = снапшоты; идеально ложится на `execute_headless` | неявные параметры (`range` = выделение) надо нормализовать; volatile-функции дают разный результат на клиентах; структурные команды дороги и должны быть эксклюзивными | средний |
| B | нормализованная **операция над моделью** (sheet + A1 + полезная нагрузка ячейки/структурная интент-операция), сервер применяет и транслирует индексы | дельта мелкая; не зависит от UI-контекста команд; проще per-user undo и presence | нужно написать второй слой мутаций в обход команд и держать его в согласии с `adjust`/`shift_sheet_features`; больше кода | высокий (дублирование логики) |
| C | **CRDT** на хранилище ячеек (LWW на ячейку, sequence-CRDT на порядок листов/объектов) | offline и сходимость без сервера; presence тривиален | формулы, таблицы, pivot, CF и pass-through не сводятся к per-cell CRDT; пересчёт и структурные правки всё равно нужны централизованно | высокий |

Рекомендация и обоснование — в `options.md` (вариант A с нормализованным конвертом
и серверной дельтой).

## 10. Риски и открытые вопросы

1. **Volatile-функции и недетерминизм.** `NOW`, `RAND`, `OFFSET`/`INDIRECT` и Python-ячейки
   дают разный результат при повторном выполнении одной и той же команды на разных
   клиентах. Значит, «выполнить ту же команду у себя» недостаточно: авторитетным должен
   быть результат сервера (дельта значений), а не только команда.
2. **`precision_as_displayed` и A1-адресация.** Округление «как показано» меняет значения
   при применении правки; порядок применения обязан быть серверным.
3. **Структурные правки.** Вставка строки переписывает формулы всей книги и сдвигает все
   объекты листа. Их нельзя перемежать с чужими правками ячеек без явной сериализации.
4. **Pass-through ссылки.** Ссылки внутри сохранённых непромоделированных частей не
   пересчитываются при структурных правках; при конкурентной работе это накапливает
   расхождение с Excel.
5. **Идентичность ячеек.** Адрес — не ID: при вставке строки «та же» ячейка получает новый
   адрес. Per-user undo и presence должны опираться на версии/дельты, а не на адрес.
6. **Undo-снапшоты дороги.** 128 MiB на документ — приемлемо для сервера, но не для
   отправки клиенту; клиенту нужны дельты.
7. **Журнал ограничен** (10 000/64 MB): как источник истины для сессии он не годится,
   нужен отдельный журнал сессии + снапшоты.
8. **Round-trip не проверен в Excel** (`gaps.md` §1): commit может выдать файл, который
   Excel попросит «исправить». Для кабинета это блокер приёмки, не только collab.
9. **Не измерено мной:** время пересчёта, размер снапшота, стоимость `CellStore::diff`
   на большой книге, поведение под wasm на реальном документе. Все цифры выше — из
   документации upstream.

## 11. Что это значит для выбора протокола (вводные для `options.md`)

- Документ единый и COW; дешёвый снапшот есть уже сейчас.
- Любая правка — команда с JSON-параметрами; журнал и проигрывание команд существуют.
- Инверсий нет, откат — снапшоты; per-user undo придётся строить над дельтами.
- Структурные правки глобальны и дороги → эксклюзивны.
- Пересчёт недетерминирован для volatile/Python → авторитетное состояние на сервере.
- Формат — XLSX с pass-through; сервер должен собирать файл тем же `write_xlsx`.
