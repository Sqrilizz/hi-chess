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

## Rejected check-extension experiment

A bounded check-extension prototype increased the three-position depth-7 total from 3,965,059 to 4,652,702 nodes and time from 923 ms to 1133 ms. Match results did not improve:

| Level | Opponent | TT + PVS | With check extensions |
|---:|---:|---:|---:|
| 3 | Stockfish 1400 | 20.0% in the TT sample | 3-3-14, 22.5% |
| 5 | Stockfish 2100 | 1-0-19, 5.0% | 1-0-19, 5.0% |

The prototype was rejected and is not part of HI 0.4.2.

## Low-level safety search

HI 0.5.0 added shallow safety search to Levels 1 and 2. The model still samples human-like moves, but only among candidates that pass a bounded search filter.

| Level | Opponent | Before | With safety search |
|---:|---:|---:|---:|
| 1 | Stockfish 800 | 0-0-20, 0.0% | 2-0-18, 10.0% |
| 2 | Stockfish 1100 | 0-0-20, 0.0% | 2-1-17, 12.5% |

The safety filter is deliberately wider for Level 1 so it remains more error-prone than Level 2.

## Level 4 profile tuning

Level 4 was changed from 16 candidates at depth 8 with policy weight 10 to 8 candidates at depth 6 with policy weight 20. Against Stockfish 1700 the result remained 1-0-19, 5.0%, rough HI Elo 1499.6.

The profile was kept because it spends search time more deliberately, but the match does not prove a strength change. The rough Elo ordering remains monotonic: Level 3 ~1280, Level 4 ~1500, Level 5 ~1900.

## `hi-v2` model

The default model was retrained on 960,000 training positions and evaluated on 96,000 validation positions:

| Metric | `hi-v1` | `hi-v2` |
|---|---:|---:|
| top-1 | 33.59% | 37.46% |
| top-3 | 58.73% | 63.83% |
| WDL accuracy | 61.97% | 62.72% |
| validation loss | 2.5764 | 2.3525 |

`hi-v2` is the default model starting with HI 0.6.0.

## `hi-v2` level calibration

Twenty games per level at 5+0.05 against strength-limited Stockfish:

| Level | Opponent Elo | W-D-L | Score | Rough HI Elo |
|---:|---:|---:|---:|---:|
| 1 | 800 | 1-0-19 | 5.0% | 599.6 |
| 2 | 1100 | 2-0-18 | 10.0% | 928.3 |
| 3 | 1400 | 8-4-8 | 50.0% | 1391.2 |
| 4 | 1700 | 1-0-19 | 5.0% | 1499.6 |
| 5 | 2100 | 1-0-19 | 5.0% | 1899.6 |

Level 3 improved dramatically with the larger model. Levels 1 and 2 were retuned to lower sampling temperature and narrower safety windows; Level 2 improved from 0% to 10%, while Level 1 remained at 5%.

## Rejected multi-threaded search experiment

A root-parallel search with per-worker heuristics and a shared transposition table reached about 11–12M aggregate NPS with six threads. Fixed-depth time was not consistently better because parallel root windows duplicated work. In a 20-game Level 5 match against Stockfish 2100, six threads scored the same 1-0-19, 5.0% as one thread.

The experiment was rejected and is not part of the stable engine.

## Rejected late-move-reduction experiment

Late move reductions reached depth 9 in about 0.52 seconds and depth 10 in about 1.97 seconds from the starting position, versus depth 8 before. However, match strength regressed:

| Level | Opponent | Stable | With LMR |
|---:|---:|---:|---:|
| 3 | Stockfish 1400 | 8-4-8, 50.0% | 4-5-11, 32.5% |
| 5 | Stockfish 2100 | 1-0-19, 5.0% | 0-0-20, 0.0% |

The aggressive reduction policy was rejected.

## Rejected expanded-evaluation experiment

A tapered evaluation with pawn structure, rook files and king safety reduced search speed by roughly 20% and did not improve match results:

| Level | Opponent | Stable | Expanded evaluation |
|---:|---:|---:|---:|
| 3 | Stockfish 1400 | 8-4-8, 50.0% | 4-5-11, 32.5% |
| 5 | Stockfish 2100 | 1-0-19, 5.0% | 0-0-20, 0.0% |

The evaluation experiment was rejected.

## Policy-guided full search

HI 0.9.0 uses the `hi-v2` policy to order root candidates for every level. Level 5 now searches the 32 most likely model moves with full depth and a small policy bonus instead of ignoring the neural model.

| Level | Opponent | Before policy guidance | With policy guidance |
|---:|---:|---:|---:|
| 3 | Stockfish 1400 | 8-4-8, 50.0% | 11-2-7, 60.0% |
| 5 | Stockfish 2100 | 1-0-19, 5.0% | 7-1-12, 37.5% |

Rough HI Elo improved from 1391 to 1439 on Level 3 and from 1900 to 2037 on Level 5.

## Final HI 0.9.0 calibration

Twenty games per level at 5+0.05:

