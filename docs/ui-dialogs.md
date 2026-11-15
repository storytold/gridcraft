# Dialog design and testing

> **Last reviewed:** 2026-11-15 · **Last updated:** 2026-11-15 · **Change:** minor (refresh source references and tracked work; initial guide, #2190)

This guide covers GridCraft's egui forms, custom dialogs, message boxes and confirmations.
**Current implementation** below was checked against `main` at
`8bde636f272cb5ac1d9b945cecf17791febe605d`. **Design rules** are the target for new work.
The tracked-work table records implementation progress on
[#2190](https://github.com/storytold/gridcraft/issues/2190) and related issues.

## Scope and existing architecture

Keep dialog code in the UI layer. Follow [architecture.md](architecture.md) and
[AGENTS.md](../AGENTS.md): the UI collects parameters and uses the existing command path rather
than changing workbook state directly.

| Piece | Source and responsibility |
|---|---|
| State and entry | [lib.rs](../crates/ui-egui/src/lib.rs) declares `SheetApp.dialog` as an `Option<dialogs::Dialog>`. [app/commands.rs](../crates/ui-egui/src/app/commands.rs) defines `open_dialog`, including host file pickers, and `after_engine`, which handles `UiRequest::Dialog`. |
| Dialog state | [dialogs.rs](../crates/ui-egui/src/dialogs.rs): `Dialog` holds `name`, `title`, `fields`, `values`, optional `command` and custom-body state. `Dialog::open` chooses a form or custom dialog. |
| Generic forms | `Dialog::form` and `Field` live in `dialogs.rs`. [dialogs/show.rs](../crates/ui-egui/src/dialogs/show.rs) draws the fields with `form_fields` and the footer with `ok_cancel`. Edits go into `Dialog.values`, which `build_params` converts before `app.run`. |
| Window and custom bodies | `dialogs::show` is re-exported from `dialogs/show.rs`. It draws an `egui::Window` and dispatches custom bodies, including [Format Cells](../crates/ui-egui/src/dialogs/format_cells.rs) and [Solver and Data Analysis](../crates/ui-egui/src/dialogs/analysis.rs). Some bodies use separate action rows or lifetimes. |
| Range picking | [dialogs/ref_edit.rs](../crates/ui-egui/src/dialogs/ref_edit.rs) manages reference boxes and the sheet and selection used during picking. See the [Dialogs section of UI parity](ui-parity.md#dialogs) for the interaction rules. |
| Parent return | `Dialog.back` in `dialogs.rs` preserves a parent, for example Name Manager while New Name is open. Go To hands over to Go To Special instead. This is not a general stack of simultaneously interactive dialogs. |
| Confirmation | The `saveChanges` body has its own action row and calls `answer_save_changes` in [app/quit.rs](../crates/ui-egui/src/app/quit.rs). |
| Message boxes | [widgets.rs](../crates/ui-egui/src/widgets.rs): `message_box` draws an `egui::Modal` from `SheetApp.message`. [app.rs](../crates/ui-egui/src/app.rs) draws it after `dialogs::show`. |
| Translation | [i18n.rs](../crates/ui-egui/src/i18n.rs): `Language::tr` and `i18n::current` provide interface translation with English fallback. |
| Automation | [control.rs](../crates/ui-egui/src/control.rs) implements the methods listed in [control-protocol.md](control-protocol.md), including dialog state and message inspection. |

A shared frame with a `DialogSpec` or `ButtonRole` API is a possible future design. The existing
form helper and custom action rows are the starting points for changes.

### Automation is a separate path

`ui.dialog` opens a named dialog. `ui.dialog.set` updates values (or the special `tab` and
`search` fields). `ui.inspect` reports both `dialog` and `message`.
`ui.dialog.confirm` uses the same `build_params` conversion as the OK button for dialogs with a
stored command, returns the dialog on failure, and rejects dialogs that handle OK themselves.
For RefEdit, confirmation restores the dialog's sheet before running the command. Scripted
cancel ends the pick and can return an error while keeping the dialog open. UI cancellation
can close it and show the reason, and can return to a parent dialog. Use input-driven tests for
keyboard handling and parent return, and test scripted confirmation and cancellation separately
where their outcomes matter.

## Layout and sizing

**Current implementation.** The main window path in `dialogs::show` sets a default width by
name (380 logical egui points for a typical form), applies a minimum content width, and disables
resizing. [PR #2094](https://github.com/storytold/gridcraft/pull/2094) moved the shared
right-to-left action layout into a horizontal row as tall as its buttons, fixing the expanding
footer. [#978](https://github.com/storytold/gridcraft/issues/978) also calls for sizing forms
to their content. Custom bodies can use their own widths and scroll regions.

**Design rules.**

- Size ordinary forms from their actual fields and action row. A one-field form must not grow
  to the window's height or inherit a large empty minimum height without a reason.
- About 380–440 logical egui points is a useful starting width for an ordinary form, not a
  required size for every dialog. Numeric forms may be narrower; tables, charts and other
  complex bodies may need more room. Explain exceptions and check them at a smaller viewport.
- Scroll a long content region, keeping the title and bottom actions visible. Bound the region
  by available viewport space; do not put the footer inside the scrolling body.
- Use a consistent label/input arrangement within a form: aligned label and control columns,
  or a consistent stacked layout when longer translations need it. Numeric inputs must not
  start at different positions just because their labels have different lengths.
- Measure in logical points, allowing for UI scale, longer translations and wrapped messages.
  Test both short forms and long bodies; a fixed screenshot size is not a sizing rule.

## Button roles and order

These are semantic roles for review and future helpers, not existing Rust types:

| Role | Meaning |
|---|---|
| Confirm / default | Validate and perform the intended operation, then close on success. Only the explicitly chosen default action may be activated by an otherwise unhandled Enter. |
| Cancel | Discard uncommitted dialog edits and leave the operation without applying them. |
| Alternate | A separate explicit outcome, such as Don't Save; it must not become the default through layout order. |
| Apply | Validate and apply changes while keeping the dialog open. Include it only when that behaviour is useful and supported by the command flow. |

Use an explicit minimum button width, allowing translated text to make the button wider.
Specify the chosen minimum in the implementation and test it; this baseline has no shared
minimum-width constant. Keep a consistent minimum within an action row.
Do not pad labels with spaces to simulate width.
Use theme tokens for a consistent primary/secondary hierarchy, including focus and disabled
states; colour alone must not be the only way to identify the default action.

Use the project's confirmed convention for the target OS. Record the intended visual order and
default in the implementation PR and its tests; if a platform convention has not been agreed,
resolve it in review rather than inventing a different order per dialog. The Windows target
reported in #2190 for Save Changes is **Save, Don't Save, Cancel**, with **Save** as default.
Do not assume the same visual order for macOS or infer an agreed Linux order from this document.

**Current implementation.** `ok_cancel` has a filled OK and plain Cancel with spaces in their
translated labels and no explicit minimum width. `message_box` has a padded English OK label.
Save Changes uses three plain buttons in the order Don't Save, Cancel, Save, with no explicit
Enter default. Fixing their layout must not silently change their meaning: Save commits the
cell draft and attempts a save before closing a clean workbook, Don't Save discards by closing,
and Cancel leaves the workbook open. A cancelled or failed save must not discard the workbook.

## Keyboard and focus

**Design rules.**

- Enter confirms only if the intended action is enabled, validation permits it, and the focused
  control has not used Enter for its own operation. Mouse and keyboard must use the same
  eligibility check; an Enter shortcut must not bypass a disabled button.
- A focused multiline editor owns Enter, including Shift+Enter when used for a newline.
  Typing a line must not also submit the dialog. Choice popups and other key-handling controls
  must get their input before a default dialog action.
- Handle a confirmation key once. If confirmation creates an error message, that same Enter
  must not dismiss the new message in the frame in which it was created (see #2189).
- Esc cancels a staged form. If a child popup or message owns Esc, dismiss that surface first;
  do not let the same event also cancel its parent. Closing the window should follow its
  documented cancellation policy.
- On opening a value-entry dialog, focus the first enabled editable field once focus can take
  effect. For a replaceable existing value, select it so typing replaces it. Do not steal focus
  back on every frame, after validation, or while returning from a popup.
- Tab and Shift+Tab should move predictably forward and backward through the dialog's enabled
  controls in reading order. Keep focus visible and keyboard access to actions, choices and
  checkboxes. Do not import PhotoCraft's text-and-number-only Tab rule as a GridCraft feature.
- A number typed immediately before Enter or a mouse click on OK must be the number submitted,
  not the previous value before the field lost focus.

**Current implementation.** The shared `take_first_focus` helper from
[PR #1331](https://github.com/storytold/gridcraft/pull/1331) focuses the first eligible text or
number field and selects its value when focus arrives. Numeric fields update while typing.
Custom bodies can manage focus themselves. `form_fields` uses a multiline editor for text
fields whose key is `text`, while `ok_cancel` reads Enter with `key_pressed` without consuming
it or checking multiline focus. [PR #1574](https://github.com/storytold/gridcraft/pull/1574)
is open and addresses this for the note editor. `dialogs::show` handles Esc, and
`message_box` reads Enter and Esc after it. Tab handling depends on the widgets and body.

## Localization and accessibility

**Design rules.** Translate user-visible titles, tabs, field labels, choices, notes, messages and
action labels through the existing `Language::tr` / `i18n::current` mechanism at the display
boundary. Preserve user-entered text, command IDs, dialog names, field keys, option values and
automation parameters. A translated caption must not become a new command parameter or field ID.
Keep interpolation values such as workbook names intact; translate the surrounding message,
not the user data. Follow the existing `context|label` lookup when a caption has different
meanings in different places, without showing the context prefix to users.

Give inputs and actions meaningful accessible names and appropriate roles, associate labels
with their inputs, keep a visible focus indicator, and make all actions usable by keyboard.
Password controls must stay masked unless the user explicitly reveals them; neither messages
nor accessibility values should leak the password. Do not describe the existence of AccessKit
or a few queryable buttons as complete screen-reader support; [ui-parity.md](ui-parity.md)
records the broader accessibility gap.

**Current implementation.** The window title, shared OK/Cancel and Format Cells captions use
translation. [PR #2106](https://github.com/storytold/gridcraft/pull/2106) added translation
for generic field labels, choices and notes at draw time, keeping the stored values unchanged.
Message boxes translate titles and error text, but still use a literal OK label. Save Changes
still has a literal prompt and action labels. Coverage varies across custom bodies and catalogs,
and [PR #669](https://github.com/storytold/gridcraft/pull/669) is open with broader dialog and
message-box localization.

## Confirm, cancel and error handling

Stage ordinary form edits in `Dialog.values`. Cancel or Esc must not submit those values.
A custom manager that runs commands immediately (for example deleting a defined name) is a
different workflow: make immediate actions explicit, and do not promise that closing it undoes
already completed commands. Apply or preview behaviour also needs an explicit cancellation policy.

Validate before closing. On failure, keep the dialog and recoverable input, show a useful error,
and allow correction and retry. Mouse and keyboard confirmation must make the error equally
visible. Password retry may deliberately clear the secret; this is an existing exception, not
permission to reset every field. Save/close confirmation must preserve unsaved data on a failed
or cancelled save. Keep related cell-editor commit and command validation checks in place.

Use `app.run` and the existing command/parameter conversion path for the operation; report errors
through the existing UI message path. Do not add panics, unchecked input-derived indexing or
`unsafe`, and keep egui-specific policy out of the engine. Programmatic operations should return
results/errors rather than unexpectedly opening UI; explicit `ui.dialog` automation is separate.

**Current implementation.** The generic command branch in `dialogs::show` runs `build_params`,
closes on success, and normally retains the dialog on failure while setting `app.message`.
Open Password clears the password on failure; Data Analysis can retain an overwrite question.
Custom bodies must be checked individually; some invoke `run_or_alert` and manage closing
themselves. This is not a uniform validation/close contract for every dialog. Because
`widgets::message_box` can see the same Enter immediately afterwards,
[#2189](https://github.com/storytold/gridcraft/issues/2189) remains an error-visibility gap.

## Test requirements

For runtime changes, drive the real `SheetApp` with `egui_kittest::Harness`, as in
[dialog_focus.rs](../crates/ui-egui/tests/it/dialog_focus.rs). Build with `build_ui_state`,
call `app.logic` and `app.ui`, and advance frames with `run_steps`. Use label or role queries
and send pointer and keyboard input for the behaviour being changed. A keyboard regression
should reach the expected focus or confirmation through input events.

| Check | Evidence expected for a relevant runtime change |
|---|---|
| Width and height | Open short and long forms at two viewport sizes. Measure content and controls after layout settles; ensure a short form does not fill the viewport, longer content scrolls, and the footer remains visible. Use current geometry, not unrelated absolute screen coordinates. |
| Footer | Assert buttons are below the last field or in a reserved footer, have the chosen minimum width and platform order, and do not clip longer translated labels. See the geometry queries in [dialog_button_row.rs](../crates/ui-egui/tests/it/dialog_button_row.rs). |
| Default and cancel | Verify mouse and Enter perform the same intended command exactly once; Cancel, Esc and window-close leave staged changes unapplied. Test Save, Don't Save and Cancel separately with failed/cancelled save cases. |
| First field | Open, type without a preliminary click, and confirm. Check that the intended field received the text, its old value was replaced where appropriate, and the resulting command uses the new value. |
| Key ownership | Exercise Enter, Esc, Tab and Shift+Tab, disabled confirmation, and a choice popup. Assert focus movement and command outcomes, including no input leaking to the sheet. |
| Multiline | Focus the multiline field, type a newline, and assert the dialog remains open and its text contains the line break. Then explicitly confirm or cancel. |
| Error and retry | Submit an invalid rename by mouse and by Enter; check the message in that frame and later frames, retained input and unchanged workbook. Dismiss, correct and retry. Use a duplicate sheet name and an invalid name. |
| Localization and access | Check an available non-English catalog, visible/accessibility labels, unchanged IDs and values, masking, and keyboard reachability. Include a longer translation in layout coverage. |

Existing examples to extend:

- [zoom_dialog.rs](../crates/ui-egui/tests/it/zoom_dialog.rs) exercises button-driven and
  control-driven confirmation and verifies the workbook's zoom value.
  [dialog_confirm.rs](../crates/ui-egui/tests/it/dialog_confirm.rs) checks parameter conversion.
- [interaction.rs](../crates/ui-egui/tests/it/interaction.rs) covers Name Manager child return,
  Data Analysis overwrite confirmation and its footer in a short viewport, and translated
  Format Cells labels with stable values. [ref_edit.rs](../crates/ui-egui/tests/it/ref_edit.rs)
  exercises range picking and dialog return paths.
- [protect_password.rs](../crates/ui-egui/tests/it/protect_password.rs) checks masking through
  `Role::PasswordInput`. [open_encrypted.rs](../crates/ui-egui/tests/it/open_encrypted.rs) checks
  retry, cancellation and programmatic file opening without a dialog.
- [file_menu.rs](../crates/ui-egui/tests/it/file_menu.rs) checks file actions and the unsaved prompt.
  [page_setup_margins.rs](../crates/ui-egui/tests/it/page_setup_margins.rs) checks preservation of
  existing and unedited values.
- [i18n.rs](../crates/ui-egui/src/i18n.rs) contains catalog, fallback and context-prefix tests;
  they do not prove that every dialog string is translated at draw time.

A regression test should fail without the behavioural fix and pass with it. Describe what was
actually run, separating input/state assertions, geometry assertions, snapshots and manual
platform checks. A snapshot alone does not establish keyboard or error handling.

## Current gaps and tracked work

Status was checked on 2026-11-15 against the baseline above. Recheck open work before changing
shared dialog code.

| Problem | Current main | Tracking | Target and verification |
|---|---|---|---|
| Form sizing and footer | The shared action row fits its buttons; default and minimum widths remain. | [#978](https://github.com/storytold/gridcraft/issues/978) (open), [PR #2094](https://github.com/storytold/gridcraft/pull/2094) (merged) | Test short and long forms and footer visibility at small viewport sizes. |
| First-field focus | The shared helper focuses text and number fields; numeric edits update while typing. | [PR #1331](https://github.com/storytold/gridcraft/pull/1331) (merged) | Preserve typing immediately after open, selection of the old value and submission of the latest edit. |
| Multiline Enter also confirms | `ok_cancel` reads Enter after the editor. | [PR #1574](https://github.com/storytold/gridcraft/pull/1574) (open) | Multiline owns newline; test text, open state and eventual confirmation. |
| Enter hides a new error | Dialog and message box can read the same key in one frame. | [#2189](https://github.com/storytold/gridcraft/issues/2189) (open) | One event has one owner; assert message visibility after mouse and keyboard failure, then retry. |
| Generic field localization | Labels, choices and notes are localized at draw time. | [PR #2106](https://github.com/storytold/gridcraft/pull/2106) (merged) | Test translated labels together with unchanged IDs and submitted values. |
| Broader dialog/message localization | Custom coverage is partial; the Save Changes prompt and message-box OK still use literal labels. | [PR #669](https://github.com/storytold/gridcraft/pull/669) (open) | Translate remaining interface text while preserving workbook text and automation. |
| Button minimums, Save Changes default/order, label alignment | Padded labels, no shared role/minimum-width helper, custom Save Changes row and mixed field layouts. | [#2190](https://github.com/storytold/gridcraft/issues/2190) | Explicit roles, widths, platform order and consistent alignment; test each action and layout. These runtime changes are outside this document. |

## Adding or changing a dialog

1. Identify its existing `Dialog::open` entry, form/custom body and command. Check this table and
   open PRs before editing shared helpers; extend the existing path rather than adding a modal system.
2. Choose content sizing, scrolling and label alignment. Explain a complex body's exceptions.
3. Record each action's role, default, enabled condition, minimum width and target-platform order.
4. Define first focus, Tab order, multiline input and Enter/Esc ownership, including child surfaces.
5. Translate display strings with stable dialog names, field keys, option values and command IDs;
   check accessible names, focus indication and password masking.
6. Trace parameter conversion and command execution. Retain input and visible errors on failure;
   verify Cancel, save cancellation and any parent-return or immediate-action behaviour.
7. Add meaningful UI regressions for changed behaviour and check automation separately. Update
   the affected parity record without marking an open PR as implemented.

## References and revision history

The pinned [PhotoCraft guide](https://github.com/storytold/photocraft/blob/872748fb5b2a53e09dd4e385e31968d985c0515b/docs/ui-dialogs.md)
and the sibling-app examples in [#2190](https://github.com/storytold/gridcraft/issues/2190) inform
the goals. For the discussion of cross-app UI component reuse, see
[craft-libs#67](https://github.com/storytold/craft-libs/issues/67).

| Date | Change | Summary |
|---|---|---|
| 2026-11-15 | minor | Refresh module and test paths, RefEdit and scripted confirmation, and the status of form layout, focus and localization work |
| 2026-10-22 | major | Document GridCraft's existing paths, target rules, test requirements and open work for #2190; no runtime changes |
