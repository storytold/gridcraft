"""Verifier for sales-average-column.

Reads /app/sales.xlsx through gridcraft-cli (no network or pip needed) and scores each check.
The reward is the fraction of scored checks passed, so partial progress earns partial credit.
Gates must all pass or the reward is 0 (a broken or tampered workbook earns nothing).
Expected numbers are hardcoded from the sample data, so editing the inputs can't move them.
"""

import argparse
import csv
import io
import subprocess
import sys

BOOK = "/app/sales.xlsx"
QUARTERS = {
    5: [48200, 51340, 56010, 61870],
    6: [39150, 37980, 42400, 45120],
    7: [55300, 58910, 57230, 63480],
    8: [61020, 64400, 70150, 76390],
    9: [28740, 31260, 33980, 36510],
    10: [72410, 81630, 95220, 112840],
}


def cli(*args: str) -> str:
    out = subprocess.run(["gridcraft-cli", *args], capture_output=True, text=True, timeout=30)
    if out.returncode != 0:
        raise RuntimeError(out.stderr.strip())
    return out.stdout.strip()


def value(cell: str) -> str:
    return cli("eval", f"={cell}", "--in", BOOK)


def formulas(rng: str) -> list[str]:
    rows = csv.reader(io.StringIO(cli("cat", BOOK, "--range", rng, "--formulas", "--csv")))
    return [row[0] if row else "" for row in rows]


def close(text: str, want: float) -> bool:
    try:
        return abs(float(text) - want) <= 1e-6 * max(1.0, abs(want))
    except ValueError:
        return False


def gates():
    yield "workbook opens", lambda: bool(cli("info", BOOK))
    yield "quarter data untouched", lambda: all(
        close(value(f"{col}{r}"), v) for r, qs in QUARTERS.items() for col, v in zip("CDEF", qs)
    )


def checks():
    averages = {r: sum(q) / 4 for r, q in QUARTERS.items()}
    yield "J4 header is Average", lambda: value("J4").strip().strip('"') == "Average"
    for r, want in averages.items():
        yield f"J{r} is a formula", lambda r=r: formulas(f"J{r}")[0].startswith("=")
        yield f"J{r} = {want}", lambda r=r, want=want: close(value(f"J{r}"), want)
    yield "J11 is a formula", lambda: formulas("J11")[0].startswith("=")
    yield "J11 averages J5:J10", lambda: close(value("J11"), sum(averages.values()) / len(averages))


def run(kind: str, name: str, check) -> bool:
    try:
        ok = bool(check())
    except Exception as e:  # a failed check scores 0, it never aborts the run
        ok = False
        name += f" ({e})"
    print(f"{'PASS' if ok else 'FAIL'}  {kind + ' ' if kind else ''}{name}")
    return ok


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--reward", required=True)
    reward_path = ap.parse_args().reward

    gated = all([run("GATE", name, check) for name, check in gates()])
    results = [run("", name, check) for name, check in checks()]
    passed, total = sum(results), len(results)

    reward = passed / total if gated and total else 0.0
    print(f"reward = {reward:.3f} ({passed}/{total} checks{'' if gated else ', gate failed'})")
    with open(reward_path, "w") as f:
        f.write(f"{reward}\n")
    return 0 if gated and passed == total else 1


if __name__ == "__main__":
    sys.exit(main())
