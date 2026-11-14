# Optional external file providers — GridCraft

The web editor exposes a small trusted same-origin capability. **It is disconnected and
inactive by default.** Serving the normal build, a query parameter, a stored preference or
receiving an unsolicited message does not activate it. Desktop, CLI and engine/control-channel
file commands retain their existing behavior. No storage server, account database, launcher,
cloud product, domain or authentication system is added.

An embedding page can opt in by explicitly calling
`window.gridcraftFileProviders.registerProvider(id, label, namespace)` after the editor starts.
The object is frozen, has `protocolVersion: 1`, and announces readiness on the
`gridcraft-file-provider` CustomEvent. An integrator can also discover it directly if the ready
event has already fired. The matching API/event names are the only app-specific transport
names; the protocol matches PhotoCraft's optional provider bridge.

The browser shell attaches provider services only after installation and its `ready` announcement
succeed. A synchronous `ready` listener may register a provider and enqueue requests; the attached
service retains that exact Host and its work. A recoverable startup failure instead retires the
attempt's callbacks and removes only the API object that attempt installed. Local browser services
remain in place. A later attempt creates a fresh Host, nonce and connection identities, so callbacks
and replies retained from the failed attempt cannot address it. An existing global API is never
overwritten, and cleanup never removes a different object a page script put there during startup.
Such a replacement must be resolved by that page before another installation can succeed.

The shell owns the installed callbacks for its lifetime and retires its API on shutdown. Parent
host mode (`?host=parent`) and same-document host mode (`data-host="page"` on the canvas) retain
their existing file delivery ownership. When either mode connects, the independent provider API
is not installed. The separate `window.gridcraft` command API is unaffected.

## Host authority and activation

The provider is trusted code on the editor's origin, not an untrusted message sender. It owns
storage, authentication, per-user authorization, namespace selection and the destination
chooser. There is no cross-origin listener and no provider-origin URL to configure inside the
editor. Resource IDs are opaque identities, never filesystem paths or arbitrary engine commands.
The backend must enforce ownership on every list/read/write; browser namespaces do not replace
server authorization.

`registerProvider(id, label, namespace)` returns a fresh connection ID. Re-registering the exact
identity is idempotent. Changing account requires `unregisterProvider()` first, which refuses
while a chooser, serialization, upload acknowledgement or read is unresolved. Unregistering
also refuses while terminal events are queued or a publisher owns the current delivery batch,
including its last event. Reentrant publication leaves that outer batch in charge. Unregistering
clears resources and bindings without closing documents. `setEnabled(false)` stops new
provider choices and hides provider Recents while allowing outstanding operations to settle;
`setEnabled(true)` restores availability. There is no editor preference or end-user cloud switch.
Never reuse an account namespace for a different account.

## Save protocol

1. A user presses Save or Save As in the editor. The editor emits `saveDialog` with
   `requestId`, `documentId`, `revision`, `name`, `saveAs` and an optional `resourceId`.
   Serialization has not happened and nothing has been marked saved. Repeated Save while
   this document already has an outstanding operation reuses that operation.
2. The host asks for **Provider**, **This device**, or **Cancel** every time, then calls
   `resolveSaveDialog(requestId, destination, name)`. Destinations are `provider`, `device`,
   `cancel`. An empty name retains the suggestion. Cancel needs no further reply.
3. On a subsequent logic frame, the editor encodes a complete package in the selected format.
   Provider selection emits `save` with a fresh owned `Uint8Array` in `bytes`, its
   `bytesLength`, the exact captured `documentId`/`revision`, `name`, `resourceId` and request
   identity. Later edits cannot mutate those bytes. Save As never infers overwrite authority
   from a matching filename.
4. Persist that exact package durably, then call `saveResult(requestId, "saved", resourceId)`.
   Only a matching outstanding request can acknowledge the captured revision. Edits made after
   capture stay dirty, closed document IDs are ignored, and another tab/document is never
   marked saved. Other open documents bound to the same overwritten resource lose their stale
   binding and clean state; unrelated local files with the same name are untouched.
5. On failure call `saveResult(requestId, "failed", "")`, or use `cancel` for cancellation.
   `download` is a terminal fallback result that does not acknowledge provider persistence.
   These outcomes keep unsaved work open. A serialization/filename rejection emits `saveFailed`.

Drafts parked in another window of the workbook are committed before capture. If a command closed
that window while the workbook remained open, its draft is retained and recovered in a separate
window of that same workbook. This never replaces another window's cell or text-box draft. A
successful recovery is included in the package and its temporary window closes; a validation or
text-box refusal leaves the recovery window and draft open for correction. Closing the workbook
itself keeps the ordinary close/discard behavior. Replies for older captures cannot mark a workbook
clean while a later draft remains parked, including one whose original window closed.

A device choice uses the browser's local download service. Accepting that service only starts
the picker or download: it never marks a workbook clean, including a workbook not yet bound to
a provider. It does not replace an existing provider binding. A later picker cancellation or
write failure cannot acknowledge persistence; the editor keeps the unsaved-changes signal.
Exports remain local copies and never mark a provider project saved.

Provider Recent rows use the UI command `file.openProviderResource` with `{ "resource": id }`.
The control channel exposes it through `engine.execute` and lists its current enablement in
`engine.commands`; the CLI's `send` and MCP/control clients can use that same route. It queues
the existing account-scoped `openRequest`, opens no local dialog and accepts no filesystem path.
The host still supplies bytes for that request. Refused clicks use the normal persistent alert
UI, while the Backstage panel owns the theme and the separate provider/device sections.

