# GridCraft collaboration server

Бинарь `apps/gridcraft-collab-server` — процесс вокруг протокола из `rfc.md` §9: он держит
`Gateway`/`Hub` из `crates/collab`, проверяет подпись токена кабинета, ведёт lease файла и
отдаёт healthcheck и метрики. Протокол, allowlist и дельта живут в `crates/collab` и
тестируются без сокета; здесь только транспорт и жизненный цикл сессии.

## Конфигурация

Всё читается из окружения один раз при старте. Секреты не печатаются и не попадают в логи.

| Переменная | Обязательна | По умолчанию | Смысл |
|---|---|---|---|
| `GRIDCRAFT_COLLAB_JWT_SECRET` | да, если нет PEM | — | общий секрет, HS256 (локальные прогоны) |
| `GRIDCRAFT_COLLAB_JWT_PEM` | да, если нет секрета | — | публичный ключ кабинета в PEM |
| `GRIDCRAFT_COLLAB_JWT_ALG` | нет | `EdDSA` | `ES256`, `ES384` или `EdDSA` для PEM |
| `GRIDCRAFT_COLLAB_SERVICE_TOKEN` | **да** | — | Bearer для `/v1/*`; без него сервер не стартует |
| `GRIDCRAFT_COLLAB_STORE` | **да** | — | каталог состояния (журнал, снапшоты); без него сервер не стартует |
| `GRIDCRAFT_COLLAB_ADDR` | нет | `127.0.0.1:8787` | адрес слушателя |
| `GRIDCRAFT_COLLAB_PUBLIC_URL` | нет | `ws://{ADDR}` | база `wsUrl` в ответе кабинета |
| `GRIDCRAFT_COLLAB_AUDIENCE` | нет | `office-sync` | `aud` токена сессии |
| `GRIDCRAFT_COLLAB_APP` | нет | `gridcraft` | приложение, которое обслуживает адаптер |
| `GRIDCRAFT_COLLAB_LEASE_SECS` | нет | `1800` | TTL lease файла |
| `GRIDCRAFT_COLLAB_HELLO_SECS` | нет | `10` | сколько ждать `hello` |
| `GRIDCRAFT_COLLAB_MAX_PARTICIPANTS` | нет | `5` | участников на документ (v1) |
| `GRIDCRAFT_COLLAB_SNAPSHOT_OPS` | нет | `500` | операций между снапшотами |
| `GRIDCRAFT_COLLAB_SNAPSHOT_SECS` | нет | `300` | период фонового снапшота по времени |

Лимиты транспорта: входящий кадр ≤ 1 MiB, сообщение ≤ 9 MiB; образ структурного
undo/redo (`{book, images}`: листы, preserved-блоки листов, defined names и защита
структуры) уходит частями `applied{more:true}` по 512 КиБ до base64 (`imagePart`), кадр
остаётся < 1 MiB (ADR-0013); чанк снапшота 768 КиБ до base64.

## Хранилище

```text
$GRIDCRAFT_COLLAB_STORE/sessions/<fileId>/
  base.xlsx       авторитетная книга на версии base_version
  keys.json       ключи листов {next, live:[(key, имя)], dead:[key]}
  seen.json       {version, epoch, окно дедупа} на base_version
  journal.jsonl   применённые версии после base_version: операции, undo и redo
  meta.json       fileId, sessionId, owner, name, version, epoch, base_version
```

- `base.xlsx` — тот же файл, что отдаёт `commit`: pass-through-части и кэш значений формул
  сохраняются ридером/писателем, второго сериализатора нет.
- Ключи листов привязаны к имени: `SheetUid` не переживает рестарт, имена уникальны, а
  переименование попадает в следующий снапшот; исчезнувшее имя — надгробие.
- Снапшот: при открытии, каждые `SNAPSHOT_OPS` операций, каждые `SNAPSHOT_SECS`
  секунд (фоновый тик; комнату без новых операций не трогает), на graceful shutdown.
  Запись атомарная (tmp + rename), идёт в `spawn_blocking` с COW-копией книги; после
  успеха журнал обрезается, операции во время записи остаются.
- Рестарт: `restore()` читает каталоги, грузит базу через `DocState::from_saved`, применяет
  хвост: запись журнала говорит, что дало версию — клиентская операция (`"kind":"op"`,
  проигрывается через `Room::apply`) или undo/redo (`"kind":"undo"`/`"redo"`, проигрывается из
  собственного ответа через `Room::replay_reversal`). Номера версий хвоста обязаны идти подряд
  от `base_version + 1`; `meta.version` — голова на момент последнего снапшота, поэтому после
  аварии без flush он отстаёт, и это нормально: сверка идёт с концом хвоста, а не с `meta`.
  История undo/redo в том не участвует (ADR-0010 §6), и после рестарта стеки пусты
  (`Room::forget_history`): документ верный, но шаги не отменяются второй раз.
  Повреждённый каталог логируется и пропускается.
- Запись хвоста: структурный undo/redo несёт в `applied` образ листов, поэтому его строка
  журнала крупная (те же байты, что уходят на провод частями `imagePart`); снапшот по
  счётчику операций, по таймеру и на shutdown обрезает журнал по своей версии.
