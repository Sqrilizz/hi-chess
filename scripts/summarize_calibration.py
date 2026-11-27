#!/usr/bin/env python3
import re
import sys
from pathlib import Path

root = Path(sys.argv[1] if len(sys.argv) > 1 else "matches/calibration")
rows = []

for level in range(1, 6):
    path = root / f"level{level}.log"
    if not path.is_file():
        raise SystemExit(f"Missing calibration log: {path}")
    text = path.read_text(encoding="utf-8")
    values = {}
    patterns = {
        "games": r"^games: (\d+)$",
        "wins": r"^HI wins: (\d+)$",
        "draws": r"^draws: (\d+)$",
        "losses": r"^HI losses: (\d+)$",
        "score": r"^score: ([\d.]+)%$",
        "opponent_elo": r"^opponent Elo input: (\d+)$",
        "elo": r"^HI Elo estimate: ([\d.]+) -> ([\d.]+)$",
    }
    for key, pattern in patterns.items():
        match = re.search(pattern, text, re.MULTILINE)
        if match is None:
            raise SystemExit(f"Missing {key} in {path}")
        values[key] = match.groups()
    rows.append(
        (
            level,
            values["opponent_elo"][0],
            values["games"][0],
            f'{values["wins"][0]}-{values["draws"][0]}-{values["losses"][0]}',
            values["score"][0],
            values["elo"][1],
        )
    )

print("| Level | Opponent Elo | Games | W-D-L | Score | HI Elo estimate |")
print("|---:|---:|---:|---:|---:|---:|")
for row in rows:
    print("| {} | {} | {} | {} | {}% | {} |".format(*row))
