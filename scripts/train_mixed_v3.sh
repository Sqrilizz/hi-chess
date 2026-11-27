#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
mkdir -p data models benchmarks

fifo_dir=$(mktemp -d "${TMPDIR:-/tmp}/hi-v3-mixed.XXXXXX")
curl_pids=()
cleanup() {
    for pid in "${curl_pids[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    rm -rf "$fifo_dir"
}
trap cleanup EXIT

inputs=()
for month in 2018-07 2026-09 2026-10; do
    fifo="$fifo_dir/$month.zst"
    mkfifo "$fifo"
    curl --fail --location --retry 3 --silent \
        "https://database.lichess.org/standard/lichess_db_standard_rated_${month}.pgn.zst" \
        > "$fifo" &
    curl_pids+=("$!")
    inputs+=(--input "$fifo")
done

./target/release/hi prepare "${inputs[@]}" \
    --train data/train-mixed-v3.bin \
    --validation data/validation-mixed-v3.bin \
    --max-train 1800000 \
    --max-validation 180000

for pid in "${curl_pids[@]}"; do
    wait "$pid" 2>/dev/null || true
done

./target/release/hi evaluate \
    --model models/hi-v3.bpk \
    --validation data/validation-mixed-v3.bin \
    --device discrete \
    | tee benchmarks/v3-mixed-baseline.log

./target/release/hi train \
    --train data/train-mixed-v3.bin \
    --validation data/validation-mixed-v3.bin \
    --output models/hi-v3-mixed.bpk \
    --resume models/hi-v3.bpk \
    --epochs 2 \
    --batch-size 128 \
    --learning-rate 0.0002 \
    --device discrete \
    | tee benchmarks/v3-mixed-training.log

./target/release/hi evaluate \
    --model models/hi-v3-mixed.bpk \
    --validation data/validation-mixed-v3.bin \
    --device discrete \
    | tee benchmarks/v3-mixed-final.log
