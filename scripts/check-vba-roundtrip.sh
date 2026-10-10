#!/usr/bin/env bash
# Verify that GridCraft preserves VBA macros (xl/vbaProject.bin) across a round-trip.
#
#   bash scripts/check-vba-roundtrip.sh
#
# What it does: builds a macro-enabled .xlsm containing a fake vbaProject.bin, runs it
# through `gridcraft-cli run`, and checks the blob comes back byte-identical with the
# macro-enabled content type + relationship intact. Requires python3 (zip) and a
# release build of gridcraft-cli.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="$ROOT/target/release/gridcraft-cli.exe"
[ -x "$CLI" ] || CLI="$ROOT/target/release/gridcraft-cli"
PYTHON="${PYTHON:-python3}"

# A fresh temp dir. Under Git Bash (MSYS) the shell's /tmp is not a path the native
# Windows python.exe or the engine understand when embedded in JSON, so convert to the
# platform-native form when cygpath is available.
TMP="$(mktemp -d)"
if command -v cygpath >/dev/null 2>&1; then
  NATIVE_TMP="$(cygpath -w "$TMP")"   # for python (native Windows path)
  JSON_TMP="$(cygpath -m "$TMP")"     # for JSON params (forward slashes: no escaping)
else
  NATIVE_TMP="$TMP"
  JSON_TMP="$TMP"
fi

# 1. Build a macro-enabled .xlsm with a fake OLE/CFB vbaProject.bin.
"$PYTHON" - "$NATIVE_TMP" <<'PY'
import zipfile, os, sys
tmp = sys.argv[1]
main_ct = "application/vnd.ms-excel.sheet.macroEnabled.main+xml"
ct = ('<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
      '<Default Extension="xml" ContentType="application/xml"/>'
      '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
      f'<Override PartName="/xl/workbook.xml" ContentType="{main_ct}"/>'
      '<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
      '<Override PartName="/xl/vbaProject.bin" ContentType="application/vnd.ms-office.vbaProject"/></Types>')
root = ('<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>')
wb = ('<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
      'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
      '<sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>')
rel = ('<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
       '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>'
       '<Relationship Id="rId2" Type="http://schemas.microsoft.com/office/2006/relationships/vbaProject" Target="vbaProject.bin"/></Relationships>')
sheet = '<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>'
vba = b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1" + b"HELLO-VBA-PROJECT-BYTES" + bytes(range(32))
with zipfile.ZipFile(os.path.join(tmp, "macro_demo.xlsm"), "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("[Content_Types].xml", ct)
    z.writestr("_rels/.rels", root)
    z.writestr("xl/workbook.xml", wb)
    z.writestr("xl/_rels/workbook.xml.rels", rel)
    z.writestr("xl/worksheets/sheet1.xml", sheet)
    z.writestr("xl/vbaProject.bin", vba)
open(os.path.join(tmp, "vba_expected.bin"), "wb").write(vba)
print("built", os.path.join(tmp, "macro_demo.xlsm"))
PY

# 2. Round-trip through the engine.
echo "--- view.macros on the input (what the app sees) ---"
"$CLI" run --in "$NATIVE_TMP/macro_demo.xlsm" --cmd 'view.macros={}' | tail -1
"$CLI" run --in "$NATIVE_TMP/macro_demo.xlsm" --cmd "file.save={\"path\":\"$JSON_TMP/macro_out.xlsm\"}" | tail -1

# 3. Assert the blob survived byte-identical with correct package wiring.
"$PYTHON" - "$NATIVE_TMP" <<'PY'
import zipfile, os, sys
tmp = sys.argv[1]
exp = open(os.path.join(tmp, "vba_expected.bin"), "rb").read()
with zipfile.ZipFile(os.path.join(tmp, "macro_out.xlsm")) as z:
    got = z.read("xl/vbaProject.bin")
    ct = z.read("[Content_Types].xml").decode()
    rels = z.read("xl/_rels/workbook.xml.rels").decode()
ok = (got == exp
      and "macroEnabled.main+xml" in ct
      and "/xl/vbaProject.bin" in ct
      and "relationships/vbaProject" in rels)
print("blob byte-identical:", got == exp)
print("macro-enabled main content type:", "macroEnabled.main+xml" in ct)
print("vbaProject content-type override:", "/xl/vbaProject.bin" in ct)
print("vbaProject relationship:", "relationships/vbaProject" in rels)
sys.exit(0 if ok else 1)
PY

echo "PASS: the VBA project survived the round-trip."