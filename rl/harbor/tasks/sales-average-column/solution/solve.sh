#!/usr/bin/env bash
# Reference solution, run by Harbor's oracle agent.
set -euo pipefail
args=(--cmd 'cell.set={"cell":"J4","input":"Average"}')
for r in 5 6 7 8 9 10; do
  args+=(--cmd "cell.set={\"cell\":\"J$r\",\"input\":\"=AVERAGE(C$r:F$r)\"}")
done
args+=(--cmd 'cell.set={"cell":"J11","input":"=AVERAGE(J5:J10)"}')
gridcraft-cli run --in /app/sales.xlsx "${args[@]}" --out /app/sales.xlsx --quiet
