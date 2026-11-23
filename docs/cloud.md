# Cloud storage

GridCraft takes Open and Save against cloud storage from `craft-ui`'s shared `cloud` feature.
The same feature covers sharing a workbook. GridCraft does not ship a provider.

## The contract

`craft_ui::cloud` carries the shared half. `CloudBackend` is a trait a host application
implements. It runs `Job`s (`Account`, `List`, `Read`, `Write`, `Create`) on a worker of its own.
It answers with `Event`s. Nothing in `craft-ui` opens a socket.

`CloudFiles` holds the account. It holds the folders being browsed. It holds a transfer in
flight. It holds each open workbook's cloud origin. `SheetApp` holds one of these as `cloud`.
The Open and Save As Backstage pages are `craft-ui`'s own widgets. So is the title bar's account
badge. So is the conflict modal. All three draw over `CloudFiles`.

`crates/ui-egui/src/cloud.rs` is GridCraft's half. It is a `CloudApp` implementation. It says
what a GridCraft document is (`DocumentKind`: extensions and content types). It says how a
document's bytes become one and back (`open_bytes`, `encode`). It says which icon a file row
draws. It holds no provider credential. It speaks no provider protocol.

A second, separate contract covers sharing. `craft_ui::share` carries it. `ShareBackend` is its
trait. `ShareDialog` is `CloudFiles`'s sibling; `SheetApp` holds one as `sharing`. A document's
overflow menu opens it. So does the title bar's Share control. `ShareDialog::available` says
whether the connected backend's provider supports sharing at all. `ShareDialog::show` runs every
frame regardless (`SheetApp::ui`). That is what lets it collect a connected backend's answers.
An actual dialog still waits for `ShareDialog::open` before it draws.

`craft_ui::presence::PresenceBar` is a third, unrelated thing GridCraft holds. `SheetApp` holds
one as `presence`, only because `CloudApp::presence` is a required method. Nothing populates it.
A live multi-person editing session needs a source of peer presence. That surface belongs to
RFC-0020. `craft-cloud` states none of it. `craft_ui::cloud::who`
already draws nothing for an empty presence bar. It is always empty here. The field costs
nothing beyond its own memory.

## No backend is connected

`apps/gridcraft/src/main.rs` and `apps/gridcraft-web/src/web.rs` each carry a comment at the
point where a `CloudBackend` would be connected. A `ShareBackend` belongs beside it, the comment
says. Neither app connects either one today.

With none connected, `cloud::available` is `false`. The Open and Save As pages draw nothing
cloud-shaped. The account badge draws nothing. The Share control draws nothing.

What each `cloud.*` command does without one is not uniform. The precise behavior lives in
`crates/ui-egui/src/app/commands.rs`. What follows is a summary.

`cloud.status` never refuses. It reports `available: false`. It reports an empty folder, account
and origin. That is a legitimate answer. It is not an error.

`cloud.open` checks availability first (see `crates/ui-egui/src/app/commands.rs` for exactly
where). So does `cloud.browse`. So does `cloud.openFile`. So does `cloud.saveAs`. Each refuses
with `cloud::UNAVAILABLE` ("no cloud service is configured") when that check fails.

`cloud.save` carries no such check of its own. `craft_ui::cloud::save` refuses with "no workbook
open" when nothing is active. It refuses with "this workbook did not come from the cloud" when
the active one has no cloud origin. Nothing can ever record an origin with no backend connected.
That second refusal is what `cloud.save` always gives today. The same refusal is also correct
later, for an ordinary local workbook, once a backend does exist.

`file.save` on a workbook with no local file and a cloud origin redirects to `cloud::save` (see
`crates/ui-egui/src/app/commands.rs`). A workbook with no cloud origin behaves exactly as it
always did. It asks where to save, through the ordinary Save As dialog request.

## The storytold pin

`craft-ui` and `craft-cloud` are both pinned to the storytold `craft-libs` revision that merged
`storytold/craft-libs#160`. That is where `cloud` landed in `craft-ui` (the `cloud` feature also
pulls in `share`). See `crates/ui-egui/Cargo.toml` and the root `Cargo.toml`. No `wdunn001` source
remains in this workspace's `Cargo.lock`.

That revision is newer than the one most other craft-libs crates in this workspace use.
craft-ui and craft-cloud moved on their own, for this one feature. They did not wait for every
craft-libs dependency to bump together. The comments beside each pin in both `Cargo.toml` files
say which crates share which revision.

## What this does not include

Real-time shared editing is not part of this. Two people editing one workbook at once, with a
presence bar showing who else is here, is what that means. RFC-0020 states that surface. It states it
separately from the documents contract this branch implements. The contracts crate it calls for
is not open yet. An application therefore has nothing to implement against. Shared
editing stays out of this branch until there is. `PresenceBar` stays empty for the same reason;
see above.

## Where the code is

| What | Where |
|---|---|
| The `CloudApp` implementation (`open_bytes`, `encode`, `DocumentKind`) | `crates/ui-egui/src/cloud.rs` |
| The `CloudBackend` / `Job` / `Event` / `CloudFiles` contract itself | `craft_ui::cloud` (storytold/craft-libs) |
| The `ShareBackend` / `ShareDialog` contract | `craft_ui::share` (storytold/craft-libs) |
| Where a backend would be connected, once an app has one | `apps/gridcraft/src/main.rs`, `apps/gridcraft-web/src/web.rs` |
| `cloud.*` control-channel commands | `crates/ui-egui/src/app/commands.rs`, catalog in `crates/ui-egui/src/control.rs` |
| The in-process mock backends the tests drive the real interface through | `crates/ui-egui/tests/cloud_files.rs` |

## Driving it

`cloud.open` runs through the control channel and MCP like any other command. So does
`cloud.browse`. So does `cloud.openFile`. So does `cloud.save`. So does `cloud.saveAs`. So does
`cloud.status`. See [control-protocol.md](control-protocol.md).
