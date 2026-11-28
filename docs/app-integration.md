# HI application integration

HI is a local UCI process. The desktop application should own the board, clocks and UI; HI owns move generation, search, model inference, review and personality text.

## Binary and model layout

```text
hi
models/hi-v3-mixed.bpk
assets/hi-logo.jpg
assets/openings.fen
assets/review-example.json
```

The Linux binary expects the model path relative to the application working directory unless `ModelPath` is set to an absolute path.

Neural inference uses wgpu/Vulkan. A Vulkan-capable integrated or discrete GPU is recommended. If the model cannot be loaded, HI reports the error and falls back to pure search.

## UCI session

Send commands on stdin and read newline-delimited responses on stdout.

```text
uci
isready
setoption name ModelPath value models/hi-v3-mixed.bpk
setoption name Level value 3
setoption name OpponentElo value 1500
isready
position startpos moves e2e4 e7e5
go movetime 1000
```

Wait for `uciok` after `uci` and `readyok` after each `isready`. After `go`, consume `info` lines until `bestmove`. The application must keep stdin open while a search is active.

Supported levels:

| Level | Intended use |
|---:|---|
| 1 | Beginner-like top-3 policy sampling with depth-3 safety search |
| 2 | Developing player, top-6 policy sampling with depth-3 safety search |
| 3 | Policy-guided search over 8 candidates |
| 4 | Deeper policy-guided search over 8 candidates |
| 5 | Full policy-guided search over 64 candidates |

These are profiles, not guaranteed Elo ratings.

## Game review

After a game, pass the complete UCI move list to the review command:

```bash
hi review \
  --moves "e2e4 e7e5 g1f3 b8c6 f1b5 a7a6" \
  --depth 8 \
  --player white
```

Use `--fen` when the game did not start from the standard position. The command writes one JSON object to stdout and diagnostics to stderr on failure.

Important fields:

| Field | Meaning |
|---|---|
| `moves[].label` | `brilliant`, `great`, `best`, `good`, `inaccuracy`, `mistake` or `blunder` |
| `moves[].best_move` | Engine best move in UCI notation |
| `moves[].alternatives` | Top three analyzed moves with white-perspective scores |
| `moves[].centipawn_loss` | Score lost by the played move |
| `moves[].win_probability_before/after` | White win probability around the move |
| `moves[].evaluation_before/after` | White-perspective evaluation graph values |
| `moves[].missed_win` | Whether a winning continuation was missed |
| `accuracy.white/black` | Estimated accuracy for each side |
| `result` | Final result when known |
| `closing_line` | Original HI closing comment |

`assets/review-example.json` contains a real generated review of Fool's Mate.

The default depth is 8. Lower depths are faster but less reliable. The classifier is heuristic and should be presented as HI's opinion, not as an absolute judgment.

## Personality lines

```bash
hi say --moment blunder --variation 1
```

Supported moments:

- `game-start`
- `thinking`
- `brilliant`
- `best`
- `good`
- `inaccuracy`
- `mistake`
- `blunder`
- `victory`
- `defeat`
- `draw`

`variation` selects one of several original lines. The application should rate-limit reactions and avoid playing a line for every move.

## Avatar states

A simple mapping can be derived from the same moments:

| Moment | Suggested avatar state |
|---|---|
| `game-start`, `good`, `best` | attentive |
| `thinking` | thinking |
| `brilliant` | impressed |
| `inaccuracy`, `mistake` | concerned |
| `blunder` | shocked |
| `victory` | pleased |
| `defeat` | respectful |
| `draw` | neutral |

The current logo can be reused for the first prototype. Animation and voice are application-layer features and are not required by the engine.

## Process lifecycle

- Spawn one HI process per board or match.
- Send `ucinewgame` before a new game.
- Send `stop` only when interrupting an active search.
- Send `quit` when closing the application.
- Treat a closed stdout or a nonzero exit code as an engine failure, not as a move.
- Do not expose arbitrary shell commands through the frontend.
