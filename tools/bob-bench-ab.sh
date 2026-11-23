#!/bin/zsh
# Three alternating release A/B pairs. Binaries must be built from the stated revisions.
# Usage: bob-bench-ab.sh BASE_BIN NEW_BIN NAME
# BOB_TRACE_DIR contains a private session.bin; all evidence stays there.
set -euo pipefail
ROOT=${0:A:h:h}
BASE=${1:A}
NEW=${2:A}
NAME=$3
DIR=${BOB_TRACE_DIR:?Set BOB_TRACE_DIR to the private trace directory}
DIR=${DIR:A}
export BOB_TRACE_DIR=$DIR
[[ -x $BASE && -x $NEW && -s $DIR/session.bin ]]
HOME_RUN=$(mktemp -d -t bob-bench)
export HORIZON_HOME=$HOME_RUN/runtime HORIZON_LAUNCHER_HOME=$HOME_RUN/launcher
export XDG_DATA_HOME=$HOME_RUN/data XDG_CONFIG_HOME=$HOME_RUN/config
export HORIZON_NO_AUDIO=1 HORIZON_LAUNCHER_NO_SELF_UPDATE=1
mkdir -p "$HORIZON_HOME" "$HORIZON_LAUNCHER_HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME"
# Relative defaults must be resolved before switching to the scratch game directory.
export TRACE=${TRACE:-${CARGO_TARGET_DIR:-$ROOT/target}/aarch64-apple-darwin/release/horizon-189-trace}
TRACE=${TRACE:A}
cd "$HOME_RUN"
run_pair_side() {
  local side=$1 number=$2 binary=$3
  BIN=$binary zsh "$ROOT/tools/bob-trace-bench.sh" "$NAME-$side$number" > "$DIR/$NAME-$side$number.summary.log" 2>&1
}
for number in 1 2 3; do
  if (( number == 2 )); then
    run_pair_side new $number "$NEW"
    run_pair_side base $number "$BASE"
  else
    run_pair_side base $number "$BASE"
    run_pair_side new $number "$NEW"
  fi
  echo "BENCH_PAIR_DONE $number"
done
python3 "$ROOT/tools/bob_bench_compare.py" \
  --pair "$DIR/bench-$NAME-base1.log" "$DIR/bench-$NAME-new1.log" \
  --pair "$DIR/bench-$NAME-base2.log" "$DIR/bench-$NAME-new2.log" \
  --pair "$DIR/bench-$NAME-base3.log" "$DIR/bench-$NAME-new3.log"