| Level | Opponent Elo | W-D-L | Score | Rough HI Elo |
|---:|---:|---:|---:|---:|
| 1 | 800 | 1-1-18 | 7.5% | 610.4 |
| 2 | 1100 | 2-1-17 | 12.5% | 927.0 |
| 3 | 1400 | 8-3-9 | 47.5% | 1398.5 |
| 4 | 1700 | 6-0-14 | 30.0% | 1622.0 |
| 5 | 2100 | 9-0-11 | 45.0% | 2065.9 |

The rough Elo estimates are monotonic across all five levels. Level 5 won nine games against Stockfish limited to 2100.

## `hi-v3` model and policy-orientation fix

HI 0.10.0 trains policy targets and legal-move masks in the same side-to-move orientation as the input planes. Previously the input was vertically mirrored for Black while policy indices remained absolute, so identical oriented positions could require opposite output coordinates. Dataset format v2 stores oriented indices; format v1 is migrated on read. Existing v2 weights retain their original absolute-coordinate inference path.

The v3 CNN uses 64 channels, 8 residual blocks and a 64-to-4-channel policy bottleneck. It was trained on 2,400,000 positions from 115,988 rated games played between July and October 2026, with 240,000 held-out validation positions.

| Epoch | Validation loss | Top-1 | Top-3 | WDL | White top-1 | Black top-1 |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 2.2012 | 40.26% | 67.23% | 54.71% | 39.96% | 40.55% |
| 2 | 2.1189 | 42.13% | 69.52% | 55.08% | 41.90% | 42.36% |
| 3 | 2.0768 | 42.97% | 70.55% | 55.20% | 42.67% | 43.27% |
| 4 | 2.0558 | 43.71% | 71.33% | 55.29% | 43.39% | 44.04% |

The v2 and v3 validation metrics use different datasets, so they are not a controlled accuracy comparison. Match results below provide the playing-strength comparison. The v3 model SHA256 is `5dda0c2073dc7659318fa807474f51f2cb8f984131aad83b2b67de6cd61673a5`.

## HI 0.10.0 calibration

Twenty games per level at 5+0.05 against the same strength-limited Stockfish targets used for HI 0.9.0. These initial v3 matches used the original 32-candidate, weight-5 Level 5 profile; the extended final-profile result is below:

| Level | Opponent | HI 0.9.0 W-D-L | HI 0.10.0 W-D-L | HI 0.9.0 score | HI 0.10.0 score | HI 0.10.0 rough Elo |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 800 | 1-1-18 | 1-4-15 | 7.5% | 15.0% | 652.7 |
| 2 | 1100 | 2-1-17 | 7-0-13 | 12.5% | 35.0% | 1009.4 |
| 3 | 1400 | 8-3-9 | 13-6-1 | 47.5% | 80.0% | 1531.6 |
| 4 | 1700 | 6-0-14 | 9-3-8 | 30.0% | 52.5% | 1731.0 |
| 5 | 2100 | 9-0-11 | 9-1-10 | 45.0% | 47.5% | 2093.2 |

All five rough Elo estimates increased and remained monotonic. The largest gains were on Levels 1–4.

A direct Level 5 duel between v3 as HI and v2 as the opponent used 40 games at 5+0.05 with alternating colors and paired openings. V3 scored 15-11-14, 51.2%, roughly +10 Elo. This is not statistically decisive at 40 games.

Color splits in the 20-game Stockfish calibration:

| Level | White W-D-L | Black W-D-L |
|---:|---:|---:|
| 1 | 0-2-8 | 1-2-7 |
| 2 | 5-0-5 | 2-0-8 |
| 3 | 9-1-0 | 4-5-1 |
| 4 | 6-2-2 | 3-1-6 |
| 5 | 5-1-4 | 4-0-6 |

V3 wins with both colors at every level except Level 1, where its only win came with Black. The earlier White-only win pattern is no longer present.

## Level 5 extended calibration

One hundred games per model at 5+0.05 against Stockfish limited to 2100:

| Model | W-D-L | Score | Rough HI Elo | White W-D-L | Black W-D-L |
|---|---:|---:|---:|---:|---:|
| v2, 32 candidates / weight 5 | 36-7-57 | 39.5% | 2037.5 | 26-2-22 | 10-5-35 |
| v3, 32 candidates / weight 5 | 40-9-51 | 44.5% | 2061.8 | 30-5-15 | 10-4-36 |
| v3, 64 candidates / weight 2 | 46-5-49 | 48.5% | 2073.9 | 29-4-17 | 17-1-32 |

Widening the Level 5 candidate set from 32 to 64 and reducing the policy weight from 5 to 2 improved the v3 result by four percentage points over its initial profile and by nine points over v2. This is the final HI 0.10.0 Level 5 configuration. The Black-side deficit appears with both models against this particular strength-limited Stockfish matchup and is not introduced by the v3 orientation fix; the final profile reduced it substantially.

A 100-game direct duel between the initial v3 Level 5 profile and v2 scored 32-34-34, 49.0% for v3. Both engines shared the same GPU, so the larger v3 model competes for inference time with v2; the separate Stockfish matches are the cleaner playing-strength comparison.

## Mixed-data v3 fine-tune

