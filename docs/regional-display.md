# Regional number and short-date display

This adapts [#2263](https://github.com/storytold/gridcraft/pull/2263) by **@pick2510**.
The five original commits are retained as merge ancestry, with their original author and
co-author metadata. The adaptation ports painting and AutoFit changes to the current modules,
adds Romanian conventions and addresses the numeric short-date case reported in
[#2869](https://github.com/storytold/gridcraft/issues/2869).

## Review maintenance — 2026-10-31

The function contracts touched by this PR now document locale selection, literal custom-format
identity, numeric ICU conversion, on-screen painting, AutoFit and the existing synthetic
regressions. Commit `1d4cdafb` is documentation only: executable Rust and test assertions are unchanged.
The original five commits from @pick2510 remain ancestors. The compatibility limits below
and the localization readiness estimate are unchanged.

The Windows follow-up reads the user's format locale through `current_locale`'s safe
`GetUserDefaultLocaleName` wrapper instead of `sys_locale`, which reports preferred UI languages
on Windows. The interface-language path remains unchanged. An in-memory UI regression switches
English/German/Romanian interface languages while retaining Romanian number/date display.
The native Windows API path still needs validation on Windows; the regression injects the
format region and does not change operating-system settings.

## Behavior

- `ro-RO` (also `ro`, `en-RO`, `ro_RO.UTF-8`) displays `1234567.89` with `#,##0.00`
  as `1.234.567,89`, and built-in short dates as `04.03.2026`.
- The numeric region is independent of the interface language. POSIX `@rg=rozzzz` and
  Unicode `u-rg-rozzzz` overrides take precedence; private-use subtags do not.
- On Windows, the user format-locale name supplies the region (for example Romanian formats
  with an English interface); failure retains the existing en-US display fallback. POSIX
  environment variables do not override this Windows format-locale lookup.
- On macOS, Foundation's current-locale short-date formatter supplies supported numeric
  user customizations. ICU `dd.MM.y` becomes Excel `dd.mm.yyyy`; ICU `yy` remains a two-digit year.
  No system preferences are changed. The locale is captured at application startup.
- Only the short-date family (built-ins 14/22) follows the regional date pattern. An imported
  custom format with code `m/d/yyyy` or `m/d/yyyy h:mm` remains literal and is saved with a
  custom format ID. Differential formats (`dxf`, used by conditional formatting) respect their
  declared built-in/custom ID as well; a missing ID with an explicit code is treated as custom.
  Built-in DXF standard spellings `mm-dd-yy` (14) and `m/d/yy h:mm` (22) normalize to the
  internal canonical codes; explicit custom spellings keep their own code and custom ID.
  Opening Format Cells or changing a font retains the custom identity; actual number-format
  edits (preset, code field or shortcut) can replace it. Case and escaped-separator aliases
  are retained too. Numeric separators
  still follow the display locale, while quoted literals and stored currency symbols stay intact.
- Grid painting, Page Layout painting and column AutoFit use the same session locale.
  `snapshot --locale ro-RO` overrides the system locale before executing commands.
- Stored numbers, date serials (both 1900 and 1904), formulas and number-format codes stay
  invariant. Input parsing, the editor, command display text, CSV and `TEXT()` keep their existing
  en-US behavior. Printed/exported PDF text also keeps the existing invariant behavior.

## Model representation

`NumFmt::new` preserves the existing format-code behavior. `NumFmt::custom` marks only custom
short-date aliases whose code could otherwise collapse to built-in 14/22. These formats intern
separately and remain separate through XLSX save/reopen and style serialization. Existing
serialized string formats are accepted. Only a marked alias serializes as
`{"code":"m/d/yyyy","literal_short_date":true}`; callers inspecting raw style JSON must accept this
additional representation. Other formats keep their string representation.

## Regression coverage

Synthetic workbooks are built in Rust from original XML and kept in memory by default:

```
cargo test -p gridcraft-numfmt --lib
cargo test -p gridcraft-engine --test regional_formats
cargo test -p gridcraft-ui-egui --test regional_formats
```

The XLSX test checks an ambiguous date (4 March), built-in 14/22 versus custom aliases,
Romanian explicit dates, grouping, negative RON literals, percentages, formula text/results,
1900/1904 date systems and three save/reopen cycles. It checks built-in IDs in `styles.xml`.
The UI tests inspect actual egui text shapes. `GRIDCRAFT_SYNTHETIC_OUTPUT` optionally writes
four test workbooks to an existing directory, using exclusive creation and refusing overwrites.
Generated binary fixtures are not committed.

## Remaining limits

This is regional display, not full locale support. No automatic currency-symbol substitution,
localized input/parser or translated month/day names is added. Built-ins 15–21 retain their
existing behavior. Unsupported ICU patterns (month names, eras, week-years or unsupported
literals) fall back to the region table; Gregorian date rendering remains the existing renderer.
Custom OS number separators and custom Windows/Linux short-date preferences are not queried.
Unknown regions and web startup retain the en-US fallback. Native Windows/Linux execution and
changing macOS date preferences need separate validation; tests inject the reported pattern
without modifying the workstation settings. No Excel-produced fixtures or vendor assets are used.

## Public references

- [Unicode CLDR 48 Romanian data](https://github.com/unicode-org/cldr/blob/release-48/common/main/ro.xml)
  specifies comma decimals, dot grouping and numeric short date `dd.MM.y`.
- [Microsoft's Office implementation notes for number formats](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oe376/0e59abdb-7f4e-48fc-9b89-67832fa11789)
  describe Excel's built-in 14/22 codes.
- [Apple DateFormatter documentation](https://developer.apple.com/documentation/foundation/dateformatter)
  documents current-locale date styles and numeric date patterns.

- [Microsoft GetUserDefaultLocaleName](https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-getuserdefaultlocalename)
  specifies the user default locale-name API used by the Windows-only safe wrapper.
