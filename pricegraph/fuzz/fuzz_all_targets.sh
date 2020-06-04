#!/bin/bash
# Usage fuzz_all_targets [time_per_target = 300s]

TIME_PER_TARGET=${1:-300}

while [ $? -eq 0 ]; do 
  # Fuzz each target sequentially, code 255 causes xargs to exit early when first fuzzing target fails
  cargo fuzz list | xargs -L1 -I{} cargo +nightly fuzz run {} -- -max_total_time=$TIME_PER_TARGET || exit 255
done