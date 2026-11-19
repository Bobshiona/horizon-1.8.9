# Horizon 1.8.9 native client handoff

## 1. Sprint Status

Updated November 18–19, 2026 (America/Los_Angeles). Release target December 2.

Native Rust + Apple Metal client for Minecraft 1.8.9 (Protocol 47), built inside the Horizon workspace and
isolated from its modern 26.3 code. Public at `github.com/Bobshiona/horizon-1.8.9` (`main`), a squashed launch
commit plus verified development checkpoints on top of upstream `4e60c05893`, authored with the account's noreply address. Local
`feature/minecraft-1.8.9` tracks it; `backup/pre-launch-squash` keeps the unsquashed history and is never
pushed (it holds local paths). The squashed commit covers:

| Scope | Contents |
| --- | --- |
| registry + physics | Full 1.8.9 block registry, `EntityPlayerSP` movement, collision shapes |
| live-play audit | Keyboard/mouse fixes, mesher thread, per-column GPU buffers, reversed-Z, mipmaps, fog |
| entities | Players and mobs, block entities, flying, Depth Strider, client skylight, respawn |
| launch | README, `docs/media/189` screenshots, upstream README in `docs/`, stricter `.gitignore` |

Before any push: `python3 tools/check_public_tree.py`, and grep the new commits for home-directory paths,
account names and tokens. Never commit the jar, extracted assets, the reference server or captures.

Checkpoint added fixed-width entity status decoding, ten-tick hurt feedback with gamma-space red overlay,
and textured standard zombie/skeleton geometry. Scoped tests (including owned-jar/Metal), Clippy, formatting,
layer/public-tree checks and the local entity/flight gate pass. The entity capture was inspected.

Equipment checkpoint adds owned armor textures, sword/bow sprites, depth-tested profile names, signed
texture-property hooks, and adult creeper/pig/cow/villager geometry. The expanded local entity gate supplied
a diamond armor set and sword, captured 700 Metal frames, and finished flight with zero movement flags.
All 69 scoped tests, strict Clippy and formatting pass. Profile README/bio updates and three private PNG
avatars have been completed outside this repository.

Interaction checkpoint adds bounded legacy slot/NBT serialization, ray selection, ordered user-driven
attack/interact/use/digging, a nine-slot hotbar, inventory/container synchronization with rejected
transaction recovery, and an item-icon grid. Mining strength, harvest permissions and stack limits were
queried from all 326 registered item entries against 198 blocks in the official reference server. Empty
typed NBT lists and modified UTF-8 survive round trips.

The managed local reference gate confirms zombie hurt status after an attack, sword use/release, hotbar
selection, cobblestone placement and survival mining, then chest open/pickup/close. Both resulting block
changes were independently confirmed by the server. Native equipment/flight smoke renders 700 frames
per viewport with zero movement flags. An occluded smoke window uses a reusable offscreen Metal target,
with offscreen counts reported explicitly. Entity/profile and inventory snapshots are shared by Arc; ray
selection retains scratch storage.

The personal repository is now `origin`; the original project is `upstream`. The feature branch tracks
`origin/main`. All new commits retain the configured GitHub noreply author. The backup branch stays local.

The next checkpoint broadens common-mob geometry and interpolates entity pitch between client ticks.
Selection shares entity dimensions, including child/slime sizes. Spectator mode now forces flight,
bypasses collision/push-out and preserves flight on ground contact. The managed server test confirms
travel through a solid wall after switching to spectator mode. The expanded native mob/flight gate passes.

Skin/cape bindings now resolve server profile properties asynchronously from the Mojang texture CDN.
Classic 64x32 skins are normalized to 64x64; slim metadata selects arm geometry. Dynamic Metal layers
are reused across despawns and restored after surface recovery. Cape geometry obeys the player model
parts bit. Downloads and decode stay off the simulation/window threads, with bounded payloads and retry
backoff. The local proxy gate exercised a public profile skin and a synthetic cape fixture.

An 80-player synthetic Protocol 47 lobby plus reference entities rendered 700 frames. Debug frame-submit
wall times over 699 samples measured P50 7,539 us, P95 7,695 us and P99 7,787 us. The viewport tracked
about 90 entities. One frame used the smoke-only offscreen target. These figures cover frame submission
and drawable acquisition; GPU duration and live multiplayer latency need their own measurement.

