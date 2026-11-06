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
| `GRIDCRAFT_COLLAB_ADDR` | нет | `127.0.0.1:8787` | адрес слушателя |
| `GRIDCRAFT_COLLAB_PUBLIC_URL` | нет | `ws://{ADDR}` | база `wsUrl` в ответе кабинета |
| `GRIDCRAFT_COLLAB_AUDIENCE` | нет | `office-sync` | `aud` токена сессии |
| `GRIDCRAFT_COLLAB_APP` | нет | `gridcraft` | приложение, которое обслуживает адаптер |
| `GRIDCRAFT_COLLAB_LEASE_SECS` | нет | `1800` | TTL lease файла |
| `GRIDCRAFT_COLLAB_HELLO_SECS` | нет | `10` | сколько ждать `hello` |
| `GRIDCRAFT_COLLAB_MAX_PARTICIPANTS` | нет | `5` | участников на документ (v1) |

Лимиты транспорта: входящий кадр ≤ 1 MiB, сообщение ≤ 9 MiB (`applied` ≤ 8 MiB по RFC §4.1).

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

Дальше — сообщения `op`, `presence`, `ping`, `leave` из `rfc.md` §4.2. `applied` и `presence`
рассылаются всем соединениям документа (`presence` — кроме автора), `rejected`/`pong`/`error`
получает только отправитель. Каждое сообщение соединения продлевает lease файла.

Соединение с медленным клиентом теряет кадры (outbox 256): клиент видит разрыв версии и
делает `resync`, как при reconnect.

## Healthcheck и метрики

- `GET /healthz` → `200 ok`;
- `GET /readyz` → `200 ready`;
- `GET /metrics` → Prometheus: `gridcraft_collab_sessions_open`, `participants`, `leases`,
  `connections`, `ops_total`, `applied_total`, `commits_total`, `auth_failures_total`,
  `dropped_frames_total`, `rejections_total{code}`.

Логи содержат только `fileId`, `sessionId`, `actorId`, версию и код — ни значений ячеек, ни
токенов.

## Чего ещё нет

- **Журнал и снапшоты** (PR 4c): документы живут в памяти процесса; после рестарта сессии
  теряются. `resync` сейчас отвечает `error{code:"resync"}` — клиент переоткрывает книгу.
- **TLS** терминируется на входном прокси (RFC §4.1); бинарь слушает HTTP/WS.
- **Метрики латентности `op→applied`** и `reconnect rate` — вместе с журналом.
- **Host-сообщения кабинета** и клиентская часть — PR 5.

## Проверка

```bash
cargo test -p gridcraft-collab-server      # unit + 6 сценариев через сокет
cargo clippy -p gridcraft-collab-server --all-targets -- -D warnings
cargo xtask layers
```
