#!/bin/bash
# Usage fuzz_all_targets [time_per_target = 300s] [rss_limit = 2048Mb]

TIME_PER_TARGET=${1:-300}
RSS_LIMIT=${2:-2048}

while [ $? -eq 0 ]; do 
  # Fuzz each target sequentially, code 255 causes xargs to exit early when first fuzzing target fails
  cargo fuzz list | xargs -L1 -I{} cargo +nightly fuzz run {} -- -max_total_time=$TIME_PER_TARGET -rss_limit_mb=$RSS_LIMIT || exit 255
done
