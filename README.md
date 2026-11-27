<div align="center">
  <img src="assets/hi-logo.jpg" width="420" alt="HI chess engine logo">
</div>

# HI

[![CI](https://github.com/Sqrilizz/hi-chess/actions/workflows/ci.yml/badge.svg)](https://github.com/Sqrilizz/hi-chess/actions/workflows/ci.yml)

**Hi. Pick your level.**

HI is a Rust chess engine that combines Stockfish-style search with a Maia-inspired neural model of human play. It supports five playing levels, from beginner-like move selection to full search.

## Current status

Implemented:

- UCI protocol;
- legal move generation;
- `startpos`, FEN and move history;
- `perft` validation;
- material and positional evaluation;
- alpha-beta search with iterative deepening;
- quiescence search;
- move ordering, killer moves and history heuristic;
- transposition table with exact/lower/upper bounds;
- principal variation search for root and internal alpha-beta nodes;
- depth, node, time and clock limits;
- early `stop`;
- checkmate, stalemate, repetition, fifty-move rule and basic insufficient-material detection;
- a CNN policy + WDL model trained on human games;
- five levels: 1–2 use model sampling with shallow safety search, 3–4 combine the model with deeper search, and 5 use full search;
- a local UCI match runner for engine-vs-engine testing.

Not implemented yet:

- a dedicated promotion head;
- multi-position history beyond the last move;
- neural evaluation inside the search;
- multi-threaded search;
- opening book;
- precise Elo calibration.

## Build

```bash
cargo build --release
```

The executable is written to `target/release/hi`.

## Run as a UCI engine

```bash
./target/release/hi
```

Example session:

```text
uci
isready
position startpos moves e2e4 e7e5
go depth 5
stop
quit
```

After `go`, HI emits `info` lines followed by `bestmove`. The `stop` command ends the current search early.

### Neural level options

```text
setoption name UseModel value true
setoption name ModelPath value models/hi-v1.bpk
setoption name Level value 3
setoption name OpponentElo value 1500
```

| Level | Target profile | Behavior |
|---:|---:|---|
| 1 | ~800 | Samples from the human-move policy with a depth-2 safety filter |
| 2 | ~1100 | Policy sampling with a depth-2 safety filter |
| 3 | ~1400 | Searches the 8 most likely model candidates |
| 4 | ~1700 | Searches 16 candidates with a policy-weight bonus |
| 5 | ~2100 | Full search |

These are target profiles, not guaranteed Elo ratings. If the model is unavailable, HI reports the problem and falls back to Level 5 search.

Supported `go` parameters:

- `depth N`;
- `nodes N`;
- `movetime N` in milliseconds;
- `wtime`, `btime`, `winc`, `binc`, `movestogo`;
- `infinite`;
- `ponder`.

## Perft

```bash
./target/release/hi perft "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1" 4
```

Expected result: `197281`.

## Search benchmark

```bash
./target/release/hi bench --depth 7
```

Runs fixed-depth searches from the starting position, Kiwipete and an endgame position, then reports nodes, elapsed time and NPS.

## Dataset preparation

Source dataset: [Lichess Standard, December 2014](https://database.lichess.org/standard/lichess_db_standard_rated_2014-12.pgn.zst), licensed under CC0.

SHA256: `4589a1af622a893d196bc8eaede657652ce65dc79d2f289ff65fadd6a7076af4`.

Download it into `data/`, then run:

```bash
./target/release/hi prepare \
  --input data/lichess_db_standard_rated_2014-12.pgn.zst \
  --train data/train.bin \
  --validation data/validation.bin \
  --max-train 240000 \
  --max-validation 24000
```

The pipeline keeps ratings from 600 to 2200, skips the first 10 plies, samples up to 24 positions per game and splits complete games into train and validation sets in a 9:1 ratio.

Datasets and model weights are ignored by Git and must be generated locally.

## Train the model

```bash
./target/release/hi train \
  --train data/train.bin \
  --validation data/validation.bin \
  --output models/hi-v1.bpk \
  --epochs 6 \
  --batch-size 128 \
  --learning-rate 0.001 \
  --device discrete
```

The model is a compact Burn CNN with approximately 3.26 million parameters:

- 23 input channels;
- 12 internal channels;
- 4 residual blocks;
- 4096-class policy head;
- three-class WDL head.

Training and inference are implemented in Rust. GPU execution uses wgpu/Vulkan.

### `hi-v1` quality

Measured on 24,017 validation positions:

- best epoch: 3;
- top-1 human move accuracy: **33.59%**;
- top-3 human move accuracy: **58.73%**;
- WDL accuracy: **61.97%**;
- validation loss: 2.5764;
- model size: approximately 13 MB.

Later epochs began to overfit, so `models/hi-v1.bpk` contains the best epoch rather than the final one.

## Inspect model predictions

```bash
./target/release/hi predict \
  --model models/hi-v1.bpk \
  --fen "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1" \
  --elo 1500 \
  --opponent-elo 1600 \
  --device discrete
```

## Engine matches

HI can play local matches against any UCI engine, including Stockfish:

```bash
./target/release/hi match \
  --opponent engines/Stockfish/src/stockfish \
  --level 3 \
  --games 20 \
  --tc 10+0.1 \
  --hi-elo 1400 \
  --opponent-elo 1400 \
  --opponent-option 'UCI_LimitStrength=true' \
  --opponent-option 'UCI_Elo=1400' \
  --output matches/hi-level3-vs-stockfish1400.tsv
```

The runner alternates colors, pairs opening positions, manages both clocks, detects terminal positions and writes a TSV log with UCI moves. It reports wins, draws, losses, score and a rough Elo update. Small matches are not precise Elo measurements.

Useful options:

- `--level 1..5`;
- `--no-model` for pure-search Level 5;
- `--model <path>`;
- `--games <n>`;
- `--tc <seconds+increment>`;
- `--openings <fen-file>`;
- `--opponent-option Name=Value`;
- `--max-plies <n>`.

A default opening suite is included in `assets/openings.fen`. Engines, match logs, datasets and model weights are ignored by Git.

Example local Stockfish build:

```bash
git clone --depth 1 https://github.com/official-stockfish/Stockfish.git engines/Stockfish
make -C engines/Stockfish/src profile-build ARCH=x86-64-avx512 -j
```

Run all five level calibrations and print a Markdown summary:

```bash
./scripts/calibrate.sh engines/Stockfish/src/stockfish 20 5+0.05 matches/calibration
./scripts/summarize_calibration.py matches/calibration
```

The first 100-game calibration report is available in [benchmarks/calibration.md](benchmarks/calibration.md).

## Verification

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Project structure

- `src/uci.rs` — UCI protocol and search management;
- `src/search.rs` — alpha-beta, PVS, quiescence and search limits;
- `src/transposition.rs` — transposition table;
- `src/evaluation.rs` — material and positional evaluation;
- `src/move_order.rs` — move ordering;
- `src/perft.rs` — move-generation validation;
- `src/features.rs` — position, rating and policy encoding;
- `src/dataset.rs` — binary training-record format;
- `src/prepare.rs` — PGN/Zstd to train/validation conversion;
- `src/network.rs` — Burn policy + WDL CNN;
- `src/train.rs` — training and validation;
- `src/inference.rs` — model loading and prediction;
- `src/hybrid.rs` — policy/search candidate selection;
- `src/level.rs` — playing-level profiles;
- `src/options.rs` — UCI options;
- `src/arena.rs` — local UCI matches, clocks, results and Elo;
- `src/bench.rs` — fixed-depth search benchmark;
- `src/uci_client.rs` — UCI process client;
- `src/main.rs` — CLI and UCI entry point;
- `scripts/calibrate.sh` — five-level Stockfish calibration;
- `scripts/summarize_calibration.py` — Markdown report generator.

Move generation uses the MIT-licensed [`chess`](https://github.com/jordanbray/chess) crate. Search, evaluation, UCI, data preparation and the neural network are implemented in this project. Because the project uses `pgn-reader`, HI is distributed under GPL-3.0-or-later.

## Roadmap

- calibrate levels with engine matches;
- add a dedicated promotion head;
- extend position history;
- enlarge the training dataset;
- add neural evaluation to the search;
- tune the transposition table and add multi-threaded search;
- run larger calibrated matches for every HI level.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
