# Release live-test plan and December 2 cut line

Prepared November 19, 2026. Baseline: `133e2a9491`. Client runtime remains native Rust + Apple Metal.

## Scope and entry gate

This is a conservative manual compatibility test. It cannot establish Watchdog safety: Hypixel says
custom modifications can send irregular data, punishments can be delayed, and no modification is guaranteed
safe. Automation and changes to vanilla server communication are prohibited. Use physical mouse/keyboard
input and vanilla gameplay semantics throughout. Sources:
[Watchdog](https://support.hypixel.net/hc/en-us/articles/360019613300-About-the-Hypixel-Watchdog-System),
[allowed modifications](https://support.hypixel.net/hc/en-us/articles/6472550754962-Hypixel-Allowed-Modifications).

Before public play, the implementation owner must have a passing local reference gate for this source
revision and inspect movement/action ordering. Keep autopilot, synthetic players, packet injection and
loopback proxies confined to localhost. Public commands below omit all scripted input flags. A lobby
login has passed previously; full PvP, real refresh-token reuse and current-release performance remain
separate gates. The sprint sync reports 115 scoped tests passing; this planning pass rebuilt release
successfully but did not rerun the suite.

## Exact commands: prepare once

Run from this checkout's root in zsh. The owned jar must already be installed. Do not rebuild between the
two login launches: a new ad-hoc binary signature can trigger another macOS Keychain prompt.

```sh
export PATH="$HOME/.cargo/bin:$PATH" CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4
cargo build --release --offline --locked -p horizon-189-client --target aarch64-apple-darwin
export H189_BIN="$PWD/target/aarch64-apple-darwin/release/horizon-189-client"
export H189_ASSETS="$HOME/Library/Application Support/minecraft/versions/1.8.9/1.8.9.jar"
test -x "$H189_BIN" && test -f "$H189_ASSETS"
umask 077
export H189_RUN_DIR="$(mktemp -d -t h189-hypixel)"
export HORIZON_HOME="$H189_RUN_DIR/runtime" HORIZON_LAUNCHER_HOME="$H189_RUN_DIR/launcher"
export XDG_DATA_HOME="$H189_RUN_DIR/data" XDG_CONFIG_HOME="$H189_RUN_DIR/config"
export HORIZON_NO_AUDIO=1 HORIZON_LAUNCHER_NO_SELF_UPDATE=1
mkdir -p "$HORIZON_HOME" "$HORIZON_LAUNCHER_HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME"
git rev-parse HEAD > "$H189_RUN_DIR/revision.txt"
git status --short > "$H189_RUN_DIR/worktree.txt"
shasum -a 256 "$H189_BIN" > "$H189_RUN_DIR/binary.sha256"
printf '%s\n' "$H189_RUN_DIR"
```

The runtime folders are private and outside git. Keychain persistence is independent of these folders.
Do not export tokens, read them with `security ... -w`, or enable verbose HTTP-body logging. Keep the
current client-ID configuration unchanged between launches.

## Launch 1: sign-in and lobby, 5–10 minutes

```sh
"$H189_BIN" --assets "$H189_ASSETS" --server mc.hypixel.net --render-distance 8 --gui-scale 0 > "$H189_RUN_DIR/01-lobby.log" 2>&1 &
H189_PID=$!
tail -n 20 "$H189_RUN_DIR/01-lobby.log"
```

If the device code has not appeared, repeat `tail`. Complete Microsoft approval yourself at the logged
verification URL. Approve the macOS Keychain prompt for this known build if it appears. Do not use
`--no-saved-login`: it skips both loading and saving. If the cache is already valid, saved-login success
on this first launch is useful evidence; do not sign out merely to force a code.

- Expect `MICROSOFT_AUTH_PASS` then `MICROSOFT_LOGIN_SAVED`, or `MICROSOFT_SAVED_LOGIN_PASS`, followed
  by `NETWORK_LOGIN_PASS` and visible terrain/entities. Save any `_NOT_SAVED` or `_UNAVAILABLE` marker.
- Walk, sprint while holding the key, jump, sneak at an edge, traverse stairs/slabs, move the mouse, switch
  focus and resume. Look for stuck input, repeated position corrections, invisible collision or drift.
- Read chat, sidebar, titles and Tab. Open a server menu, use normal clicks, close/reopen it, and compare
  contents. Check NPC labels and symbols; record unreadable navigation text.
- Capture a lobby view, Tab and an open menu. Check visible chests, signs, skulls and banners when present.
  Existing local captures already prove these render in fixtures; report a live regression with location.

After exiting through the window close control, collect its exit status:

```sh
wait "$H189_PID"
H189_EXIT=$?
printf 'exit=%s\n' "$H189_EXIT" > "$H189_RUN_DIR/01-exit.txt"
```

## Launch 2: saved login, then manual PvP

```sh
"$H189_BIN" --assets "$H189_ASSETS" --server mc.hypixel.net --render-distance 8 --gui-scale 0 > "$H189_RUN_DIR/02-pvp.log" 2>&1 &
H189_PID=$!
tail -n 20 "$H189_RUN_DIR/02-pvp.log"
```

Expect `MICROSOFT_SAVED_LOGIN_PASS` without another device approval, then network login. A Keychain prompt
is a separate OS access decision; record it separately from a new Microsoft code. Refresh failure is a
login gate failure, even if fallback sign-in subsequently works.

1. **Duels, one complete match**, preferably with a consenting partner: hotbar swaps, ordinary attacks,
   swing/block-hit pose, bow draw/release, rod cast/retract where available, taking hits and knockback,
   hurt feedback, health/absorption, death and respawn. Record visible action-to-response delays. Do not
   deliberately probe anticheat thresholds or use click automation.
2. **BedWars, one complete game**, after Duels works: shop purchases, chest pickup/shift-click/number-key
   swaps, close/reopen authoritative contents, world drops, bridging with held sneak, block placement,
   mining/cracks/break fragments, combat and respawn. Observe naturally used TNT/fireballs, fuse,
   explosion and knockback. Explosion/smoke visuals now have local reference coverage; inspect them live. SkyWars can supplement
   coverage but does not replace BedWars shop/placement checks.
3. Mark unavailable cases as untested. Check Depth Strider or permitted flight only where the server
   legitimately provides them. Do not enable abilities or synthesize actions for coverage.

Stop advancing to matches on a kick/warning, repeated rubberbanding, missing terrain/collision,
incorrect knockback or ghost inventory. Save exact disconnect text and the preceding physical actions;
reproduce on the local server before another public attempt. The client now retains disconnect text on
a connection-lost screen. Capture it before using Reconnect (or Enter); retry reuses the current session,
so an expired session requires a normal relaunch/sign-in. Check that old HUD/world/input state does not
survive a local retry. Native button interaction remains a manual gate. A clean short session is compatibility
evidence, not an anticheat certification.

For each screenshot, run in the same terminal and select the client window:

```sh
/usr/sbin/screencapture -i -w -x "$H189_RUN_DIR/lobby.png"
/usr/sbin/screencapture -i -w -x "$H189_RUN_DIR/tab.png"
/usr/sbin/screencapture -i -w -x "$H189_RUN_DIR/shop.png"
/usr/sbin/screencapture -i -w -x "$H189_RUN_DIR/duels.png"
```

Take these at their corresponding stages, not consecutively on one screen. Use macOS screen recording
for a short knockback/block-hit/TNT or inventory fault clip. After closing the second client:

```sh
wait "$H189_PID"
H189_EXIT=$?
printf 'exit=%s\n' "$H189_EXIT" > "$H189_RUN_DIR/02-exit.txt"
```

## Separate stationary lobby timing run

Only after leaving a match, launch this bounded session and keep the window visible. It exits after
1,200 rendered world frames; never use it for a match that could be interrupted.

```sh
"$H189_BIN" --assets "$H189_ASSETS" --server mc.hypixel.net --render-distance 8 --gui-scale 0 --smoke-frames 1200 --capture "$H189_RUN_DIR/lobby-timing.png" > "$H189_RUN_DIR/03-timing.log" 2>&1
H189_EXIT=$?
printf 'exit=%s\n' "$H189_EXIT" > "$H189_RUN_DIR/03-exit.txt"
rg 'MICROSOFT_|NETWORK_|PLAYER_TICKS|FRAME_SUBMIT_US|FRAME_INTERVAL_US|SMOKE_PASS|offscreen_frames' "$H189_RUN_DIR"/*.log > "$H189_RUN_DIR/markers-private.txt"
```

Current normal sessions do not collect/report frame summaries. Bounded runs print submit wall time and
intervals between visible render completions; neither is a GPU timestamp nor measured display scanout
latency. The ring retains the last 1,024 samples, not the whole session. Report P50/P95/P99/max,
`over_2x_median`, sample count, frames, columns and offscreen count. Offscreen rendering does not prove
presented cadence. Record resolution, display refresh rate, GUI scale, render distance, approximate
visible-player count and thermal/power conditions. Repeat on external high-refresh display if available;
do not extrapolate the existing 60 Hz synthetic benchmark to live 240 Hz play.

## Evidence to return

Return the private run-directory path, revision/hash/worktree files, three full logs and exit files,
lobby/Tab/shop/Duels screenshots, and short fault clips. Review before sharing: logs contain account
names and may contain device codes; screenshots/chat/video can identify players. Keep raw evidence out
of public git. A marker extract supplements full logs; it cannot establish packet sequencing.

For each issue provide timestamp with timezone, stage/map, physical action, expected/observed result,
repeatability and evidence filename. Explicitly list Keychain reuse, Duels completion, BedWars completion,
block entities and timing as pass/fail/untested. There is no current packet-trace flag; obtain sanitized
ordering evidence with a local fixture, not an invented public-server tracing command.

## November 19–December 1 priorities

| Dates | Required outcome | Cut line |
| --- | --- | --- |
| Nov 19–20 | Release login/relaunch and Lobby → Duels evidence; fix packet, movement, knockback, inventory or block-entity regressions first. Audit validation flags as localhost-only. | Any reproducible gameplay/desync failure blocks widening live play. |
| Nov 21–23 | TNT explosion/smoke/fuse readability; particle material/collision fixes; drag-split and double-click collect with transaction recovery. | Correct physics and authoritative inventory outrank visual polish. |
| Nov 24–26 | Re-test full Duels/BedWars; minimal own FPS/CPS HUD and adjustable FOV. | Counters observe physical events; never generate attacks or expose hidden opponent data. |
| Nov 27–29 | Repeated complete games, Retina and available 60/120/240 Hz checks; fix tail hitches and focus/resize failures. | Record actual coverage; one benchmark does not certify every display. |
| Nov 30–Dec 1 | Feature freeze, release blockers only; repeat sign-in, reference gate, full-game checks and document limitations. | Missing P0 evidence means hold release readiness. Dec 2 is the target, not proof of readiness. |

P0: vanilla packet/action/movement behavior, correct hits/velocity/respawn, authoritative shops/inventory,
usable block entities and sustained rendering. Locally audit C0A/C02 swing/attack, C03–06 movement and
C0B actions, C07/C08 dig/use, C0E/C0F clicks/transactions, keepalives and S08 correction handling.

P1: explosion/smoke and fuse readability, drag-split/collect, basic FPS/CPS/FOV and readable server text.
Raise missing label glyphs/bold if they hide NPC navigation or server-supplied PvP information. The current
CLI now supports `--pvp-hud` (off by default) and `--fov F` (30–110°, default 70°). CPS is left/right
button-down transitions over the last second, excluding GUI/dead/unfocused/recapture clicks; held actions
are not extra clicks. FPS uses visible render completions, not GPU timestamps or display scanout. Use
`--pvp-hud --fov 90` for an optional live readability check, keeping the same settings across comparisons.
Zoom and keystrokes remain deferred.

Defer keystroke visuals and zoom first if gates slip; also brewing, inventory player model, URL confirmation,
Tab heart artwork, exact glint stripe spacing, rare mob variants/cape dynamics, full AO/light/fluid polish,
launcher cosmetics and extra menus. Maintain working safety/correctness fallbacks for deferred containers.

Do not restart stair parity: 52,488 official-server configurations already match. Depth Strider and
spectator/flight have tests. Reopen these when touched or when live evidence identifies a regression.
Action bar/Tab header/footer can use local Protocol 47 fixtures; furnace progress, crits, block poses and
hurt feedback can be exercised with controlled local gameplay. They do not inherently require Hypixel.
