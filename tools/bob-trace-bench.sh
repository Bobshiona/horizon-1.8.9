#!/bin/zsh
# Bob Client's deterministic packet-trace benchmark (docs/sprint-queue.md, 1.1).
#
#   tools/bob-trace-bench.sh NAME [client flags...]
#
# Replays $BOB_TRACE_DIR/session.bin (recorded with tools/bob-trace-record.sh)
# through `horizon-189-trace serve` with an 80-player fighting crowd from 1.5 s
# to 15.3 s, phase markers lobby@4000 and flyover@15300, render distance 16,
# and runs the release client with --bench-trace. Prints one BOBTRACE_RESULT
# line per phase and the client's resident memory. Run three times and
# compare medians; see HANDOFF.md for the baseline and its noise.
#
# BOB_TRACE_DIR  trace and outputs (default: target/bob-trace)
# BIN, TRACE     the client and trace tool (default: release builds in
#                $CARGO_TARGET_DIR, else target)
# PORT           replay port (default 25571)
set -u
name=$1; shift
DIR=${BOB_TRACE_DIR:-target/bob-trace}
TARGET=${CARGO_TARGET_DIR:-target}/aarch64-apple-darwin/release
BIN=${BIN:-$TARGET/horizon-189-client}
TRACE=${TRACE:-$TARGET/horizon-189-trace}
PORT=${PORT:-25571}
OUT=$DIR/bench-$name
"$TRACE" serve --listen 127.0.0.1:$PORT --trace $DIR/session.bin \
  --crowd 80 --crowd-at 1500 --crowd-until 15300 --seed 1 \
  --phase lobby@4000 --phase flyover@15300 --log $OUT.client.bin > $OUT.serve.log 2>&1 &
SERVE=$!
sleep 0.5
"$BIN" --offline --username NativeBench --server 127.0.0.1 --port $PORT \
  --render-distance 16 --bench-trace "$@" > $OUT.log 2>&1 &
CLIENT=$!
max=0; total=0; n=0
while kill -0 $CLIENT 2>/dev/null; do
  rss=$(ps -o rss= -p $CLIENT 2>/dev/null | tr -d ' ')
  if [[ -n "$rss" ]]; then
    (( rss > max )) && max=$rss
    (( total += rss, n += 1 ))
  fi
  sleep 0.5
done
wait $SERVE 2>/dev/null
grep -hE "BOBTRACE_|TRACE_ERROR|panicked" $OUT.log $OUT.serve.log
(( n > 0 )) && echo "BOBTRACE_RSS max_mb=$(( max / 1024 )) mean_mb=$(( total / n / 1024 ))"
