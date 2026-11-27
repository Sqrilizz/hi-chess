# HI level calibration

Date: 2026-11-27

## Setup

- HI version: 0.3.0, with Level 5 rechecked on 0.3.1.
- Model: `hi-v1`, best epoch 3.
- Opponent: official Stockfish commit `88337401`.
- Stockfish options: `Threads=1`, `UCI_LimitStrength=true`, `UCI_Elo=<target>`.
- Games per level: 20.
- Time control: 5 seconds + 0.05 seconds increment.
- Colors: alternating.
- Openings: `assets/openings.fen`, paired across colors.
- Draw adjudication: checkmate, stalemate, repetition, fifty-move rule, insufficient material or 300 plies.

Command:

```bash
./scripts/calibrate.sh engines/Stockfish/src/stockfish 20 5+0.05 matches/calibration
```

## Results

| Level | Target Elo | Opponent Elo | Games | W-D-L | Score | Rough HI Elo |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 800 | 800 | 20 | 0-0-20 | 0.0% | 582.1 |
| 2 | 1100 | 1100 | 20 | 0-0-20 | 0.0% | 882.1 |
| 3 | 1400 | 1400 | 20 | 3-3-14 | 22.5% | 1279.2 |
| 4 | 1700 | 1700 | 20 | 1-0-19 | 5.0% | 1499.6 |
| 5 | 2100 | 2100 | 20 | 1-0-19 | 5.0% | 1899.6 |

Level 5 was repeated after disabling unnecessary neural inference in HI 0.3.1. The result remained 1-0-19 with the same rough Elo.

## Observations

- Level 3 is currently the strongest relative profile against its matched Stockfish opponent.
- Levels 1 and 2 are too weak even against Stockfish limited to 800 and 1100.
- Level 4 underperforms Level 3 on this sample. More games are required before changing its profile.
- All HI wins occurred with White. Opening coverage, color-specific strategy and deterministic policy sampling need investigation.
- These numbers are not human Elo measurements. Stockfish's limited-strength setting is approximate, and 20 games per level is a small sample.

## Next work

- add a transposition table and improve search efficiency;
- investigate the White-only win pattern;
- tune level candidate counts and policy weights;
- run larger matches with more opening positions.
