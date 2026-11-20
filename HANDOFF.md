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
world labels skip unicode glyphs and bold, and `hearts` list objectives show numbers.

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
shows the 40-tick item name. `--smoke-inventory` opens the inventory for a capture. Block items are
drawn as `renderItemIntoGUI` does for 3D models (10 GUI pixels a block, 210 degrees about X, -135 about
Y, then the item model's own `gui` turn: 90 for fences, 180 for stairs), projected on the CPU and
drawn back to front with back faces culled. Fences, walls, buttons, pressure plates and pistons use the
`*_inventory` block models their item models name, in the hand, on the ground and in slots. Known gaps:
no player model in the inventory, no brewing stand screen, and other window types have no shift-click prediction. Furnaces draw
`GuiFurnace`'s flame and arrow from S31 window properties.

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
the held sword, bow, fishing rod and wool block. S0D `Collect Item` marks an item, arrow or orb with its
collector: it skips its destroy packet, flies along `EntityPickupFX`'s squared progress to the
collector's feet plus half a block for three ticks, then goes. Explosion and smoke particles are now implemented (see the sprint execution record below).

Saved sign-in and release pacing checkpoint. A device-code sign-in now keeps Microsoft's refresh token
in the macOS Keychain (generic password, service "Horizon 1.8.9 Microsoft sign-in", account = client
ID) through `horizon_auth::saved`, using `security-framework`; nothing touches disk and errors carry the
OSStatus only. Later `--server`/`--login` runs call `horizon_auth::refresh` (refresh grant, then the same
Xbox Live, XSTS and Minecraft Services chain) and save the rotated token. An `invalid_grant` deletes the
item and falls back to a device code; a network or service failure keeps it. `--sign-out` removes it,
`--no-saved-login` skips the Keychain for one run. Markers: `MICROSOFT_SAVED_LOGIN_PASS`,
`_EXPIRED`, `_UNAVAILABLE`, `MICROSOFT_LOGIN_SAVED`/`_NOT_SAVED`, `MICROSOFT_SIGNED_OUT`. Verified: a
Keychain round trip (`HORIZON_KEYCHAIN_TEST=1 cargo test -p horizon-auth`), and a planted bogus token
that live Microsoft rejected, which the client removed before offering a device code. Not yet verified:
a real account's refresh. Each rebuilt binary has a new ad-hoc signature, so macOS asks once per build
before it reads the item ("Always Allow").

Redraws now follow the window's display: the timer runs at 96% of the reported refresh period (16 ms
at 60 Hz, 8 ms at 120 Hz) and is recomputed on resize and move; FIFO vsync with two queued frames sets
the actual rate. Back-to-back redraws were tried and rejected: requesting the next frame from inside
the redraw starves CoreAnimation of drawables on the main thread and stalled `nextDrawable` for a full
second. Smoke runs now also print `FRAME_INTERVAL_US` (present-to-present, only for frames the display
took, since macOS throttles a covered window) with a count of intervals over twice the median. Release
build (`cargo build --release`, thin LTO) in the 80-player synthetic lobby with the reference entities,
1,200 frames on a 60 Hz panel: frame submission P50 2,005 us, P95 2,334, P99 2,579, max 3,407; frame
interval P50 16,665 us, P95 17,097, P99 17,310, max 18,103, no interval over twice the median. A
covered smoke window still drops to macOS's throttled cadence; the pacing runner shows one window only.

Other players' held items checkpoint. `LayerHeldItem` is ported: the item hangs from the arm's
`postRenderArm` point plus (-1, 7, 1) pixels, chests get the render-type-2 transform, sneaking adds
0.203125, and every item uses its 1.8.9 `THIRD_PERSON` display (handheld tools, bow, fishing rod, the
flat-item depth variants, and the block, carpet, fence, gate, anvil and button transforms), through
`ItemRenderer.renderItem`'s doubling for blocks and `RenderItem.renderItem`'s half scale. `ModelBiped`
poses come from the jar: holding (-pi/10), block-hitting (-3pi/10 and -30 degrees yaw, from the using
flag 0x10 with a sword) and `aimedBow` (both arms along the head); the bow draws its pulling frames
from how long the using flag has been set (`Entity::use_ticks`). A synthetic-player capture checked
holding, blocking, drawing, a held block, sneaking, a fishing rod and an apple from front and side.

Name labels follow `Render.renderLivingLabel`: each label gets a black backing at alpha 0.25 and its
text at alpha 0x20 drawn through walls, then the full text depth-tested; sneaking players get only a
depth-tested backing and faint text (`RendererLivingEntity.renderName`). Labels render in their own
pass on the non-sRGB view, after the world and before the hand, keeping the world's depth, so the
translucency blends in gamma space as vanilla. A capture checked a label behind a wall, a sneaking
label and the backing over sky.

Fishing lines follow `RenderFish`: 16 black segments from the rod to the bobber (its spawn data names
the angler), sagging by `(f*f + f) / 2`. Another player's line starts from their body yaw at
(0.35, eye - 0.45, 0.8), lowered 0.1875 while sneaking; the local player's starts at the camera's
(-0.36, 0.03, 0.35) turned by pitch, yaw and the swing, so it needs the local entity ID, now kept in
`Shared`. They draw as depth-tested line lists in the label pass. Whoever has a bobber out holds
`fishing_rod_cast`, in first and third person. A capture checked another player's line to a bobber.