A v3 checkpoint was fine-tuned for two epochs at learning rate 0.0002 with a fresh Adam optimizer. The training set contained 1,800,000 positions sampled evenly from July 2018, September 2026 and October 2026; validation contained 180,000 positions from the same three archives.

| Validation | Model | Loss | Top-1 | Top-3 | WDL | White top-1 | Black top-1 |
|---|---|---:|---:|---:|---:|---:|---:|
| Mixed | Original v3 | 2.0027 | 44.83% | 72.24% | 56.85% | 44.54% | 45.12% |
| Mixed | Fine-tuned v3 | 1.9480 | 46.13% | 73.51% | 57.60% | 45.84% | 46.42% |
| Modern 2026 | Original v3 | 2.0558 | 43.71% | 71.33% | 55.29% | 43.39% | 44.04% |
| Modern 2026 | Fine-tuned v3 | 1.9694 | 45.64% | 73.18% | 56.30% | 45.26% | 46.03% |

The fine-tuned model improved every reported metric on both validation sets. Its SHA256 is `5565ac9af2ca46c6d9e5b740973cead7aaa8c86cc9a5604fa5fc560d7a749ff6`.

### Mixed-model match results

One hundred Level 5 games at 5+0.05 on the expanded 59-position opening suite:

| Match | W-D-L | Score | Rough HI Elo |
|---|---:|---:|---:|
| Original v3 vs Stockfish 2100 | 38-4-58 | 40.0% | 1959.2 |
| Mixed v3 vs Stockfish 2100 | 44-7-49 | 47.5% | 2162.3 |
| Mixed v3 vs original v3 | 44-30-26 | 59.0% | 2177.9 |

Against Stockfish, the mixed model improved White from 19-2-29 to 20-3-27 and Black from 19-2-29 to 24-4-22.

The mixed model was also calibrated across all five levels with 100 games per level on the expanded opening suite:

| Level | Opponent | W-D-L | Score | Rough HI Elo |
|---:|---:|---:|---:|---:|
| 1 | 800 | 14-12-74 | 20.0% | 540.0 |
| 2 | 1100 | 24-6-70 | 27.0% | 901.4 |
| 3 | 1400 | 58-13-29 | 64.5% | 1555.2 |
| 4 | 1700 | 34-9-57 | 38.5% | 1653.2 |
| 5 | 2100 | 44-7-49 | 47.5% | 2162.3 |

The rough Elo estimates remain monotonic. The final table uses the tuned Level 1 and Level 2 profiles.

### Weak-level safety-search A/B

One hundred games per configuration on the expanded opening suite:

| Level | Profile | W-D-L | Score |
|---:|---|---:|---:|
| 1 | Depth 2, top 6 | 10-7-83 | 13.5% |
| 1 | Depth 3, top 4 | 12-3-85 | 13.5% |
| 1 | Depth 3, top 3, temperature 0.9 | 14-12-74 | 20.0% |
| 2 | Depth 2, top 8 | 20-7-73 | 23.5% |
| 2 | Depth 3, top 6 | 24-6-70 | 27.0% |

The final profiles are depth 3 with top 3 for Level 1 and top 6 for Level 2. They remain deliberately weaker than the search-based levels.

## HI 0.12 candidate experiments

The stable HI 0.11.0 configuration was challenged with direct A/B matches whenever possible. Candidate scores below are from the candidate's perspective; the current stable profile scored the complement.

| Candidate | Comparison | Candidate score | Decision |
|---|---|---:|---|
| v3.2 model | Direct Level 5 duel vs mixed-v3 | 49.0% | Rejected |
| TT 1M | 200-game direct Level 5 duel vs TT 256K | 49.8% | Rejected |
| Clock divisor 10 | 200-game direct Level 5 duel vs divisor 20 | 45.0% | Rejected |
| Level 5 policy weight 0 | 200-game direct duel vs weight 2 | 0.5% | Rejected |
| Level 5 policy weight 3 | 200-game direct duel vs weight 2 | 49.3% | Rejected |
| Level 5 policy weight 5 | 200-game direct duel vs weight 2 | 40.0% | Rejected |
| Level 5 128 candidates | 200-game direct duel vs 64 candidates | 49.0% | Rejected |
| Level 1 top-2 | 200-game direct duel vs top-3 | 40.5% | Rejected |
| Level 2 top-4 | 200-game direct duel vs top-6 | 44.8% | Rejected |

Mate distance pruning did not change node counts or fixed-depth benchmark time and was reverted without matches. Aspiration windows, quiescence delta pruning and SEE had already been rejected in earlier experiments.

Separate 100-game matches against strength-limited Stockfish showed substantial variance, with Level 5 scores ranging from 38.5% to 53.0% across closely related configurations. Those matches are useful as calibration samples but were not treated as decisive proof for small changes. Direct A/B matches with identical models and settings were preferred.

No candidate from this series produced a confirmed playing-strength improvement, so HI 0.11.0 remains the stable engine and model configuration.

## Next work

- run larger matches with still more opening positions;
- tune Level 3 candidate count and policy weight toward its advertised target;
- improve WDL accuracy with additional training;
- tune transposition-table size and replacement policy;
- integrate move review, avatar states and personality lines into the desktop coach application.