Backstage Save on a provider-bound workbook uses the ordinary Save route and retains its
resource ID even when there is no device path. Save As still requests a new destination.
The explicit UI command `ui.chooseProviderSave` takes `{ "saveAs": bool }` (default false)
and returns `{ "handled": bool }`. Backstage uses this result to offer the provider chooser
or retain its existing local picker/download fallback. It is discoverable through the
control command catalog. This explicit interactive UI request does not intercept ordinary
programmatic `file.save`/`file.saveAs` calls or exports.

Supported Save packages: **XLSX, XLSM and native JSON**.

The ordinary workbook serializer includes every sheet and supported embedded workbook resources. A workbook carrying VBA defaults to XLSM; choosing XLSX for it is rejected so that saving cannot silently strip the VBA project. Formula evaluation and active-content trust rules are unchanged. Password-required reads fail explicitly without binding the previously active workbook.

## Recent files and opening

`setResources(connectionId, resources)` replaces a bounded list for the current connection.
Each resource is `{ id, name, updatedAt? }`; IDs must be unique. `updatedAt` is display text,
not a sorting instruction or trusted timestamp. The editor shows these entries in its native
Home/Open Recent section under the provider's label, separately from **Recent on this device**.
No provider resource is inserted into the device's recent filesystem paths. A successful save
also refreshes the provider entry in this session; the host supplies persistence across visits.

Clicking a provider Recent or calling `requestOpen(resourceId)` emits `openRequest` and returns
its correlated request ID. Read bytes from authorized storage and call
`openResource(requestId, name, Uint8Array)`. Only requests the editor issued are accepted.
`cancelOpen(requestId, message)` finishes a cancelled or failed read. After decoding/admitting
an actual document, the editor emits `opened`; malformed/unsupported/password-required bytes
emit `openFailed`. Bytes are not dropped into an uncorrelated general file-picker inbox.
Resource identity comes from the outstanding read, not a filename sent by the host.

## Identity, limits and lifecycle

Every event carries `providerId`, `namespace` and `connectionId`. Request IDs contain a random
per-WASM-instance nonce plus a checked monotonic counter. Late replies from a previous page,
connection, account, read or completed save cannot address a newer request. Metadata never
contains credentials. `snapshot()` reports connection state, current provider resources,
document IDs/dirty state and outstanding work; `setResources` rejects stale connection IDs.

- 80 MiB per file and aggregate queued open/capture event bytes.
- 16 pending operations; 64 queued events; 100 listed resources.
- IDs: 1–160 ASCII letters/digits, hyphen or underscore.
- Filenames: 1–255 UTF-8 bytes; no controls, slash, backslash, `.` or `..`.
- Labels and update text: at most 100 UTF-8 bytes; read-error text at most 1024 bytes.
- The WASM adapter checks JavaScript string/array/byte lengths before copying them into Rust;
  arbitrary extra host object properties are not stringified or copied.

The host should register listeners before requesting work and always settle each request.
A failing event listener cannot silently acknowledge a save. Recoverable JavaScript byte-copy
or event-dispatch errors are caught by the production publisher. A still-owned captured save
is transferred into a bounded failure inbox and consumed by the ordinary `SheetApp` Result
path on the next logic frame. It remains pending until that consumer runs. This is a failure,
never a durable acknowledgement: newer edits, workbook resources, bindings and alias clean
states are preserved, and the ordinary chooser can start a fresh retry. Accepted synchronous
callbacks are not replaced by a later dispatch error.

A failed `saveDialog` or `openRequest` dispatch retires only its still-owned current-connection
request and queues a correlated `saveFailed` or `openFailed`. Later events in the drained batch
continue. A full command queue refuses a non-cancel chooser reply without consuming its owner.
The ordinary command limit stays 16. The separate failure inbox contains at most 16 already
admitted captures and no file bytes or newly admitted requests. Terminal events remain bounded
by their original owners and cannot be erased by account replacement while queued/in flight.
The publisher requests repaint for failure consumption and work left after its bounded drains.

Exceptions swallowed by the browser while invoking a DOM event listener are not observable at
this boundary. Those saves still require an explicit host result. A process abort or fatal WASM
allocation failure cannot be recovered by catching a JavaScript exception. This contract does not
provide autosave, a background synchronization daemon, storage conflict resolution or remote
session authentication. Those are host/backend decisions.

## Validation sources

`crates/ui-egui/src/file_providers/tests.rs` covers opt-in defaults, metadata bounds,
coalescing, choices, exact captures, cancellation, identity changes, aliases and correlated
reads. `crates/ui-egui/src/app/provider_files.rs` includes native package round-trip and
late-edit/failure regressions. They are source fixtures; execution results belong in the PR's
validation report after running the repository's checks.

`apps/gridcraft-web/src/file_provider_browser_tests.rs` adds one real-window fixture using the
production catch-enabled publisher and `SheetApp::logic`. Rust closures inject recoverable
JavaScript copy and request-dispatch errors. The fixture catches the actual publisher call with
`Function::call0` and restores each exact own-property descriptor (or removes its temporary
shadow) before propagating the result. It checks later-event continuation, failure consumption,
retry payload identity and native XLSM round-trip including
two sheets, formulas, defined names, synthetic VBA and an opaque original workbook part.
Native tests also cover a full command/event queue, accepted reentrant callbacks and the last
in-flight terminal. These are regression sources, not execution or backend-durability results.

Resolving a destination commits that workbook's cell and text-box drafts again, including
drafts parked in another window. The previously active window and unrelated workbook drafts
are preserved. Validation refusals keep the original draft in front and abort the capture.
ACKs leave later drafts dirty. Resource overwrites invalidate alias revisions before accepting
any subsequently delivered acknowledgement.