Enchantment glint: `ItemStack.hasEffect` (enchanted stacks, stored-enchantment books, enchanted
golden apples, potions with effects, bottles o' enchanting, nether stars) adds the
`enchanted_item_glint` passes. Held items, in first and third person, use `RenderItem.renderEffect`
(two additive passes in 0x8040CC, coordinates as a 1024x512 items atlas scaled by 8, scrolled by the
clock and turned -50 and 10 degrees); armour uses `LayerArmorBase.renderGlint` (a third scale, 30 and
-30 degrees, scrolled by entity age, 0.76 of (0.5, 0.25, 0.8)). Glint vertices copy the item's
positions and draw with an equal depth test and `GL_SRC_COLOR, GL_ONE` blending: world glint in the
gamma label pass, the hand's in the hand pass. Interface icons get the same two passes: the GUI
pipeline now blends premultiplied, and a glint fragment (layer flag 0x40000000) masks by the icon's
alpha and outputs its added colour with zero alpha. Potions mask by the bottle, leather by the base
piece. A capture checked an enchanted chestplate on another player, the held enchanted sword and the
hotbar. Known gap: the atlas size is assumed, so the stripe spacing is approximate.

Block cracks follow `RenderGlobal.drawBlockDamageTexture`: the local player's stage is
`(int)(curBlockDamageMP * 10) - 1`, other players' come from S25 (kept by breaker, cleared outside 0-9),
and each draws the block's model with `destroy_stage_N` in the overlay pass, alpha-tested at 0.1,
blended `GL_DST_COLOR, GL_SRC_COLOR` in gamma space and pushed toward the eye by a depth bias. A capture
checked two injected S25 stages on the floor and a wall.

Block-break particles follow `addBlockDestroyEffects`: S28 effect 2001 (state id `id + meta << 12`) and
the local player's own breaks (survival completion and creative hits, resolved against the world before
the server's change) spawn 64 `EntityDiggingFX` on a 4x4x4 grid, flung from the centre, a 4-pixel patch
of the block texture at 0.6 brightness, falling at 0.04 with swept vanilla collision shapes, 0.98 drag and 0.7 ground friction. A bounded reusable shape cache
survives network-write ticks; unknown cells defer motion while lifetime still expires. Fresh world reads
refresh cached shapes. The inherited model `particle` slot supplies the texture, independently of face order. A
capture checked red wool particles from injected 2001 effects landing on the floor.

Next (in order): live Hypixel PvP play test with the release build.

## 2. Verified State

- 146 scoped tests pass with `--include-ignored` (client 65, login CLI 2, assets 1, auth 6, physics 16,
  protocol 38, world 18 — counts include the Metal readback and the owned-jar registry test).
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
   prediction and crafting/special-container rules. Player/chest shift-click, drag-split and double-click
   collect now work (Section 9). Adventure restrictions are enforced conservatively. An
   authenticated Hypixel lobby smoke passed; full release PvP and saved-login reuse remain unverified.
2. Player armor, generated item sprites, held block geometry, depth-tested names and asynchronous
   skin/cape bindings are present, as are first-person hands and first-person item display transforms.
   Third-person item display transforms and enchantment glint (world and interface) are ported. Cape motion currently uses limb/sneak pose; vanilla chasing-
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

Latest recorded gate outcomes are 146 scoped tests passing, strict scoped Clippy and formatting passing, layer and
public-tree checks passing, interaction gate passing, and native equipment/flight smoke passing with zero
server movement flags. Earlier broad-workspace validation recorded macOS path/tool-assumption failures and disk exhaustion.
This checkpoint revalidates the scoped client packages; the whole upstream workspace remains unverified.

Manual play: run the canonical binary with `--offline --username <name> --server 127.0.0.1` against the
reference server. The cursor captures on join; Escape releases it; click recaptures. Keys are WASD, Space,
Shift for sneak, Control or Left Option for sprint, and double-tap Space to fly where allowed. Left/right
mouse drive attack/dig and interact/use. Number keys or the wheel select hotbar slots. E opens the player
inventory; E or Escape closes it. Container slots accept normal left/right pickup clicks. Minecraft
access tokens stay in memory; only the Microsoft refresh token is kept, in the Keychain, so later
`--server` launches sign in without a device code until `--sign-out`.


## 8. Next Claude task — November 19 sprint cut

Read [the live-test plan and sprint priorities](docs/189-hypixel-test-plan.md). The user is conducting
manual public-server testing; do not start another login or rebuild their running test binary. Baseline
is `133e2a9491`. This planning pass successfully built the canonical ARM64 release binary; it did not
repeat the recorded 115-test/reference gate. Use a separate target directory for concurrent builds and
coordinate file ownership before edits. Keep the client runtime pure Rust + Apple Metal.

### Priority override: live failures

Triage returned logs/screenshots first. Any reproducible packet ordering, movement, knockback, hit,
respawn, shop desync or invisible block-entity failure takes precedence over particles/HUD. Record
expected vanilla behavior, produce a local regression, fix it, then request a targeted manual retest.
Do not treat no kick as Watchdog certification. Preserve f64/f32/update order and fixed 50 ms ticks.
Audit that autopilot/scripted smoke actions cannot target a non-loopback destination; add a guard and
rejection coverage if missing. Keep all synthetic packet fixtures local.

### Self-contained parallel task: TNT/explosion/smoke

1. Inspect `particles.rs`, `network.rs`, protocol `player.rs`, world `entities.rs`, model material
   resolution and owned-jar 1.8.9 bytecode before implementing. Reuse existing primed-TNT fuse/flash
   state and particle batching. Confirm legacy particle IDs against vanilla.
2. Current S27/0x27 `decode_explosion` returns only knockback. Add bounded rich decoding/event delivery
   for centre, strength and affected records, retaining the existing API where callers need it. Apply
   explosion velocity exactly once; preserve vanilla affected-block handling and outbound behavior.
   Handle S2A/0x2A particle packets for normal/large/huge explosion and normal/large smoke, with vanilla
   extra-data rules, bounded counts and malformed/truncated-packet coverage. Add local fuse smoke.
3. Port vanilla lifetimes, sizes, frame selection, tint and blend using owned textures, including the
   large explosion animation sheet and huge-explosion child emission. Use bounded visual events and
   reusable batches. Simulation stays at 20 Hz; no per-particle network locks or render-thread I/O.
4. Resolve block-break fragments through the inherited model `particle` material, rather than its first
   baked face. Fix floor tunnelling during world-write contention without blocking the Metal thread
   or skipping collision. Choose a bounded cached collision snapshot or equivalent and document it.
5. Tests: packet bounds/truncation and finite geometry; lifetimes/emission limits; signed explosion
   velocity applied once; log/bark material inheritance; collision during world lock contention. Supply
   a deterministic local capture/clip of fuse → explosion/smoke, fragments landing and unaffected
   player knockback. Preserve the existing bounded particle budget; do not enable public injection.

### Next highest-priority PvP usability gap

Implement vanilla drag-to-split and double-click-to-collect in player/chest inventories after P0 reports
and particles. Cover left/right drag start/add/end, valid slot and stack limits, cursor remainder,
collect ordering, action numbers, rejected transaction recovery and authoritative slot resync. Compare
with owned 1.8.9 behavior and validate against the official reference server. Keep unsupported windows
on safe resync fallback. Do not expand to brewing or an inventory player model this sprint.

Then add minimal own FPS/CPS and adjustable FOV. CPS counts physical click events without changing
packet generation; FOV changes camera projection without changing reach, rays or movement semantics.
Defer keystrokes/zoom if release gates are at risk. Promote unreadable server label symbols when live
reports show lost navigation or gameplay information.

### Acceptance and handoff

- Run Section 7 scoped tests/strict Clippy/fmt/layer/public-tree checks and managed
  `python3 tools/test_189_reference.py` after each implementation milestone. The managed runner supplies
  the environment for `--include-ignored`; do not invoke fixture-dependent ignored tests bare.
- Inspect a native release Metal capture and timing, with visible/offscreen counts separated. Do not
  replace existing movement/entity coverage with a particle-only smoke. Re-run stair/Depth Strider or
  flight parity when those paths change or a regression warrants it.
- Inspect the staged diff and new commit range for credentials, account names and local paths. Keep
  owned assets, server state and raw captures/logs outside public git. Use the configured GitHub noreply
  identity. Commit only scoped work, push HEAD to origin/main after gates, verify the remote hash, and
  leave the backup branch local. Do not include another agent's concurrent edits accidentally.
- Update Sections 1/2/5 with actual results and return commit, exact gate commands/exit codes/test counts,
  private evidence paths, remaining limitations and manual live-test status. Keep reported baseline
  evidence distinct from checks performed for the new revision.


## 9. November 19 fallback queue execution

### Explosion/smoke checkpoint — `5c78cbd671`

Bounded S27/S2A decoders retain signed affected-block records and particle parameters; explosion block
changes remesh and player knockback remains applied once. The window receives bounded visual events.
Owned-jar bytecode `bdp`, `beh`, `bdx`, `bdu`, `bdv`, `bec` and `cy` informed normal/large/huge explosions,
smoke, sheet animation and fuse emission. All sixteen explosion cells survive texture-layer upload.
Validation scripts now reject non-loopback server names; movement also checks resolved socket addresses.

Full gate: 120 scoped tests (41 client, 2 login CLI, 1 assets, 6 auth, 16 physics, 37 protocol, 17 world),
zero failures; strict scoped Clippy, fmt, layer/public-tree and diff privacy checks passed. Managed official
server result: `VANILLA_189_REFERENCE_GATE_PASS`. Native ARM64 release build passed. A private 960x600
release capture was inspected: explosion clouds, smoke and the reference terrain/chest render correctly;
500 frames, 50 columns, one offscreen frame. This is a visual fixture, not live match or display certification.
Evidence remains outside git in the private `h189-sprint-effects` capture directory and `h189-effects-*`
logs. Live Hypixel and real Keychain reuse are still pending user testing.

Next: inherited particle material and busy-tick collision; then player/chest drag-split and collect.


### Block-break material and collision checkpoint — `3633dab1bd`

Inherited/rebound `particle` slots now bake into the registry. The owned-jar test checks oak-log fragment
pixels against bark, and the assets test checks inherited texture aliases. Fragments use the existing
vanilla swept AABB collision operations, including thin shapes, rather than endpoint/full-cube tests.
A 32,768-cell reusable shape cache retains collision on busy network ticks. Missing cache coverage
conservatively defers motion; particle age still advances, bounding lifetime and memory. Smoke uses the
same collision path. Regression tests cover high-speed floor clipping, five unavailable-world ticks,
refresh after floor removal and an initially missing snapshot.

Full gate passed: 122 scoped tests (43 client; other counts unchanged), strict Clippy, fmt, layers,
public-tree/privacy and official reference gate. ARM64 release built. The private
`h189-sprint-fragments/capture.png` was inspected: red-wool and bark fragments appear above and on the
stone-brick floor. Release smoke: 500 frames, 25 columns, one offscreen frame. Logs use the private
`h189-fragments-*` prefix. Next: drag-split and double-click collect.


### Inventory drag and collect checkpoint — `8e04e6bc48`

`Container.slotClick` mode 5/6 follows the owned 1.8.9 `xi` bytecode: original-count equal shares or
one per slot, stack/armour/output validity, duplicate filtering, remainder, and two-pass collect with
partial stacks before full stacks. Mouse gestures have a 250 ms double-click threshold, drag count
preview and cancellation on focus loss/window changes. Window-bound quick-craft batches submit on
release; the actor reserves transaction capacity before sending any stage. Physical input remains the
only gameplay trigger. Rejected transactions block new gestures until authoritative resync.

Unmodeled special containers keep their existing ordinary-click path; drag/collect is disabled there.
Those modes return null even after changing slots, so claiming that null alone forces resync would be
incorrect. Creative middle-button fill and shift-double-click bulk transfer are not implemented in this
checkpoint. Player crafting result collection is excluded; brewing and inventory player model stay deferred.

Full gate: 130 scoped tests (51 client, 2 login CLI, 1 assets, 6 auth, 16 physics, 37 protocol, 17 world),
zero failures, strict scoped Clippy/fmt, layers, public-tree and diff privacy checks passing. The official
reference gate independently queried chest NBT after the split (one diamond in each of slots 1 and 2)
and after collect (empty), in addition to placement/mining, combat, spectator and death/respawn checks:
`VANILLA_189_REFERENCE_GATE_PASS`. New units exercise distribution, slot limits, cursor remainder,
collect ordering, item matching, rejection/resync, gesture cancellation and preview/commit agreement.

Native ARM64 release built. The private `h189-sprint-inventory/capture.png` was inspected: filled slots,
stack counts, sword and tooltip render. It is a static inventory capture; gesture semantics are covered
by units and authoritative chest NBT checks. Release smoke: 500 frames, 25 columns, one offscreen frame.
No claim of manual Hypixel testing or successful real-account Keychain refresh is added.

### Exact next task after the user's live test

Review the user's release sign-in/relaunch, Lobby → Duels → BedWars logs/screenshots first. Reproduce
any movement/packet/knockback/hit/respawn/shop failure locally, fix it and rerun this full gate before
requesting a targeted live retest. The passive FPS/CPS HUD and configurable FOV are now implemented
(Section 10). If gameplay passes, continue repeated full-game validation and the planned display/resize/
focus regression checks before feature freeze.
Do not start deferred zoom, keystrokes, brewing, inventory player model or cosmetic polish. The fallback
queue of explosion/smoke, block fragments and inventory gestures is complete; Section 8 remains its
original task specification rather than a request to repeat it.


Final inventory review adds the `GuiContainer` crafting-result guard: a double-click on that slot does
not emit collection, even when it is empty. The collect regression also distinguishes equal item IDs
with different NBT. Final full gate: 131 scoped tests (52 client; remaining package counts unchanged),
strict Clippy/fmt/layers/public-tree/privacy and managed reference gate passing. The separate
`horizon-client-input` suite also passes 209 tests. Static release captures are recorded above; the last
change affects gesture admission, covered by the added GUI unit and the repeated reference gate.


## 10. Passive PvP metrics and world FOV checkpoint — `2190653827`

`--pvp-hud` enables two translated top-left labels, FPS and left/right CPS; it is off by default.
CPS measures OS left/right button-down transitions over a rolling second while focused, captured and
alive in the world. Duplicate downs, held actions, GUI/chat input and recapture clicks do not count.
Focus/cursor release clears click state. Metrics have bounded 4,096-event queues and never send actions.
FPS samples only visible render completions (offscreen frames excluded), includes idle/stall time and
shows a warm-up marker until a 250 ms span exists. It does not measure GPU duration or physical scanout.

`--fov F` configures vertical world FOV from 30 through 110 degrees, default 70; values outside that
range and non-finite values fail parsing. It is a launch setting, with no new runtime key bindings or
settings screen. The value survives mesh-origin changes and surface recovery. Only projection/culling
changes: centre selection ray, reach, movement, outgoing actions and the hand's existing fixed 70-degree
projection remain unchanged. Zoom, keystrokes and other deferred features remain outside this milestone.

Verified: 137 scoped tests (58 client, 2 login CLI, 1 assets, 6 auth, 16 physics, 37 protocol, 17 world),
strict scoped Clippy, fmt, layer/public-tree and diff privacy checks, native ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`. New regressions cover transition counting, expiry, ineligible input,
focus release, bounded storage, 60/240 Hz windows with stalls, FOV validation and centre-ray/near-plane
invariance. The prior 209-test input-suite result remains recorded in Section 9; this milestone does not
change that crate.

Private release captures `h189-sprint-pvp70/capture.png` and `h189-sprint-pvp100/capture.png` were inspected:
both show readable FPS and idle 0/0 CPS; 100 degrees exposes more of the same coloured-pillar fixture,
while the sword remains the same screen size. Each renders 500 frames, 25 columns, one offscreen frame.
These are stationary visual tests, not a live-match FPS promise or manual CPS validation. Evidence logs
use the private `h189-pvp-*`, `h189-pvp70-*` and `h189-pvp100-*` prefixes, outside git.

Next: user release sign-in/relaunch and Lobby → Duels → BedWars testing from the live-test plan. Triage
any returned gameplay/packet/inventory failure before widening public play. If that passes, repeat complete
games and available Retina/60/120/240 Hz focus/resize checks; address demonstrated regressions and
release blockers before the November 30 freeze. No new live login or real-account refresh was attempted.


## 11. Respawn lifecycle and native validation checkpoint — `5ce448bdf3`

A real actor/socket regression first failed on the previous code: same-dimension S07 discarded its only
loaded column. The owned 1.8.9 `bcy.a(he)` bytecode creates a WorldClient only when the dimension changes;
`ave.a(int)` recreates the local player every time, and `bda.b(wn)` initializes its yaw to -180 degrees.
The corrected actor preserves chunks/nearby entities on same-dimension respawn, recreates player physics,
potions, attributes, controller, reporter and hand state, and applies new game-mode capabilities. On a
dimension change it clears chunks, world entities, cracks and pending old-world effects; connection-level
player profiles survive so new player spawns retain known skin properties. Player/world epochs cancel
stale inventory gestures/CPS state and old-world particle/collision caches in the window.

S07 now has a bounded complete decoder, including its signed dimension and level-type string. S08
coordinates outside the block simulation's range are rejected before an acknowledgement is emitted.
The actor tests exercise actual framed outgoing bytes: settings before brand, keep-alive echo,
immediate C06 ground=false, retained same-world terrain/entities, ordinary jump height after an old
Jump Boost effect, new-player relative rotation, invalid-position rejection and dimension cleanup.
A separate world regression checks that a player spawned after a dimension change retains its profile,
while a fresh connection clears it.

Verified full gate: 141 scoped tests (60 client, 2 login CLI, 1 assets, 6 auth, 16 physics, 38 protocol,
18 world), strict scoped Clippy/fmt, layers, public-tree/privacy, native ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`. The explicit `HORIZON_KEYCHAIN_TEST=1` throwaway-service test also
passed its save/replace/read/delete cycle; it does not access or validate the real account's refresh.
The previous separate 209 input tests remain recorded; input crate source is unchanged.

Native macOS validation used a temporary app bundle containing a copy of the Rust executable, outside
git, so accessibility tools could select its window. Maximize/restore was visually checked at drawable
960x600 → 3440x1378 → 960x600, backing scale 1. Focus false/true events were logged and rendering/HUD
recovery was observed. The completed local run rendered 12,000 frames over 52,424 ms, 25 columns, one
offscreen frame; its final 960x600 capture was inspected. Frame statistics cover the last 1,024 samples,
not every resize frame. No Retina/backing-scale-2 or complete 60/120/240 Hz matrix is claimed.
The first unbundled attempt exited normally before its scheduled capture and is not counted as a
completed capture gate. All app/server state, captures and logs remain private outside git, under the
`h189-lifecycle-*` prefixes; the completed image is `h189-lifecycle-ui-app/capture.png`.

Next: actual account sign-in/relaunch and complete Lobby → Duels → BedWars sessions from the test plan.
Use returned failures to choose the next P0 fix. Remaining manual evidence includes successful real-token
refresh, shops/knockback under live latency, complete matches and the remaining display/backing-scale
matrix. Deferred features remain deferred; this pass made no public-server connection.


## 12. Inventory synchronization checkpoint — `dc4fe50f66`

Two regressions reproduced stale player rows and stale held-item prediction. S30 window 0 now updates
InventoryPlayer through the same slot-mirroring path as individual S2F changes, so an open chest/shop
reflects player main-inventory and hotbar updates without replacing its own contents. Rejected C0E
prediction now restores both the visible snapshot and the held/player mirror immediately, while further
clicks remain blocked until the authoritative window resync. This follows `bcy.a(gd)`'s routing to the
shared InventoryPlayer slots in the owned 1.8.9 client.

The units failed before the fix and pass afterward, including rollback in player and chest windows.
A framed actor test sends open-window, chest items, full player items, an unrelated window update and
server close, and verifies the correct rows through the whole sequence. A private loopback proxy then
exercised the same window-0 update against the native release client over an official server connection.
Its inspected 960x600 capture shows three diamonds retained in the chest, seven in the updated player
row and five apples in the hotbar/cursor hand. It rendered 800 frames, 25 columns, one offscreen frame.
The synthetic window packets are localhost-only; the server does not interpret this visual fixture as
actual shop transactions. Raw evidence remains under private `h189-inventory-sync-*` paths outside git.

Full gate: 144 scoped tests (63 client, 2 login CLI, 1 assets, 6 auth, 16 physics, 38 protocol, 18 world),
strict scoped Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`. Deferred container features remain deferred. Real-account saved
refresh and complete live matches still require the user's manual session.


## 13. Game-mode capability transition checkpoint — `b8b2dc569d`

A framed actor regression reproduced a player hovering at y=8 after S2B reason 3 changed the mode to
survival without a following S39 packet. The owned client `bcy` handler calls PlayerControllerMP's
mode setter, which immediately invokes `adp$a.a(wl)` (configurePlayerCapabilities). The actor now uses
that same capability policy on Join Game, Respawn and Change Game State. Survival/adventure disable
old flight, creative preserves the current flying choice while allowing flight, and spectator enables
flight/no-clip. Configured walk/fly speeds survive mode-only changes. Server state changes do not set
the client flight-toggle latch or synthesize C13 packets; authoritative S39 remains a separate update.
Unknown IDs use vanilla's survival fallback.

The hovering test failed before this fix and passes afterward for both survival and adventure. A
transition matrix also checks creative's preserved flying choice, spectator flags, speed retention and
absence of pending client ability toggles. The official reference gate still covers spectator travel,
combat/placement/mining, inventory split/collect and death/respawn.

Full gate: 146 scoped tests (65 client, 2 login CLI, 1 assets, 6 auth, 16 physics, 38 protocol, 18 world),
strict Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`. Logs remain private under `h189-gamemode-*`. This behavioral change
has packet/physics evidence and the native reference smoke; the separately inspected inventory-sync
capture is recorded in Section 12. No new visual feature or public-server script was introduced.

Next: manual real-account saved-login/relaunch and complete live Duels/BedWars sessions using the test
plan. Saved-login error handling was reviewed; no new account sign-in was started in this pass. Use
actual live failures to choose further P0 work, and complete the remaining display/backing-scale matrix.


## 14. Disconnect recovery checkpoint — `4707a5d149`

Normal connected sessions retain a login/play disconnect or transport failure reason in a translated,
opaque connection-lost screen instead of exiting. Server JSON components and plain IO errors use the
same text formatter. Reconnect (button or Enter) deliberately starts a new connection to the original
host/port; Quit and Command-Q exit (Escape no longer does; see section 21). A connecting screen disables duplicate retries and blocks
gameplay input until the first authoritative player position. Bounded smoke sessions still fail fast.

Retry shares the original memory-only Session via Arc; it does not reopen Keychain, refresh an expired
access token, or request Microsoft approval. An expired session still requires normal application
relaunch/sign-in. Disconnect cancels and joins the old transport/mesher, releases all input/cursor
state, clears world/entity/label/hand geometry and skin bindings, and resets HUD/chat/particles. The
new actor owns independent world, inventory and player state. GPU resources and display settings stay.

Verification: 150 scoped tests (69 client, 2 login CLI, 1 assets, 6 auth, 16 physics, 38 protocol,
18 world), strict Clippy/fmt, layer/public-tree/diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`. The full loopback retry test rejects the first login, accepts the
second using the same Session, checks fresh input/chat state and the immediate S08/C06 acknowledgement,
and joins both transports on drop. Metal readback proves old fixture geometry disappears and captures
the recovery screen: title, red formatted reason, highlighted Reconnect and Quit are readable in the
inspected 256x256 image. Private evidence remains under `h189-reconnect-*`, including
`h189-reconnect-ui/connection.png`; no assets or captures enter git.

A normal native release run retained a connection-refused failure with the window alive and later
exited with status zero; its paired bounded run returned failure as required. Computer-use controls
were unavailable for this pass: no native button-click/reconnect-after-kick interaction is claimed.
The reconnect transport and button hit/state transitions have automated coverage, while that native
interaction remains manual. No public-server connection or new Microsoft approval was attempted.

Next: manual actual-account saved-login/relaunch, then complete Lobby → Duels → BedWars sessions from
the test plan. On a naturally occurring disconnect, capture the retained reason before any deliberate
retry; stop live testing on warnings/rubberbanding/inventory faults and reproduce locally first.
Successful real-token refresh, complete live matches, native retry interaction and the remaining
Retina/high-refresh display matrix remain unverified. Deferred features remain deferred.


## 15. Held-item (C09) sync checkpoint

`Controller` now ports `PlayerControllerMP.currentPlayerItem`/`syncCurrentPlayItem` (bytecode `bda.n`,
called from `e` updateController, `c(cj,cq)` onPlayerDamageBlock, onPlayerRightClick, sendUseItem
outside spectator, attackEntity, both entity right-click paths and onStoppedUsingItem; not from
clickBlock or windowClick). A tick starts with the comparison `selected != last_sent → C09`, and the
same check runs immediately before each of those packets, so a selection made in the tick it is used
gives vanilla's C0A, C09, C02 order. A key/wheel selection now only changes `currentItem`: it no longer
sends C09 at once, an `ABORT_DESTROY_BLOCK`, or `RELEASE_USE_ITEM`. The item in use is cleared locally
as `EntityPlayer.onUpdate` does, and a dig restarts through onPlayerDamageBlock's tool comparison.
`blockHitDelay` now counts down only while a block is targeted, after the sync.

S09 is read as one byte; `InventoryPlayer.isHotbar` ignores values outside 0-8 (including negative
bytes). Server-forced S09 slots are synced once on the next tick. Respawn (same or other dimension)
creates a new player whose inventory selects 0 while the controller keeps its last sent slot, so a
non-zero slot produces one C09 0. A second Join Game builds a new controller and inventory (nothing
sent, slot 0); a reconnect starts a new actor and controller.

Regressions written first and failing before the fix: `hotbar_selection_syncs_like_player_controller_mp`
(selection-only tick silent, select+attack order, no release when switching off a blocking sword,
duplicate selection silent), `forced_hotbar_changes_sync_once_and_survive_respawn`, and
`mid_session_join_starts_a_fresh_controller_and_inventory_without_c09`; the S09 decoder has a
0-255 table test. Gate: 154 scoped tests (73 client, 2 login CLI, 1 assets, 6 auth, 16 physics,
38 protocol, 18 world), strict Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release
build and `VANILLA_189_REFERENCE_GATE_PASS`.

Noted for the parity audit: spectator left-clicks on an entity send no C02 here, while vanilla's
attackEntity sends one (spectate target).


## 16. Right-click use parity and stairs re-verification

Section 15's C09 fix is `43a7b19e26`.

`Minecraft.rightClickMouse` is followed more closely. A block click sends C08 for the block; when
neither `onBlockActivated` (unless sneaking with an item) nor the item's `onItemUse` consumes it, the
held item's `sendUseItem` follows for any item, so a golden apple, food or potion aimed at the floor is
now eaten or drunk (previously only swords and bows were used). The swing now happens only for a
consumed block click, as vanilla. `item_use_on_block` models which items consume: block items,
flint and steel, fire charges, spawn eggs, fireworks, armour stands, and the conditional ones only on
their targets (hoes on dirt/grass, seeds/carrots/potatoes on farmland, wart on soul sand, paintings and
frames on sides, doors/beds on tops, signs/skulls not on bottoms, minecarts on rails, bone meal on
growable blocks, cocoa on jungle log sides, leads on fences). An entity click sends INTERACT_AT; an
armour stand's interactAt consumes it (no INTERACT, no use), villagers and horses consume INTERACT,
and otherwise any held item is used. `sendUseItem` sends nothing in spectator.

Regression `right_click_uses_held_food_after_unconsumed_block_and_entity_clicks` failed first (a swing
where the apple's C08 belonged). Gate: 155 scoped tests (74 client, 2 login CLI, 1 assets, 6 auth,
16 physics, 38 protocol, 18 world), strict Clippy/fmt, layers, public-tree and diff privacy checks,
ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.

Stairs: a rebuilt private reflection harness on the official server jar (`kb.c()` bootstrap, a proxy
`adq` block access, `aju.e`/`h`/`i` with bounds fields `B`-`G`) prints all 8 stairs metadata against
every air/stairs mix on four sides; its 52,488 lines are byte-identical to
`cargo run -p horizon-physics-189 --example stairs_probe`. The harness stays private.


## 17. Hit slow-down and spectator attack parity

Section 16's right-click fix is `0c7c5571c9`.

`attackTargetEntityWithCurrentItem` applies `motion *= 0.6` and `setSprinting(false)` only when the
knockback level is positive (the Knockback enchantment, id 19, plus one while sprinting) and the
target's client-side `attackEntityFrom` succeeded. On the client that is `EntityOtherPlayerMP`
(always true), boats and minecarts; mobs return false, so hitting a zombie while sprinting no longer
slows or unsprints the player, and a Knockback sword now slows a non-sprinting hit on a player.
Spectators now send attackEntity's C02 after the swing (the server spectates the target) while local
hit effects stay skipped.

Regressions written first: `hit_slowdown_needs_knockback_and_a_player_target` (failed on the
Knockback case) and `spectator_attack_sends_c02_to_spectate` (no C02). Gate: 157 scoped tests
(76 client, 2 login CLI, 1 assets, 6 auth, 16 physics, 38 protocol, 18 world), strict Clippy/fmt,
layers, public-tree and diff privacy checks, ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.


## 18. Transaction ping parity

Section 17's hit fix is `e85df02ee5`.

`handleConfirmTransaction` answers an unaccepted S32 with C0F (accepted) when the window is 0 (the
player's own container, even while a chest is open) or the open window. Before, a window-0 S32 under an
open container got no reply, and any unaccepted S32 on the current window, including server pings with
action numbers this client never used, restored a stale view, dropped pending clicks and blocked
further clicks until the next S30. Now only a transaction this client numbered for the open window
undoes a predicted click; pings are answered and change nothing. Accepted confirmations and other
windows still get no reply. Regression `transaction_pings_are_answered_without_touching_clicks` failed
first ("the ping blocked clicks").

Audited without change: C0B sprint/sneak actions are sent from `onUpdateWalkingPlayer` order (sprint,
then sneak, then movement) against the last sent states, reset on respawn; a hit's `setSprinting(false)`
is followed in the same tick by the walking update's sprint-key re-sprint, as vanilla; attacks are
drained while an item is in use. Gate: 158 scoped tests (77 client, 2 login CLI, 1 assets, 6 auth,
16 physics, 38 protocol, 18 world), strict Clippy/fmt, layers, public-tree and diff privacy checks,
ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 19. Death with a screen open

Section 18's transaction fix is `0262333bf7`.

`GuiContainer.updateScreen` calls `EntityPlayerSP.closeScreen` (C0D with the open window id, cursor
cleared) once the player is not alive, and `Minecraft.runTick` shows `GuiGameOver` only after no screen
is open. Before, dying with a chest or the player's own inventory open sent no C0D: the container stayed
open (hidden) under the death screen and the server kept it open until respawn. Now the first dead tick
with a container open sends C0D and closes it locally; the respawn button still waits for that tick.
Regression `dying_with_a_container_open_closes_it_like_gui_container_update_screen` failed first (no C0D).
Gate: 159 scoped tests (78 client, 2 login CLI, 1 assets, 6 auth, 16 physics, 38 protocol, 18 world),
strict Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`.

## 20. Depth Strider after predicted armour clicks

Section 19's death-close fix is `02eea81f2c`.

`EnchantmentHelper.getDepthStriderModifier` reads the worn armour on every `moveEntityWithHeading`.
The client recomputed the level only on S2F/S30, but the server absorbs an accepted window click
without echoing slots, so boots put on by clicking left water movement at the old level until some
unrelated slot update. The level is now read from the player inventory each tick before movement.
Regression `depth_strider_follows_predicted_armor_clicks` (water walk with and without clicked-on
Depth Strider III boots) failed first with identical distances.

Audited without change: S39 copies flying, allow-flying, creative, invulnerable, fly and walk speed as
`handlePlayerAbilities`; C13 is sent from the double-tap toggle, spectator forcing and landing, as
`sendPlayerAbilities`. Known gap, not implemented: the dynamic FOV of
`AbstractClientPlayer.getFovModifier` (sprint, speed effects, bow pull, walk speed) is absent; the world
FOV is fixed at the configured value. Gate: 160 scoped tests (79 client, 2 login CLI, 1 assets, 6 auth,
16 physics, 38 protocol, 18 world), strict Clippy/fmt, layers, public-tree and diff privacy checks,
ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 21. Connection screen keys

Section 20's Depth Strider fix is `13e1bde909`.

Escape on the connecting/connection-lost screen exited the application. Escape is the in-game key for
menus, chat and containers, so one pressed at the moment of a kick closed the client and lost the
retained reason. `GuiDisconnected` and `GuiConnecting` ignore typed keys; Escape now does nothing there.
Command-Q and the Quit button still exit, Enter still retries only once disconnected (never while
connecting). Key handling moved into `connection::Screen::key_action`; regression
`escape_neither_quits_nor_retries_like_gui_disconnected` failed first (Escape returned Quit).

Audited without change: disconnect releases cursor, keyboard, container pointer, tab and attack/use
input before the old actor is dropped; mouse position resets so an in-flight attack click cannot land on a
button; reconnect builds a new actor with fresh world, inventory, controller and player state; a failed
start returns to the lost screen. Gate: 161 scoped tests (80 client, 2 login CLI, 1 assets, 6 auth,
16 physics, 38 protocol, 18 world), strict Clippy/fmt, layers, public-tree and diff privacy checks,
ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 22. Drop key without local prediction

Section 21's connection-key fix is `aa4f8fa7cb`.

`EntityPlayerSP.dropOneItem` sends C07 DROP_ITEM (Q) or DROP_ALL_ITEMS (Control-Q) at the origin
facing down and returns without touching the inventory; the hotbar changes when the server's S2F
arrives. The client removed one item (or the stack) locally and sent nothing for an empty hand. On a
server that cancels the drop and restores the stack unchanged, no S2F is sent, so the predicted
removal left a phantom missing item. Now C07 is always sent outside spectator and the stack waits for
the server. Regression `drop_key_sends_c07_without_predicting_the_stack` failed first (count 3, not 4).
Gate: 162 scoped tests (81 client, 2 login CLI, 1 assets, 6 auth, 16 physics, 38 protocol, 18 world),
strict Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`.

## 23. Number-key clicks follow `slotClick` mode 2

Section 22's drop fix is `8c5690458c`.

`Container.slotClick` mode 2 only swaps when the hotbar slot is empty or the hovered slot is the
player's own and accepts the hotbar stack. Otherwise, with an empty slot anywhere in the player's
inventory, the hovered stack comes to the hotbar and the old hotbar stack goes through
`addItemStackToInventory` (partial stacks first, then the first empty slot, hotbar first), leaving
the hovered slot empty; with no empty slot nothing moves. The server answers mode 2 with no item, so
it accepts any C0E and never resends the slots. The client swapped blindly, so pressing a number
key over a chest slot while holding something put the held stack into the chest on screen only:
a phantom item until the next full window update (common when looting). It also never sent number-key
clicks over the armour or crafting-grid slots, which vanilla sends. Prediction now follows the jar,
with `Slot.isItemValid` for armour (armour type, pumpkin or skull on the head), crafting/furnace
outputs and plain container slots. Clicks whose result is not modelled (a crafting result with a
stack, furnace fuel, other generic windows) are still not sent, as before. Regression
`number_keys_follow_slot_click_mode_two` failed first. Gate: 163 scoped tests (82 client, 2 login CLI,
1 assets, 6 auth, 16 physics, 38 protocol, 18 world), strict Clippy/fmt, layers, public-tree and diff
privacy checks, ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 24. Pickup clicks respect slot rules

Section 23's number-key fix is `1812ca6de7`.

`slotClick` mode 0 returns the clicked slot's original stack, so the server accepts a pickup click
whatever the client predicted and never resends the slots. The client ignored `Slot.isItemValid`:
clicking dirt onto an armour slot, a crafting result or a furnace output showed it there while the
server kept it on the cursor, and a stack of pumpkins went whole onto the head. Prediction now follows
the jar: placing, merging and swapping need a valid slot, armour slots hold one item, and an output
slot only empties into a matching cursor that can take it all. Windows this client does not model still
accept anything, as before. Regression `pickup_clicks_respect_slot_validity_and_limits` failed first.
Known gap: taking a crafting result does not predict the grid's ingredient use. Gate: 164 scoped tests
(83 client, 2 login CLI, 1 assets, 6 auth, 16 physics, 38 protocol, 18 world), strict Clippy/fmt,
layers, public-tree and diff privacy checks, ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 25. Dead-player ticks

Section 24's pickup fix is `03373f0361`.

`EntityLivingBase.onDeathUpdate` counts `deathTime` while health is zero and calls `setDead` at 20;
`World.updateEntities` then stops updating the player until respawn replaces it. Until then the dead
player still runs its living update with movement blocked (no input, but gravity, knockback and
sprint stop apply) and reports through `onUpdateWalkingPlayer`. The client froze physics at death and
kept sending a stationary C03 every tick indefinitely. Now a dead player moves without input for 20
ticks, sending its C0B/C13/movement packets, then sends no movement until health returns on respawn.
The S08 reply, keepalives, held-item sync and the death-time C0D are unaffected. Regression
`dead_player_moves_for_twenty_ticks_then_stops_reporting` failed first (32 reports in 30 ticks).
Gate: 165 scoped tests (84 client, 2 login CLI, 1 assets, 6 auth, 16 physics, 38 protocol, 18 world),
strict Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`.

## 26. Respawn key waits for the death screen

Section 25's dead-player fix is `b884c336b4`.

`GuiGameOver` enables its Respawn and Title-screen buttons twenty ticks after it opens, so a click
already in flight at the moment of death cannot respawn. The mouse path honoured that delay, but the
Enter shortcut (not in vanilla) sent C16 immediately, so Enter pressed to send chat as the player
died respawned at once. Enter now waits for the same delay through `Hud::respawn_ready`. Regression
`respawn_waits_for_the_death_screen_buttons` failed first. Gate: 166 scoped tests (85 client, 2 login
CLI, 1 assets, 6 auth, 16 physics, 38 protocol, 18 world), strict Clippy/fmt, layers, public-tree and
diff privacy checks, ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.
