#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
mkdir -p data models benchmarks

fifo_dir=$(mktemp -d "${TMPDIR:-/tmp}/hi-v3.XXXXXX")
curl_pids=()
cleanup() {
    for pid in "${curl_pids[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    rm -rf "$fifo_dir"
}
trap cleanup EXIT

inputs=()
for month in 07 08 09 10; do
    fifo="$fifo_dir/$month.zst"
    mkfifo "$fifo"
    curl --fail --location --retry 3 --silent \
        "https://database.lichess.org/standard/lichess_db_standard_rated_2026-${month}.pgn.zst" \
        > "$fifo" &
    curl_pids+=("$!")
    inputs+=(--input "$fifo")
done

./target/release/hi prepare "${inputs[@]}" \
    --train data/train-v3.bin \
    --validation data/validation-v3.bin \
    --max-train 2400000 \
    --max-validation 240000

for pid in "${curl_pids[@]}"; do
    wait "$pid" 2>/dev/null || true
done

./target/release/hi train \
    --train data/train-v3.bin \
    --validation data/validation-v3.bin \
    --output models/hi-v3.bpk \
    --epochs 4 \
    --batch-size 128 \
    --learning-rate 0.001 \
    --device discrete \
    | tee benchmarks/v3-training.log