**Live Hypixel lobby login passed** with Microsoft device-code sign-in: 120 Metal frames, 86 chunk columns
and about 40 tracked entities. It needed an auth fix: an empty 204 body is `Null`, and a non-JSON HTTP error
keeps its status (`<service> returned invalid JSON (HTTP n)`) without echoing the body.

Vitals checkpoint: hearts (absorption rows from entity metadata index 17, `generic.maxHealth` with its
modifiers), armour points from the four armour slots (vanilla per-material table), and food, all cut from
the owned `gui/icons` atlas with `GuiIngame`'s full/half thresholds and row stacking. At zero health the
client stops ticking physics, drops attack/use/actions, releases the cursor and shows a death panel using
the jar's `en_US.lang` (`deathScreen.title`, `deathScreen.respawn`); Enter or the button sends C16
`ClientStatus` action 0. The managed reference gate kills the test player and confirms the respawn. A
local capture checked a 14-point armour row, a partial absorption heart and an empty food bar.

Interface and chat checkpoint. The 2D HUD moved off world-space geometry onto its own screen-space pass
(`ui.rs`, `ui.wgsl`): vanilla `ScaledResolution` on the logical size (`--gui-scale`, 0 automatic), integer
device pixels per GUI pixel, a 256-pixel atlas (`ascii`, `gui/icons`, `gui/widgets`, lazily uploaded
`unicode_page_XX`), and a non-sRGB view of the drawable so translucency blends in gamma space as 1.8.9
does. `FontRenderer` is ported (colour/shadow tables, bold, italic, underline, strikethrough, obfuscated,
unicode glyph sizes, `getStringWidth` quirks, `wrapFormattedStringToWidth`). `hud.rs` draws `GuiIngame`
from the real sheets: inverted crosshair, widgets hotbar and selection, stack counts, armour, hearts
(absorption rows, hurt blink, low-health shake, regeneration wave, poison/wither/hardcore rows), food
(hunger, saturation shake), the experience bar and level, the action bar, and `GuiGameOver` with its
gradient, score and widgets buttons. Chat (`chat.rs`, protocol `text.rs`/`chat.rs`): JSON components with
inheritance, translations (`en_US.lang`), click/hover events per character, 320-pixel wrapping, 100 lines,
the 10-second fade, focused 20-line view with scroll bar and wheel/PageUp scrolling; `GuiChat` input on T
or `/` with history, macOS editing (Option word, Command line), clipboard via `arboard`, server tab
completion (C14/S3A), `run_command`/`suggest_command` clicks and `show_text` tooltips. `open_url` copies the
link instead of opening it. The reference gate sends chat through the chat screen (`--smoke-chat`) and
requires the server to log it; a capture checked formatting, unicode, wrapping and the input line.

