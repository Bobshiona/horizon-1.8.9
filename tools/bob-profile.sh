#!/bin/zsh
# Profile Bob Client on the deterministic packet-trace session (docs/sprint-queue.md, 1.1).
#
#   tools/bob-profile.sh time|metal|alloc|game NAME [client flags...]
#
# Replays the same session as tools/bob-trace-bench.sh (80-player crowd, lobby
# and flyover phases, render distance 16) under Instruments from the command
# line: `time` is the Time Profiler, `metal` the Metal System Trace, `alloc`
# Allocations and `game` Game Performance. Writes $BOB_TRACE_DIR/profile-NAME.trace
# (open it in Instruments for the full view) and prints a summary from
# tools/bob-profile-summary.py, from 4 s on so the login and first chunks are left out.
#
# BOB_TRACE_DIR  trace and outputs (default: target/bob-trace)
# BIN, TRACE     the client and trace tool (default: release builds in
#                $CARGO_TARGET_DIR, else target)
# PORT           replay port (default 25572)
# SECONDS_LIMIT  recording length (default 20)
set -u
kind=$1; name=$2; shift 2
case $kind in
  time) template='Time Profiler' ;;
  metal) template='Metal System Trace' ;;
  alloc) template='Allocations' ;;
  game) template='Game Performance' ;;
  *) echo "usage: $0 time|metal|alloc|game NAME [client flags...]" >&2; exit 2 ;;
esac
DIR=${BOB_TRACE_DIR:-target/bob-trace}
TARGET=${CARGO_TARGET_DIR:-target}/aarch64-apple-darwin/release
BIN=${BIN:-$TARGET/horizon-189-client}
TRACE=${TRACE:-$TARGET/horizon-189-trace}
PORT=${PORT:-25572}
OUT=$DIR/profile-$name
rm -rf $OUT.trace
"$TRACE" serve --listen 127.0.0.1:$PORT --trace $DIR/session.bin \
  --crowd 80 --crowd-at 1500 --crowd-until 15300 --seed 1 \
  --phase lobby@4000 --phase flyover@15300 > $OUT.serve.log 2>&1 &
SERVE=$!
sleep 0.5
xcrun xctrace record --template "$template" --output $OUT.trace \
  --time-limit ${SECONDS_LIMIT:-20}s --launch -- \
  "$BIN" --offline --username NativeBench --server 127.0.0.1 --port $PORT \
  --render-distance 16 --bench-trace "$@" > $OUT.log 2>&1
RECORD=$?
kill $SERVE 2>/dev/null
# xctrace exits non-zero when the time limit stops the client; the trace is still whole.
if [[ ! -d $OUT.trace ]]; then
  tail -5 $OUT.log
  echo "PROFILE_FAIL record=$RECORD"
  exit 1
fi
echo "PROFILE_TRACE $OUT.trace"
python3 "${0:A:h}/bob-profile-summary.py" $OUT.trace --from 4
