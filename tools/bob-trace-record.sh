#!/bin/zsh
# Record the benchmark session for tools/bob-trace-bench.sh.
#
#   BOB_TRACE_SERVER=<offline 1.8.9 server folder> tools/bob-trace-record.sh
#
# The server folder needs server.jar, an accepted eula.txt, online-mode=false,
# server-port=25566 and view-distance=16. The session: stand at spawn for
# about 14 s (the lobby), rise 30 blocks, then fly east 0.8 blocks a tick
# (16 blocks/s) for 25 s with console teleports. Writes
# $BOB_TRACE_DIR/session.bin. Traces hold the test world: keep them private.
set -u
SERVER=${BOB_TRACE_SERVER:?set BOB_TRACE_SERVER to the test server folder}
DIR=${BOB_TRACE_DIR:-target/bob-trace}
TARGET=${CARGO_TARGET_DIR:-target}/aarch64-apple-darwin/release
BIN=${BIN:-$TARGET/horizon-189-client}
TRACE=${TRACE:-$TARGET/horizon-189-trace}
JAVA=${JAVA:-java}
mkdir -p $DIR
DIR=${DIR:A}; BIN=${BIN:A}; TRACE=${TRACE:A}
cd $SERVER || exit 1
rm -f trace-server.log trace-in
mkfifo trace-in
( tail -f trace-in | "$JAVA" -Xms512M -Xmx2G -jar server.jar nogui > trace-server.log 2>&1 ) &
for i in {1..400}; do grep -q "Done (" trace-server.log 2>/dev/null && break; sleep 0.5; done
"$TRACE" record --listen 127.0.0.1:25570 --server 127.0.0.1:25566 --out $DIR/session.bin > $DIR/record.log 2>&1 &
PROXY=$!
sleep 1
"$BIN" --offline --username NativeBench --server 127.0.0.1 --port 25570 --render-distance 16 \
  --smoke-frames 30000 > $DIR/record-client.log 2>&1 &
CLIENT=$!
for i in {1..120}; do grep -q "NativeBench joined" trace-server.log && break; sleep 0.5; done
echo "gamemode 1 NativeBench" > trace-in
echo "time set 6000" > trace-in
echo "gamerule doDaylightCycle false" > trace-in
sleep 14
echo "tp NativeBench ~ ~30 ~" > trace-in
sleep 1
for step in {1..500}; do
  echo "tp NativeBench ~0.8 ~ ~" > trace-in
  sleep 0.05
done
sleep 2
echo "kick NativeBench Recording done" > trace-in
sleep 2
wait $PROXY 2>/dev/null
kill $CLIENT 2>/dev/null
echo stop > trace-in
sleep 4
pkill -f "server.jar nogui"; pkill -f "tail -f trace-in"
rm -f trace-in
tail -1 $DIR/record.log
