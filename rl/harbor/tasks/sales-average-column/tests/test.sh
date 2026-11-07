#!/usr/bin/env bash
# Harbor verifier: writes a reward in [0, 1] to /logs/verifier/reward.txt.
set -uo pipefail
mkdir -p /logs/verifier
python3 /tests/test_outputs.py --reward /logs/verifier/reward.txt || true
[ -f /logs/verifier/reward.txt ] || echo 0 > /logs/verifier/reward.txt