Scoreboard checkpoint. Protocol `scoreboard.rs` decodes S3B/S3C/S3D/S3E/S45/S47 and `player_list` now
keeps game mode, ping and display names. The client `scoreboard.rs` applies vanilla rules (unknown
objectives ignored, empty-objective removal clears a holder everywhere, joining a team leaves the old one,
removal clears display slots) with size bounds, and publishes to the window once per tick. `hud.rs` draws
`renderScoreboard` (own team colour's sidebar slot first, 15 lines, `#` holders hidden, vanilla sort),
titles and subtitles with fade times, and `GuiPlayerTabOverlay` (header/footer, up to 80 entries in
columns of 20, team sort with spectators last, list-slot scores, ping bars, faces on encrypted logins from
loaded skins or the default Steve/Alex). Nametags use team prefix/suffix colours, team visibility rules
and the below-name objective; armour-stand and mob custom names (`CustomNameVisible`) render as labels.
A capture against the reference server checked the sidebar, title, Tab overlay, team nametag and an
invisible armour-stand hologram. `--smoke-tab` holds the list key for validation captures. Known gaps:
world labels skip unicode glyphs and bold, lack vanilla's see-through background, and `hearts` list
objectives show numbers.

Inventory checkpoint. Windows know their `GuiContainer` (player, chest of any rows, hopper,
dispenser/dropper, crafting table, furnace, and a chest-style fallback) with vanilla sheets, slot
coordinates, `getSlotAtPosition` hit areas, titles (`getUnformattedText`) and labels. Clicks follow
`slotClick`: pick up, shift-click (`ContainerPlayer`/`ContainerChest.transferStackInSlot` with
`mergeItemStack` and the retry loop), number keys over a slot, Q/Control+Q drop over a slot, and -999
outside the window; each is predicted locally because accepted clicks are not resent. Windows without a
predictor send no clicked item so the server resends them. Q in the world sends C07 drop actions. Item
data comes from `item_data.rs` (translation keys, durability, stack sizes queried from the official
registry; names translate from the owned jar); `stack_limit` now reads it, which fixed records stacking
to 64. `tooltip.rs` ports `getDisplayName`, rarity colours and `getTooltip` (enchantments, stored
enchantments, custom potion effects, dyed, lore, attack damage with Sharpness, attribute modifiers,
unbreakable, `HideFlags`). Slots draw durability bars and two-layer potion and leather icons; the hotbar
shows the 40-tick item name. `--smoke-inventory` opens the inventory for a capture. Known gaps: block
items draw their first face flat, no player model in the inventory, no drag-splitting or double-click
collect, furnace/brewing progress not drawn, and other window types have no shift-click prediction.

Combat entities and first-person checkpoint. Spawn Object velocity and S12 velocity now reach non-living
entities, which simulate locally between server updates: arrows and throwables (0.99 drag, per-kind
gravity, stopping at solid blocks, yaw/pitch from motion), items, primed TNT and falling blocks (0.04
gravity, ground friction), constant-motion fireballs. `RenderEntityItem` is ported from the jar's
bytecode: bob and spin, 2-5 copies at counts above 1/16/32/48, 3D models scattered by a `Random` seeded
187 and flat items stacked 0.046875 deep, both through `RenderItem.renderItem`'s half scale. Arrows use
`arrow.png`; snowballs, eggs, pearls, eyes, bottles, splash potions (with overlay tint), fire charges
(small and large fireballs), fishing bobbers and pulsing XP orbs are billboards; primed TNT swells and
flashes; falling blocks draw their block. `hand.rs` ports `ItemRenderer`: equip progress (0.4 per tick,
swap below 0.1, re-equip on use), arm lag (`rotateWithPlayerRotations`), `transformFirstPersonItem`,
`doItemUsedTransformations`, block-hit (`doBlockTransformations`), bow draw and shake, eating/drinking,
and `renderPlayerArm` with the player's own skin. Held items use the jar's display transforms: generated
items [0,-135,25]/[0,4,2]/1.7 ([0,45,25] for the fishing rod and carrot on a stick), block items doubled
by `ItemRenderer.renderItem` with the carpet, trapdoor and fence-gate offsets; bow pulling, potion and
leather layers draw. The hand renders in its own pass with cleared depth and a fixed 70-degree
projection. Camera effects follow `EntityRenderer`: hurt tilt, death roll and view bobbing
(`--no-view-bobbing`). `particles.rs` emits `EntityCrit2FX` bursts on predicted crits (falling, not on
ground, not in water or blind) and enchanted hits, S0B animations 4/5 from the server, and arrow crit
trails. Physics now tracks fall distance, distance walked and camera yaw/pitch for these effects. A
capture checked dropped items, a stuck arrow, swelling TNT, a landed falling block, collected XP orbs and
the held sword, bow, fishing rod and wool block. Known gaps: other players' held items still use an
approximate third-person transform (no `LayerHeldItem` port, so their block-hit pose is not shown), no
fishing line, no item pickup animation, no explosion/smoke/block-break particles, and the cast fishing
rod model is not swapped in.

Next (in order): Keychain-backed refresh-token caching; a release build measured under the 80-player lobby.

## 2. Verified State

- 107 scoped tests pass with `--include-ignored` (client 34, login CLI 2, assets 1, auth 3, physics 16,
  protocol 35, world 16 — counts include the Metal readback and the owned-jar registry test).
  `horizon-client-input` 209/209. Scoped Clippy `-D warnings`, `cargo fmt --check`, `check_layers.py`,
  `check_public_tree.py` pass.
- Registry: `variants=2557 texture_layers=361 model_failures=0 inexact_states=12` (the 12 are rail metadata
  vanilla never stores).
- Stair-corner collision equals the official 1.8.9 server jar on all 52,488 neighbour configurations
  (`cargo run -p horizon-physics-189 --example stairs_probe` against a private reflection harness).
- Movement gate (official 1.8.9 server, view distance 8): walk/sprint/sprint-jump/sneak/strafe/back-jump
  autopilot for 300 ticks, 357 columns meshed, `SERVER_MOVEMENT_FLAGS 0`.
- Entity gate: a second native client renders as a skinned player; textured zombie and villager models; an
  invisible armour stand stays hidden; single, double, trapped and ender chests; a standing sign with its
  text in the vanilla font; creeper and skeleton skulls; banners in their base dye. A creative client then
  double-taps into flight, climbs, cruises, sprint-flies, descends and lands with `SERVER_MOVEMENT_FLAGS 0`.
- Captures (ignored by git): `target/horizon-189-verification/{registry-showcase,entities-showcase,sign-closeup}.png`.

## 3. Crate Architecture Map

| Package | Role |
| --- | --- |
| `horizon-protocol-47` | Framing, compression, AES/CFB8, login, play primitives; `player.rs` (entity actions, health, velocity, explosions, attributes, effects); `entities.rs` (spawns, movement, metadata, equipment, status, abilities, signs and block entities); `interaction.rs` (user action packets); `player_list.rs` (profile/texture properties); `nbt.rs` (bounded read/write NBT and slots). |
| `horizon-world-189` | Columns, sections, block changes, heightmap skylight; `blocks.rs` IDs 0–197; `variants.rs` blockstate selection; `entities.rs` tracked entities (3-step interpolation, body yaw, limb swing, swings); `block_entities.rs` (signs, skulls, banners). |
| `horizon-physics-189` | Exact AABB clipping; legacy sine table; per-block collision shapes; the player tick with flight and Depth Strider. |
| `horizon-assets-189` | Owned-jar blockstates, rotated model baking, textures. |
| `horizon-189-client` | `skins.rs` bounded CDN worker/cache and bindings; `frame_stats.rs` allocation-free smoke timing; `keyboard.rs` macOS key state; `network.rs` Protocol 47 actor + mesher thread; `terrain.rs` column meshing; `models.rs` entity/block-entity geometry; `scene.rs` Metal resources; `renderer.rs` surface, camera, capture; `gameplay.rs` ordered interaction controller; `inventory.rs` authoritative snapshots/transactions; `hardness.rs` and `items.rs` queried legacy registry facts. |

## 4. Live-play audit (after the first keyboard/mouse session)

- **Mouse:** vertical look was inverted (AppKit deltas are down-positive, LWJGL's up-positive). Look is
  applied once per frame with vanilla's sensitivity curve and `setAngles`; the warp delta after capture is
  dropped; `--sensitivity` sets vanilla's 0–1 value; the cursor captures on join.
- **Keyboard:** auto-repeat ignored; modifier releases from `flagsChanged`; Command chords drop other keys
  (AppKit withholds their key-ups); Escape, focus loss and occlusion unpress everything; held keys are
  reconciled each frame against `CGEventSourceKeyState` once it proves reliable. Sprint is Control or Left
  Option, since Control-Space is the macOS input-source shortcut.
- **Movement loop:** meshing runs on its own thread (≈1.2 ms per column in debug), so 50 ms ticks never wait.
- **Renderer:** reversed-Z infinite projection, back-face culling, per-column buffers with frustum culling,
  sorted translucency, sRGB mipmaps, fog, vanilla-like sky.

## 5. Known Gaps

1. Combat and interaction now have user-driven swing/attack/interact, hotbar selection, sword blocking,
   block placement, survival/creative digging, and a basic inventory/container grid. Remaining parity work
   includes exact selection boxes for non-colliding decorations, all entity collision sizes, client break
   prediction/crack effects, third-person held-item transforms, crafting/shift-click
   and special-container rules. Adventure restrictions are enforced conservatively. Live Hypixel remains
   unverified.
2. Player armor, generated item sprites, held block geometry, depth-tested names and asynchronous
   skin/cape bindings are present, as are first-person hands and first-person item display transforms.
   Enchantment glint and third-person item display transforms remain pending. Cape motion currently uses limb/sneak pose; vanilla chasing-
   position cape dynamics and additional HD texture layouts require further parity.
3. Standard zombie, skeleton, wither skeleton, creeper, pig, cow, villager, sheep, chicken, wolf, spider,
   cave spider, slime and enderman meshes are present. Age, sheep shearing/color, slime size, wolf texture
   flags and server ground state are retained. Child transforms cover zombies and several quadrupeds.
   Remaining mobs, villager professions, collars/sitting and special animation states need further parity.
4. Full client light engine (heightmap skylight covers open sky; covered cells use server light).
5. Fluid flow textures and smooth lighting/AO.

## 6. Hard Invariants (Do Not Break)

- Native Rust and Apple Metal on `aarch64-apple-darwin`; no JVM/JNI/LWJGL/Forge/OpenGL translation.
- Preserve f64 positions/motion and 1.8.9's exact f32 operations, promotions and update order. No fast-math.
- Fixed 50 ms ticks; vanilla movement packet selection, ground flags, abilities → actions → movement order.
- Settings precede `MC|Brand` after Join Game. Vanilla swing-before-attack when combat lands.
- Combat stays user-driven with vanilla reach, velocity and inventory semantics.
- `--autopilot` and `--autopilot-fly` are validation-only scripted input for a local reference server.
- `HORIZON_MSA_CLIENT_ID` overrides the default Microsoft client ID; tokens never appear in diagnostics.
- Never commit game jars, extracted assets, captures, reference worlds or credentials.
- Validate against a local vanilla 1.8.9 server before Hypixel. Watchdog outcomes need live evidence.

## 7. Verification Gate

```sh
export PATH="$HOME/.cargo/bin:$PATH" CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4
export HORIZON_189_TEST_JAR="$HOME/Library/Application Support/minecraft/versions/1.8.9/1.8.9.jar"
P=(-p horizon-189-client -p horizon-protocol-47 -p horizon-auth -p horizon-world-189 -p horizon-assets-189 -p horizon-physics-189 -p horizon-client-platform)
cargo build --offline -p horizon-189-client --target aarch64-apple-darwin
cargo test --offline $P --target aarch64-apple-darwin
cargo clippy --offline $P --all-targets --target aarch64-apple-darwin -- -D warnings
cargo fmt --check $P -p horizon-client-input
python3 tools/check_layers.py --base 4e60c05893 && python3 tools/check_public_tree.py
python3 tools/test_189_reference.py
```

The private `horizon-189-skins-gate.py` runner injects a public skin and synthetic cape into the native
viewport through a loopback framing/compression proxy. `horizon-189-lobby-gate.py` adds 80 synthetic
players and movement packets for the frame-submission measurement. These fixtures remain outside git.

The committed `tools/test_189_reference.py` runner locates Java 8, verifies/downloads the official server
into the system temporary directory, configures an offline loopback fixture, runs all scoped tests with
`--include-ignored`, independently confirms placement/mining, and runs a native Metal smoke. It stops the
server in `finally` and retains diagnostic logs outside git. The native client gets isolated runtime homes.
The earlier private `horizon-189-interaction-gate.py` runner provides the same interaction fixture. It sets `HORIZON_189_REFERENCE_GATE`, `HORIZON_189_FIXTURE_READY` and the owned-jar
path for the ignored interaction test. Run that test only through the managed fixture. The private
`horizon-189-mobs-gate.py` runner covers the second player, armor, held items, names, common mobs,
block entities and flight. Both scripts and all server files live in the system temporary directory.
The official server jar SHA-1 is `b58b2ceb36e01bcd8dbf49c8fb66c55a9f0676cd`, using installed Zulu Java 8.

Latest gate outcomes are 83 scoped tests passing, strict scoped Clippy and formatting passing, layer and
public-tree checks passing, interaction gate passing, and native equipment/flight smoke passing with zero
server movement flags. Earlier broad-workspace validation recorded macOS path/tool-assumption failures and disk exhaustion.
This checkpoint revalidates the scoped client packages; the whole upstream workspace remains unverified.

Manual play: run the canonical binary with `--offline --username <name> --server 127.0.0.1` against the
reference server. The cursor captures on join; Escape releases it; click recaptures. Keys are WASD, Space,
Shift for sneak, Control or Left Option for sprint, and double-tap Space to fly where allowed. Left/right
mouse drive attack/dig and interact/use. Number keys or the wheel select hotbar slots. E opens the player
inventory; E or Escape closes it. Container slots accept normal left/right pickup clicks. Tokens remain
memory-only and a later authenticated session requires another device-code login or direct token.