- `DELETE /v1/sessions/{id}` удаляет каталог; takeover продолжает **сохранённую** сессию,
  а не присланные кабинетом байты.

## HTTP

Все `/v1` вызовы требуют `Authorization: Bearer <GRIDCRAFT_COLLAB_SERVICE_TOKEN>`; сравнение
идёт по SHA-256, не по строке. Ошибка — `{code, message}` с кодом `invalid`, `lease`,
`unauthorized`, `forbidden`, `conflict`, `not-found`, `internal`. Токен пользователя по HTTP
не ходит.

```text
POST   /v1/sessions            {fileId, app, owner, initialName?, initialBytes?, takeover?}
                               → 201 {sessionId, wsUrl, expiresAt}
GET    /v1/sessions/{id}       → {sessionId, fileId, app, owner, name, state, version, epoch,
                                  participants[], createdAt, updatedAt}
POST   /v1/sessions/{id}/commit?expectedVersion=N
                               → {name, bytes(base64), sha256, version, warnings[]}
DELETE /v1/sessions/{id}       → 204
GET    /v1/sessions/{id}/ws    WebSocket
```

- `initialBytes` — base64 исходного `.xlsx`/`.csv`; без него открывается пустая книга (тесты и
  локальные прогоны). Всё открытое помечается `Trust::FromFile`: активный контент на сервере
  не исполняется (RFC §12).
- `409 lease` — файл уже открыт в другой сессии. `takeover: true` (кабинет, владелец) закрывает
  старую комнату и открывает новую; старые соединения получают `error{code:"session-closed"}`.
- `commit` собирает файл из авторитетной книги сервера, не из копии клиента (ADR-0009);
  `expectedVersion` ≠ head → `409 conflict`.

## WebSocket

Первое сообщение — `hello` с подписанным токеном:

```json
{ "type": "hello", "token": "<jwt>", "name": "Ann", "lastVersion": 0 }
```

Проверка идёт в этом порядке: подпись и `exp`/`aud` (`jsonwebtoken`) → `fileId`, `app`,
`sessionId`, `role` из claims против сессии и роли из ACL → lease → лимит участников. Только
после этого соединение получает `welcome` и попадает в рассылку. Коды отказа — в `error.code`
и в причине close-кадра: `token-invalid`, `token-expired`, `token-audience`, `token-app`,
`token-file`, `token-session`, `lease`, `too-many`, `session-unknown`, `hello-timeout`.

Догон: если `lastVersion` меньше head, сервер сам снимает соединение с рассылки, шлёт хвост
журнала (если он покрывает версию) или `snapshot{mode:"base"}` чанками, затем `resynced`;
после этого соединение возвращается в рассылку. Тот же путь — по `resync{from}`.

Дальше — сообщения `op`, `presence`, `ping`, `leave` из `rfc.md` §4.2. `applied` и `presence`
рассылаются всем соединениям документа (`presence` — кроме автора), `rejected`/`pong`/`error`
получает только отправитель. Каждое сообщение соединения продлевает lease файла.

Соединение с медленным клиентом теряет кадры (outbox 256): клиент видит разрыв версии и
делает `resync`, как при reconnect.

## Healthcheck и метрики

- `GET /healthz` → `200 ok`;
- `GET /readyz` → `200 ready`;
- `GET /metrics` → Prometheus:
  - счётчики: `ops_total`, `applied_total`, `commits_total`, `snapshots_total`,
    `resumes_total` (соединения, вернувшиеся с известной версией — reconnect rate),
    `catchups_total` (догон по хвосту журнала), `resync_snapshots_total` (догон по книге),
    `auth_failures_total`, `dropped_frames_total`, `rejections_total{code}`;
  - гистограмма `op_latency_seconds` — от кадра `op` до отправленного `applied`
    (бакеты 1–1000 мс, `_sum`/`_count`);
  - гейджи: `sessions_open`, `participants`, `leases`, `connections`, `journal_ops`
    (операций в памяти), `journal_bytes` и `base_bytes` (размер файлов на томе),
    `last_snapshot_timestamp_seconds`.

Латентность — это время сервера, а не общий RTT клиента; reconnect rate считается как
`rate(resumes_total)` при скрейпе.

Логи содержат только `fileId`, `sessionId`, `actorId`, версию и код — ни значений ячеек, ни
токенов.

## Чего ещё нет

- **TLS** терминируется на входном прокси (RFC §4.1); бинарь слушает HTTP/WS.
- **Коммит в кабинет** через server-to-server callback: HTTP `commit` есть, исходящего
  вызова кабинета пока нет.
- **Отмена структурных правок** — v2 (undo и redo merge-шагов есть).
- **Логгер в бинаре** не инициализирован: `log::*` из библиотеки сейчас уходит в никуда;
  метрики и `/healthz` это не затрагивает.

Клиент: `docs/collab/client.md` (feature `collab`, `GRIDCRAFT_COLLAB_URL`/`_TOKEN`).

## Проверка

```bash
cargo test -p gridcraft-collab-server      # unit + 6 socket + 3 восстановление + 5 приёмка
cargo clippy -p gridcraft-collab-server --all-targets -- -D warnings
cargo xtask layers
```
