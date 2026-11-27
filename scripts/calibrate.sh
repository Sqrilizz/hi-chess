#!/usr/bin/env bash
set -euo pipefail

stockfish="${1:-engines/Stockfish/src/stockfish}"
games="${2:-10}"
time_control="${3:-5+0.05}"
output_dir="${4:-matches/calibration}"
hi_bin="${HI_BIN:-target/release/hi}"
model="${HI_MODEL:-models/hi-v2.bpk}"
ratings=(800 1100 1400 1700 2100)

if [[ ! -x "$stockfish" ]]; then
  printf 'Stockfish executable not found: %s\n' "$stockfish" >&2
  exit 1
fi
if [[ ! -x "$hi_bin" ]]; then
  printf 'HI executable not found: %s\n' "$hi_bin" >&2
  exit 1
fi
if [[ ! -f "$model" ]]; then
  printf 'HI model not found: %s\n' "$model" >&2
  exit 1
fi
if [[ ! "$games" =~ ^[1-9][0-9]*$ ]]; then
  printf 'Games must be a positive integer\n' >&2
  exit 1
fi

mkdir -p "$output_dir"

for index in "${!ratings[@]}"; do
  level=$((index + 1))
  rating="${ratings[$index]}"
  "$hi_bin" match \
    --opponent "$stockfish" \
    --level "$level" \
    --games "$games" \
    --tc "$time_control" \
    --hi-elo "$rating" \
    --opponent-elo "$rating" \
    --opponent-option 'Threads=1' \
    --opponent-option 'UCI_LimitStrength=true' \
    --opponent-option "UCI_Elo=$rating" \
    --output "$output_dir/level${level}.tsv" \
    >"$output_dir/level${level}.log" 2>&1
done

printf 'Calibration complete: %s\n' "$output_dir"
