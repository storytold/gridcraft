#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
manifest=${CORPUS_MANIFEST:-"$script_dir/manifest.tsv"}
cli=${GRIDCRAFT_CLI:-"$script_dir/../target/release/gridcraft-cli"}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

prefix='https://raw.githubusercontent.com/apache/poi/2c00b85/test-data/spreadsheet/'
files=()
declare -A seen_names=()
count=0
while IFS=$'\t' read -r name digest url || [[ -n ${name:-} ]]; do
    [[ -n ${name:-} ]] || continue
    if [[ ! $name =~ ^[A-Za-z0-9_.-]+\.xlsx$ || $name == *..* ]]; then
        printf 'corpus: invalid filename: %s\n' "$name" >&2
        exit 2
    fi
    if [[ -n ${seen_names[$name]+x} ]]; then
        printf 'corpus: duplicate filename: %s\n' "$name" >&2
        exit 2
    fi
    seen_names[$name]=1
    if [[ ! $digest =~ ^[[:xdigit:]]{64}$ || $url != "$prefix"* ]]; then
        printf 'corpus: invalid manifest row for %s\n' "$name" >&2
        exit 2
    fi
    path="$tmp/$name"
    curl --fail --location --silent --show-error --retry 3 "$url" --output "$path"
    if ! printf '%s  %s\n' "$digest" "$path" | sha256sum --check --status -; then
        printf 'corpus: SHA-256 mismatch: %s\n' "$name" >&2
        exit 1
    fi
    files+=("$path")
    ((count += 1))
done < "$manifest"

if (( count != 24 )); then
    printf 'corpus: expected 24 manifest entries, found %s\n' "$count" >&2
    exit 2
fi
if [[ ! -x $cli ]]; then
    printf 'corpus: gridcraft-cli is not executable: %s\n' "$cli" >&2
    exit 2
fi

set +e
"$cli" check --json "${files[@]}" > "$tmp/report.json"
check_status=$?
set -e
if (( check_status > 1 )); then
    printf 'corpus: gridcraft-cli check failed with exit code %s\n' "$check_status" >&2
    exit "$check_status"
fi

python3 - "$tmp/report.json" "$count" <<'PY'
import json
import os
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    report = json.load(f)
totals = report.get("totals")
if not isinstance(totals, dict) or totals.get("files") != int(sys.argv[2]):
    raise SystemExit("corpus: gridcraft-cli returned an invalid file count")
summary = (
    f"{totals['files']} files; {totals['opened']} opened; {totals['failedToOpen']} failed to open; "
    f"{totals['checked']} formulas checked; {totals['mismatched']} mismatches; "
    f"{totals['roundTripFailed']} round-trip failures; {totals['roundTripDiffering']} round-trip differences"
)
print(f"gridcraft-cli check: {summary}")
summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
if summary_path:
    with open(summary_path, "a", encoding="utf-8") as f:
        f.write("## Public workbook corpus\n\n")
        f.write(summary + "\n")
PY
