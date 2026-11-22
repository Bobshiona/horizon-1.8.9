#!/bin/zsh
# The Watchdog packet-parity suite (docs/sprint-queue.md, standing rules).
#
#   tools/bob-watchdog.sh
#
# Replays the lobby part of $BOB_TRACE_DIR/session.bin (before its first
# teleport) with 24 fighters, runs the release client with --watchdog (every
# module on, a scripted session through the real key and mouse handlers) and
# checks everything the client sent with `horizon-189-trace check`: vanilla
# packets only (with at least one real attack), one movement packet a tick, a position at least every 20
# ticks, each attack after its swing, sneak/sprint alternation, no sneak
# packets with a screen open, settings and brand once. Prints WATCHDOG_PASS or
# WATCHDOG_FAIL lines; exits non-zero on failure.
set -u
DIR=${BOB_TRACE_DIR:-target/bob-trace}
TARGET=${CARGO_TARGET_DIR:-target}/aarch64-apple-darwin/release
BIN=${BIN:-$TARGET/horizon-189-client}
TRACE=${TRACE:-$TARGET/horizon-189-trace}
PORT=${PORT:-25572}
OUT=$DIR/watchdog
"$TRACE" serve --listen 127.0.0.1:$PORT --trace $DIR/session.bin --until 14000 \
  --crowd 24 --crowd-at 1000 --seed 2 --sparring --log $OUT.client.bin > $OUT.serve.log 2>&1 &
SERVE=$!
sleep 0.5
"$BIN" --offline --username NativeBench --server 127.0.0.1 --port $PORT --watchdog > $OUT.log 2>&1
CLIENT=$?
sleep 1
kill $SERVE 2>/dev/null
wait $SERVE 2>/dev/null
windows=$(grep -oE "WATCHDOG_WINDOWS [0-9,-]+" $OUT.log | awk '{print $2}')
done_line=$(grep -oE "WATCHDOG_SCRIPT_DONE ticks=[0-9]+" $OUT.log)
if [[ $CLIENT != 0 || -z "$done_line" || -z "$windows" ]]; then
  echo "WATCHDOG_FAIL the scripted session did not finish (client exit $CLIENT)"
  grep -E "panicked|Error" $OUT.log | head -3
  exit 1
fi
echo "$done_line"
"$TRACE" check --log $OUT.client.bin --closed $windows || exit 1
# The swing-before-attack rule needs real attacks: the script swings at a
# sparring partner standing in reach.
"$TRACE" check --log $OUT.client.bin | grep -E " 0x02x[1-9]" > /dev/null || {
  echo "WATCHDOG_FAIL no attack reached the server; the attack rule went unchecked"
  exit 1
}
