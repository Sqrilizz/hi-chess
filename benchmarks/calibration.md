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

## Transposition table experiment

HI 0.4.0 added a 262,144-entry transposition table. Fixed-depth `startpos` search at depth 7:

| Version | Nodes | Time | NPS | Score | Best move |
|---|---:|---:|---:|---:|---|
| 0.3.1 | 3,749,818 | 846 ms | 4,432,408 | -77 | a2a4 |
| 0.4.0 | 2,129,432 | 504 ms | 4,225,063 | -77 | a2a4 |

The table reduced searched nodes by 43.2% and elapsed time by 40.4% while preserving the result on this position.

Reproducible three-position benchmark on HI 0.4.0:

```text
startpos: depth 7 nodes 2129432 time 503 ms nps 4233463 score -77 bestmove a2a4
kiwipete: depth 7 nodes 2301550 time 512 ms nps 4495214 score -122 bestmove e5f7
endgame: depth 7 nodes 161718 time 25 ms nps 6468720 score 46 bestmove a5a4
total: nodes 4592700 time 1041 ms nps 4411815
```

Match impact at 5+0.05 over 20 games:

| Level | Opponent | Before TT | With TT |
|---:|---:|---:|---:|
| 3 | Stockfish 1400 | 3-3-14, 22.5% | 2-4-14, 20.0% |
| 5 | Stockfish 2100 | 1-0-19, 5.0% | 1-0-19, 5.0% |

The table is a clear efficiency improvement, but this small match sample does not show a strength gain.

## Principal variation search

HI 0.4.1 added PVS to internal alpha-beta nodes. Fixed-depth results:

| Position | 0.4.0 nodes | 0.4.1 nodes | 0.4.0 time | 0.4.1 time |
|---|---:|---:|---:|---:|
| startpos | 2,129,432 | 1,692,880 | 503 ms | 426 ms |
| kiwipete | 2,301,550 | 2,300,602 | 512 ms | 505 ms |
| endgame | 161,718 | 156,369 | 25 ms | 24 ms |
| total | 4,592,700 | 4,149,851 | 1041 ms | 956 ms |

Scores and best moves were unchanged on all three positions.

HI 0.4.2 extended PVS to the root for pure-search mode while keeping exact candidate scores for hybrid levels:

| Position | 0.4.1 nodes | 0.4.2 nodes | 0.4.1 time | 0.4.2 time |
|---|---:|---:|---:|---:|
| startpos | 1,692,880 | 1,520,228 | 426 ms | 389 ms |
| kiwipete | 2,300,602 | 2,299,861 | 505 ms | 509 ms |
| endgame | 156,369 | 144,970 | 24 ms | 23 ms |
| total | 4,149,851 | 3,965,059 | 956 ms | 923 ms |

Scores and best moves were unchanged.

## Rejected null-move experiment

A guarded null-move prototype reduced the three-position depth-7 total from 4,149,851 to 2,259,254 nodes, but changed scores and best moves on two positions. Match results were mixed:

| Level | Opponent | TT + PVS | With null-move |
|---:|---:|---:|---:|
| 3 | Stockfish 1400 | 2-4-14, 20.0% | 2-6-12, 25.0% |
| 5 | Stockfish 2100 | 1-0-19, 5.0% | 0-0-20, 0.0% |

The prototype was rejected and is not part of HI 0.4.1.

## Next work

- tune transposition-table size and replacement policy;
- investigate the White-only win pattern;
- tune level candidate counts and policy weights;
- run larger matches with more opening positions.
