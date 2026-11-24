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
non-zero slot produces one C09 0. A second Join Game builds a new controller but keeps the player
(corrected in section 32); a reconnect starts a new actor and controller.

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

## 27. Taking a crafting result uses the grid

Section 26's respawn-key fix is `1e6ec47c1c`.

`SlotCrafting.onPickupFromSlot` uses one of each grid ingredient whenever the result is taken by a
pickup click, a number key or Q (Q uses the whole grid once even when dropping one item), and a
milk, water or lava bucket leaves an empty bucket in the inventory (or in its emptied grid slot when
the inventory is full). The server accepts these clicks without resending the grid, so the client
kept showing ingredients that were gone. These takes now consume the grid and clear the result; the
next result comes from the server (CraftBukkit-based servers send it with S2F after every grid
change, since `EntityPlayerMP` never sends `SlotCrafting` contents itself). Shift-clicking a result
repeats crafts against recipes this client does not have, so it predicts nothing and sends no clicked
item: a server that crafted anything rejects it and resends the window. Number keys over a result,
previously not sent, now follow mode 2. Remaining gap: no local recipe matching, so on a vanilla
server the result slot only appears after a window resync. Regression
`taking_a_crafting_result_uses_the_grid` failed first. Gate: 167 scoped tests (86 client, 2 login CLI,
1 assets, 6 auth, 16 physics, 38 protocol, 18 world), strict Clippy/fmt, layers, public-tree and diff
privacy checks, ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 28. Item use starts only when `onItemRightClick` allows it

Section 27's crafting-grid fix is `defc197cf0`.

`PlayerControllerMP.sendUseItem` sends C08 for any held item, but the item decides whether use
starts: `ItemFood` only when `canEat` (hunger below 20, or always for golden apples, and never while
`disableDamage`), `ItemBow` only in creative or with an arrow in the main inventory. The client started
use for every food and bow, so right-clicking food at full hunger or a bow without arrows slowed the
player to use speed, stopped sprinting and dropped attacks while held, none of which the server
applied. Use now starts as in the jar; `Controller::invulnerable` mirrors S39. C08 is still sent in
every case. Regression `use_starts_only_when_on_item_right_click_would` failed first (steak at full
hunger). Gate: 168 scoped tests (87 client, 2 login CLI, 1 assets, 6 auth, 16 physics, 38 protocol,
18 world), strict Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`.

## 29. Client-side `EntityPlayer.onDeath`

Section 28's item-use fix is `ac74a03308`.

S19 status 3 for the player's own entity runs `handleStatusUpdate(3)`, which calls
`EntityPlayer.onDeath`: the box shrinks to 0.2 by 0.2 (recentred), motionY becomes 0.1 and
motionX/Z drift 0.1 away from the yaw (`attackedAtYaw` is zero on the client). The server's dead
player uses the same 0.2 box. Since section 25 the dead body moves for twenty ticks, and with the
client still colliding as a 0.6 player while the server collided as 0.2, the reference gate's
spectator-in-the-floor death occasionally logged "moved wrongly" (about one run in twenty). The
physics player now has a size (`Player::width`/`height`, 0.6 by 1.8 until `on_death`) and the
client applies `onDeath` on S19 status 3. Not ported: the client-side `dropAllItems` of the same
method. Regressions `death_shrinks_the_box_and_starts_a_hop_and_drift` (physics) and the extended
`dead_player_moves_for_twenty_ticks_then_stops_reporting` (no hop or drift before) failed first.
Gate: 169 scoped tests (87 client, 2 login CLI, 1 assets, 6 auth, 17 physics, 38 protocol, 18 world),
strict Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`, plus repeated reference runs without movement flags.

## 30. C0A on every swing

Section 29's death fix is `d63bb4d9b5`.

`EntityPlayerSP.swingItem` calls `EntityLivingBase.swingItem` (which restarts the arm animation only
when idle or past half of it) and then sends C0A unconditionally. The client sent C0A only when the
animation restarted, so the second of two clicks within three ticks sent its C02 with no C0A before
it, a no-swing pattern anticheats look for, and digging swung every third tick instead of every
tick. C0A is now sent on every swing; only the local animation keeps the half-way rule. Regression
`every_swing_sends_c0a_even_mid_animation` failed first (click 1 sent C02 without C0A). Gate: 170
scoped tests (88 client, 2 login CLI, 1 assets, 6 auth, 17 physics, 38 protocol, 18 world), strict
Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`.

## 31. Entities beyond 3.0 blocks hide blocks behind them

Section 30's swing fix is `6d71a87815`.

`EntityRenderer.getMouseOver` traces entities along the block reach (4.5 in survival, 6.0 with
creative's extended reach), choosing the nearest one closer than the block hit. In survival, if that
entity is farther than 3.0 the result becomes a miss: the block behind it is not selected. The client
traced entities only to 3.0, so looking at a player 3 to 4.5 blocks away with ground behind selected
the ground: a click started digging (C07) instead of the vanilla miss and its ten-tick
`leftClickCounter`, and right-click placed against the block. The selection now follows the jar,
including the zero-distance replacement rule for an eye inside an entity box. Not modelled: the
direction-dependent end of `rayTraceBlocks`' last-uncollidable miss. Regression
`entities_beyond_three_blocks_turn_a_block_hit_into_a_miss` (physics) failed first. Gate: 171 scoped
tests (88 client, 2 login CLI, 1 assets, 6 auth, 18 physics, 38 protocol, 18 world), strict
Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`.

## 32. A second Join Game keeps the player

Section 31's selection fix is `18a6e89537`.

`handleJoinGame` creates a new `PlayerControllerMP` and a new, empty `WorldClient`, then
`Minecraft.loadWorld` creates an `EntityPlayerSP` only when none exists: on a second Join Game the same
player (inventory, selected slot, last reported position, sprint/sneak states) is kept and
`preparePlayerToSpawn` resets its size, motion and pitch and restores full health. Section 15 wrongly
replaced the inventory (selection back to 0) and kept the old world's chunks. Now the controller is
new (so a non-zero slot is synced with C09 on the next tick), the world is emptied and the player is
reset as `preparePlayerToSpawn` does. Proxies such as BungeeCord switch servers with Respawn packets,
so this path is rare. Regression `mid_session_join_keeps_the_player_but_starts_a_new_controller_and_world`
(replacing the section 15 test) failed first (old chunks kept). Gate: 171 scoped tests (88 client,
2 login CLI, 1 assets, 6 auth, 18 physics, 38 protocol, 18 world), strict Clippy/fmt, layers,
public-tree and diff privacy checks, ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 33. Downloading-terrain keepalives

Section 32's join fix is `3da7773e8c`.

`handleJoinGame`, and `handleRespawn` when the dimension changes, open `GuiDownloadTerrain`, which
`handlePlayerPosLook` closes on the first S08. While open, its `updateScreen` sends a C00 keepalive
with key 0 every twenty ticks. The client sent none. The actor now counts those ticks and sends them;
a same-dimension respawn opens no screen. The network test barrier skips these unsolicited replies.
Regression `downloading_terrain_sends_keepalive_zero_each_second` failed first (no keepalive).
Gate: 172 scoped tests (89 client, 2 login CLI, 1 assets, 6 auth, 18 physics, 38 protocol, 18 world),
strict Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`.

## 34. Held-item sync without a ticking player

Section 33's keepalive fix is `639e91d6be`.

`Minecraft.runTick` calls `PlayerControllerMP.updateController` (and so `syncCurrentPlayItem`)
before the open screen's `updateScreen`, whether or not the player itself ticks. The client synced C09
only inside a player tick, which waits for the first S08 and a loaded chunk, so a forced or reset
slot was not synced while downloading terrain. C09 sync now runs first in every client tick. The
respawn regression now expects that C09 before the S08 reply instead of relying on timing. Regression
`hotbar_sync_runs_before_the_first_position` failed first. Gate: 173 scoped tests (90 client, 2 login
CLI, 1 assets, 6 auth, 18 physics, 38 protocol, 18 world), strict Clippy/fmt, layers, public-tree and
diff privacy checks, ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 35. Dying entities

Section 34's held-item sync fix is `019ca90a2c`.

On the client, a living entity at zero health runs `onDeathUpdate`: it stays in the world (and
`canBeCollidedWith`, so the crosshair can still select it) for twenty ticks, then `setDead` removes it
even without S13. S19 status 3 also runs `EntityPlayer.onDeath` for other players, shrinking them to
0.2 by 0.2. The client skipped zero-health entities in selection, so a click passed through a dying
player to whatever stood behind, and never removed dying entities on its own. Entities now count
`death_time`, are removed at 20, dead players use the 0.2 box, and selection no longer filters on
health. The death tilt rendering is not ported. Regression
`dying_entities_linger_twenty_ticks_and_dead_players_shrink` (world) failed first. Gate: 174 scoped
tests (90 client, 2 login CLI, 1 assets, 6 auth, 18 physics, 38 protocol, 19 world), strict
Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`.

## 36. Block placement prediction

Section 35's dying-entity fix is `58516abaae`.

`PlayerControllerMP.onPlayerRightClick` checks `ItemBlock.canPlaceBlockOnSide` before sending
anything: if the block could not go there (not replaceable, or a `preventEntitySpawning` entity such
as the player itself inside the cube) it returns without the block C08, and `rightClickMouse` falls
back to `sendUseItem` (C08 at -1). Otherwise C08 is sent and `ItemBlock.onItemUse` places the block in
the client world at once and takes one from the stack (not in creative). The client sent every block
C08, including placements into its own body (a placement check anticheats use), and showed nothing
until the server's S23, so the player could fall through a bridge block the server already had.
For full-cube blocks whose placed state is known (wool, clay, glass, planks, stone and other plain or
damage-variant cubes), the client now refuses impossible sides as vanilla does, places the block
locally before the player's movement that tick and decrements the stack; the server's S23 and S2F
confirm or correct it. Other item blocks (slabs, stairs, oriented blocks, plants, torches) keep the
previous behaviour. Regression `block_placement_is_predicted_and_never_sent_into_the_player` failed
first (it sent the block C08 into the player). Gate: 175 scoped tests (91 client, 2 login CLI,
1 assets, 6 auth, 18 physics, 38 protocol, 19 world), strict Clippy/fmt, layers, public-tree and diff
privacy checks, ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 37. Block break prediction

Section 36's placement fix is `83f2a63ed9`.

`onPlayerDestroyBlock` sets the block to air in the client world as soon as the client considers it
broken: when a survival dig completes (after C07 STOP), at once when `clickBlock` finds a block that
breaks in one tick (only C07 START is sent; there is no STOP), and on every creative click
(`clickBlockCreative`, which first puts out fire on the clicked face and destroys nothing while a
sword is held, though it still sends START). The client only drew break particles and waited for the
server's S23, so the player could not step into a dug space until the server answered; instant
breaks sent a STOP one tick after START, and creative clicks with a sword sent no START. A block
broken this tick is air for the rest of the tick (a second click falls through to the miss case, as
`clickMouse` does), and `onPlayerDamageBlock` now swings on every held-attack tick, including the
five-tick `blockHitDelay`. Regression `broken_blocks_are_removed_locally_and_instant_breaks_send_no_stop`
failed first. Gate: 176 scoped tests (92 client, 2 login CLI, 1 assets, 6 auth, 18 physics,
38 protocol, 19 world), strict Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release
build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 38. Mods, liquid-glass menu and frame cap

Section 37's break-prediction fix is `5f19ddeedb`.

The owner lifted the earlier deferral of zoom, keystrokes and cosmetic polish and asked for a full PvP
mod set with an Apple-style glass interface. First milestone:

- **Glass renderer** (`glass.rs`, `glass.wgsl`): the finished frame is copied, blurred with a dual-Kawase
  chain (half, quarter, eighth resolution, then back up) and sampled by rounded-rect panels with
  edge refraction, vibrancy, tint, a specular rim and a soft shadow. Panels come in two layers: under
  the interface (HUD cards) and under an overlay (`Canvas::overlay`), which re-copies the frame so menu
  glass frosts the HUD too. Without a copyable drawable the panels fall back to tinted glass.
- **Settings** (`settings.rs`): JSON at `~/Library/Application Support/Horizon 1.8.9/settings.json`,
  written atomically; malformed values fall back to defaults. Smoke runs never read or write it.
- **Mod menu** (`menu.rs`, Right Shift): cards with iOS-style switches for every mod, a frame-limit
  card, Edit HUD (drag cards; Right Shift or Escape finishes) and Done. Escape closes it; it takes all
  keys while open. `--smoke-menu` opens it for captures.
- **HUD mods** (`hud_mods.rs`): keystrokes (WASD, mouse buttons with CPS, space), CPS, FPS, armour
  status (`item_slot` with durability), potion effects (vanilla icons, names, `getDurationString`,
  counted down client-side), coordinates with facing, and own tab-list ping. `--pvp-hud` now enables
  the FPS and CPS cards; the old text overlay is gone.
- **Gameplay mods**: smooth zoom on C (FOV x0.25, eased, look speed scaled), Toggle Sprint and Toggle
  Sneak latches, Fullbright (a gamma-10 lightmap baked into a full remesh), and a frame cap: Display
  (vsync), 60-360 or Unlimited (immediate presentation, no redraw wait).

Hypixel-allowed mods only (no reach display, radar or auto-anything). Still to do: Apple system font
(Core Text) for the menu, open/close animation, per-card settings (scale, colours), more mods
(crosshair, block outline, hit colour, nick hider, scoreboard), and measured performance work.
Regressions: settings round trip and atomic save, menu geometry and click/drag behaviour, glass vertex
expansion, potion duration and facing formatting. Captures checked the menu over the world and the
HUD cards. Gate: 182 scoped tests, strict Clippy/fmt, layers, public-tree and diff privacy checks,
ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 39. San Francisco text in the mod interface

Section 38's mod milestone is `f665bf3f4b`.

The menu and HUD cards now draw the macOS system font. `sysfont.rs` reads the installed variable font
`/System/Library/Fonts/SFNS.ttf` at run time (nothing is bundled or committed), sets regular (400) and
semibold (590) weights, and rasterises glyphs with `ab_glyph` (pure Rust; the client forbids `unsafe`,
so Core Text bindings were not used) at the drawable's physical pixel size. Glyphs are shelf-packed on
demand into eight new interface atlas layers after the unicode pages and drawn as pixel-snapped
quads with coverage alpha: the `0x20000000` layer flag exempts them from the GUI alpha test, so edges
stay antialiased. `Canvas::system_text`/`system_width` take a line-box height in GUI pixels; without
the font file they fall back to the Minecraft font. Vanilla HUD, chat and screens keep the Minecraft
font. Regressions: glyph rasterising/caching/metrics and the atlas packer. A capture checked the
menu and cards. Gate: 184 scoped tests, strict Clippy/fmt, layers, public-tree and diff privacy checks,
ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 40. Measured terrain and meshing performance

Section 39's system-font milestone is `228d5bf6d5`.

**Benchmark.** `--bench` (with `--smoke-frames`) renders uncapped to an offscreen target, waits for
each frame's GPU work and prints `BENCH_CPU_US`, `BENCH_GPU_US`, `BENCH_FRAME_US` (p50/p95/p99) and
`BENCH_FPS` over the last 1,024 frames. Offscreen matters: with a visible but covered window, macOS
paced frames to 16/33 ms and the first reading (40 FPS) was the compositor, not the renderer. Setup
used for the numbers below: Apple M5, 1920x1200, render distance 12, a private local vanilla 1.8.9
server on default terrain (seed 8675309, view distance 12, spawn facing forest and a lake), 6,000
frames per run, three alternating runs per build. Baseline = `228d5bf6d5` with only the bench patch.

| | Baseline | This change |
| --- | --- | --- |
| GPU per frame (p50) | 1.86 ms | 1.05 ms |
| CPU per frame (p50) | 0.20 ms | 0.24 ms |
| Frame, serialized (p50) | 2.08 ms | 1.30 ms |
| FPS (serialized mean) | 465 | 748 |
| Mesh one chunk batch (~31 columns) | 24.7 ms | 6.7 ms |

Breakdown before the change: terrain was ~1.4 ms of the 1.86 ms (no-terrain frame 0.46 ms, empty
frame 0.20 ms), with 2.6M visible terrain vertices and 8.7M resident (350 MB at 40 bytes each).

- **Packed terrain vertices** (`terrain.rs` `TerrainVertex`, 16 bytes): column-local position in
  1/128 blocks biased by 8 plus the sprite layer (`Uint16x4`), UV (`Unorm16x2`), and vanilla's 8-bit
  gamma vertex colour (`Unorm8x4`). Quads are four corners through one shared `quad_indices` buffer
  instead of six expanded vertices, so a quad is 64 bytes instead of 240. Each draw takes its
  column's offset from the mesh origin as per-instance data written once per frame.
- **Solid/cutout split**: sprites with any texel under the 0.1 alpha test are cutout; everything else
  draws first with a fragment shader that has no `discard`, so Apple's hidden-surface removal works.
  The terrain shader decodes the vertex colour once per vertex; without the hurt overlay vanilla's
  gamma-space product is exactly texel x decoded colour, so captures are unchanged.
- **Per-section frustum culling**: meshes are ordered by 16-block section with quad offsets, so each
  column draws only its visible sections, adjacent ones merged into one indexed draw.
- **Occlusion culling** (`VisGraph`): the mesher computes each section's face connectivity (fewer than
  256 opaque cubes sees everything, fully opaque sees nothing, otherwise flood-filled regions join
  the faces they touch). `scene::reachable` walks breadth-first from the camera's section as
  `RenderGlobal.setupTerrain` does: never back toward the camera, and out of a section only through a
  face joined to the one it entered by. The walk ignores the frustum so its result is cached until
  the camera changes section or a column's connectivity changes; above, below or outside the loaded
  world nothing is occluded.
- **Parallel meshing**: a coordinator hands the nearest dirty columns to `cores - 3` workers (7 on
  the M5). A column is never meshed twice at once, and meshes built for an old origin are rebuilt
  instead of shown, so out-of-order results cannot replace newer geometry. `MESHER_IDLE` reports when
  a burst is done.

Not yet measured: crowded lobbies (entity and nametag vertices are rebuilt on the CPU every frame),
and a side-by-side with Lunar or OptiFine on the same machine and world, which needs the owner's
launcher. On-screen FPS with Frame Limit at Unlimited also depends on how fast macOS hands back
drawables. Regressions: packed-vertex precision and ranges, quad indices, section ranges and run
merging, alpha classification (owned jar: stone solid, oak leaves cutout, grass mixed; every baked UV
and position fits the packed format), `VisGraph` connectivity, the occlusion walk, mesher dispatch
ordering and stale-origin handling, and `--bench` option validation. Captures before and after
matched. Gate: 195 scoped tests, strict Clippy/fmt, layers, public-tree and diff privacy checks,
ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 41. Menu motion, card sizes, scoreboard and crosshair mods

Section 40's performance work is `993e2583f1`.

- **Motion**: the menu sheet eases in over 0.22 s (ease-out cubic), rising 12 GUI pixels into place
  as it fades, and plays the same in reverse on close; it stays drawn while closing but takes no
  input. Switches slide their knob and blend the track from grey to green over 0.16 s. Glass panels
  gained an `opacity` (the shader's spare `params.w`), applied to the shadow and body alike.
- **Card sizes**: in Edit HUD the scroll wheel resizes the card under the mouse in 0.1 steps from 0.5x
  to 3x (`Hud.scale`, saved). Cards draw under `Canvas::scale` from their saved corner and are kept
  on screen at their scaled size. System-font text now rasterises at the scaled pixel size, so a
  scaled card's text stays sharp.
- **Scoreboard Numbers** (on by default): off hides the sidebar's red score column and narrows the
  sidebar to the names, as Lunar's scoreboard mod does.
- **Crosshair**: Vanilla (the inverted sprite), Cross (a gapped plus of four 4-pixel arms) or Dot (2x2
  pixels). Both custom styles use the vanilla crosshair's inverting blend
  (`Canvas::inverted_rect`) and are centred on its pixel.

Regressions: the open/close and switch animations (including staying visible while closing), scroll
resizing with clamping, card placement at scale, crosshair centring, the new cards' clicks, and the
new settings' round trip and bad-value fallback. A capture checked the menu with the new cards.
Gate: 200 scoped tests, strict Clippy/fmt, layers, public-tree and diff privacy checks, ARM64
release build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 42. Entity frustum culling for crowded lobbies

Section 41's menu milestone is `0b40409071`.

A crowd benchmark (private bench server, 150 or 441 villagers with visible custom names summoned
around the camera by console `execute ... summon`) showed entity and name-tag vertices rebuilt and
uploaded for every loaded entity each frame, wherever it was: 150 named villagers were 43,000 entity
and 23,500 label vertices (about 2.7 MB a frame), and a CPU sample put `queue.write_buffer` copies
from `set_entities` at roughly 40% of non-waiting main-thread time.

`scene::entity_in_view` now does `RenderManager.shouldRender`'s frustum check: a generous box (1.5
blocks out, from half a block below to 3.5 above the interpolated feet) against the view planes.
Entities that fail are neither meshed nor labelled (labels render with their entity, as vanilla).
Mobs larger than the box (giant, slime, ghast, magma cube, dragon, wither) and fishing bobbers, whose
line reaches the angler, always draw. Surrounded by 441 named villagers, two alternating runs each
(M5, 1920x1200, render distance 12):

| | Before (`0b40409071`) | After |
| --- | --- | --- |
| CPU per frame (p50) | 1.61 ms | 0.76 ms |
| GPU per frame (p50) | 1.78 ms | 1.44 ms |
| FPS (serialized mean) | ~286 | ~420 |

A crowd entirely in view is unchanged, as expected. Still possible later: a compact indexed entity
vertex format, or static per-model meshes with per-entity bone transforms, to cut the upload for
crowds that are in view. Regressions: frustum acceptance in front, rejection behind and beside,
the box's margin at the edge, always-drawn large mobs and bobbers, and partial-tick interpolation.
A capture inside the crowd showed no missing entities at the screen edges. Gate: 201 scoped tests,
strict Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`.

## 43. Block selection outline

Section 42's entity culling is `24b17db3b8`.

The client drew no selection box. It now does `RenderGlobal.drawSelectionBoundingBox`: the targeted
block's selection box grown by 0.002, black at 0.4 alpha, two points wide, depth-tested, after the
world (with the depth-tested labels, in gamma space). Metal has no wide lines, so each of the twelve
edges is a camera-facing quad whose width stays constant on screen (`outline.rs`); edges are pulled
0.3% toward the eye so the half of a quad that dips behind a face at grazing angles still passes.

- **Targeting**: the window thread casts `EntityRenderer.getMouseOver`'s block ray every frame from
  the interpolated eye (`try_read` on the world, so it never waits on the network thread), so the
  box follows the mouse at frame rate. Entities are not re-traced per frame: the tick's own ray
  reports when an entity is aimed at (`Shared.aiming_entity`), and then no box is drawn, as vanilla.
- **When**: `isDrawBlockOutline` only for players who may edit, so survival and creative; adventure
  (Hypixel lobbies) and spectator draw none.
- **Shape**: `raycast::selection_box` is the union of the boxes the look ray can hit (collision
  boxes, or the inset plant and sign bounds), with fences, walls and gates one block tall as their
  selection bounds are. The ray now shares that box list (`target_boxes`).
- **Block Outline card**: Vanilla, Thick (4 points, 0.6 alpha), White (0.8 alpha) or Off.
- `--smoke-pitch DEGREES` (validation, with `--smoke-frames`) tilts the view once in the world, so a
  capture can show the box.

Regressions: selection boxes (air, full cube, bottom slab, fence post height, inset tall grass,
water, unloaded), outline geometry (twelve edges, constant screen width, near the grown box), the
outline card cycle, settings round trip, and the new option's validation. A capture on the bench
world, looking 40 degrees down, showed the box on the targeted grass block. Gate: 203 scoped tests,
strict Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`.

## 44. Horizon 1.8.9.app: launcher and self-update

Section 43's outline is `ce54a12409`.

The owner asked for a dedicated, self-updating app to test with, in Rust, with updates built on his
own Mac rather than published. The client is now also the app:

- **Install** (`--install [--update-source URL]`, `updater::install`): writes `Horizon 1.8.9.app` to
  `/Applications` (or `~/Applications` if that is not writable) with an `Info.plist`, the Horizon
  icon and this program as `Contents/MacOS/horizon-189`, and records the update source (a plain
  https git URL, branch `main`) in `~/Library/Application Support/Horizon 1.8.9/update.json`. The URL
  lives only there, never in the repository.
- **Launcher** (`launcher.rs`): with no mode flags the app opens to a liquid-glass card over the
  vanilla title panorama (its four side faces drift across the screen; they load as interface layers
  9 to 12). It shows the Microsoft account (the saved sign-in is refreshed on a background thread;
  Sign In runs the device-code flow there, opens the browser and shows the code with Copy Code), the
  server list from `settings.json` (Hypixel, Minemen Club and This Mac by default; add by typing an
  address, × removes the selected one, double-click or Enter joins), and Play and Quit. Without a
  sign-in only servers on this Mac can be joined, as an offline "Player". A missing 1.8.9 jar or a
  fatal error shows a native alert, since the Finder-opened app has no terminal.
- **Leaving a server**: the disconnect screen's second button becomes "Back to server list"
  (`gui.toMenu`), and the mod menu gains Disconnect, for sessions started from the launcher.
- **Self-update** (`updater.rs`): only when running from an app bundle. At launch a background thread
  keeps a shallow clone in Application Support (`source/`), fetches the branch, and when its tip
  differs from the commit the program was built from (`HORIZON_189_BUILD`, set by the updater's own
  build) checks it out and runs the user's `cargo build --release --locked` into `build/`, showing
  progress on the launcher. The new program is copied beside the old one and renamed over it, so the
  swap is atomic and no old copy is left; the launcher then offers Restart (`open -n` the bundle).
  Failures go to `update.log` and show on the card. Development and validation runs never update.
- `--smoke-launcher` (validation) shows the launcher signed out, without the saved sign-in or updates.

Regressions: bundle detection, update-source validation (https only, no option-like or
shell-significant characters), rebuild decisions and cargo progress, the one-file program swap, the
plist, address parsing and the local-only offline rule, launcher geometry, adding a typed server,
server-list persistence and validation, the Disconnect button, and the new options. A
`--smoke-launcher` capture checked the card over the panorama. Gate: 211 scoped tests, strict
Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`.

## 45. On-screen frame rate, frame cost and latency

Section 44's app and launcher is `898127bfab`.

The owner's first session on a real server (Minemen) found no noticeable performance difference.
Measured on his Mac (MacBook Air M5, a 60 Hz built-in display): with Frame Limit at Unlimited the
present mode is Immediate, yet frames still reach the screen every 16.7 ms, because macOS returns a
windowed layer's drawable only as the compositor consumes one. A windowed app on this display shows
at most 60 frames a second whatever it renders; Java clients' counters count frames rendered and
discarded. The renderer itself needs about 1.3 ms of a 16.7 ms refresh.

- The swap chain now queues one frame instead of two (`desired_maximum_frame_latency = 1`), so the
  frame shown was started from newer input. Pacing on the bench world stayed steady (2,400 frames,
  interval p50 16.67 ms, p95 17.4 ms, none over twice the median).
- The FPS card shows the frames actually shown and the smoothed CPU cost of each frame without the
  display wait, e.g. "60 FPS · 1.2 ms" (`pvp_metrics::FrameTime`).
- View bobbing was checked rather than changed: on a scripted walk the camera sway reaches vanilla's
  ~0.1 block, so the report was read as asking for a View Bobbing setting (next section).

Next for latency: start each frame just before the display's refresh instead of just after the
previous one, so input is sampled as late as possible. Regressions: frame-time smoothing and the
FPS card text. A capture showed the card. Gate: 213 scoped tests, strict Clippy/fmt, layers,
public-tree and diff privacy checks, ARM64 release build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 46. Held items at vanilla size

Section 45's frame-rate change is `77084669cb`.

The owner saw his sword sit away from the hand on Minemen. `RenderItem.renderItemModelTransform`
(`bjh.a(zx, boq, bgr$b)`) calls `preTransform` (`bjh.b(zx)`), which scales by 2 every model that is
not 3D, before the model's display transform; `ItemRenderer.renderItem` doubles the 3D (block)
models instead, so every held item is doubled exactly once. The block paths had the doubling; the
generated (flat) item paths, first and third person, did not, so swords, tools, rods, bows and food
were drawn at half size with their display translation halved: smaller, lower and further left
than vanilla. Worked by hand through `transformFirstPersonItem` and the sword's firstperson display
(rotation [0, -135, 25], translation [0, 4, 2], scale 1.7), the fixed sword's tip lands just past
the right edge a third above centre and its hilt below the bottom edge, as a capture now shows.
Third-person held items (other players) use the same rule. Regression: the first-person generated
transform puts the model centre at twice the display translation and spans 1.7 units. Gate: 214
scoped tests, strict Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build
and `VANILLA_189_REFERENCE_GATE_PASS`.

## 47. Game menu and settings

Section 46's held-item fix is `1cb2e9d387`.

The owner found no Minecraft settings. Escape in a world (with no other screen open) now opens a
liquid-glass game menu (`pause.rs`, after `GuiIngameMenu`): Back to Game, Options, Mods (the mod
menu) and Disconnect for sessions started from the launcher (Quit Game otherwise); Escape steps back
a page. Options holds vanilla's FOV (30 to 110, "Normal" at 70, "Quake Pro" at 110), Sensitivity
(0 to 200%, "*yawn*" and "HYPERSPEED!!!" at the ends) and Render Distance (2 to 16 chunks) as
draggable sliders, GUI Scale (Auto, Small, Normal, Large, Huge), and View Bobbing and Fullscreen
switches, labelled from the game's own language file. Changes apply as they happen: FOV, sensitivity
and bobbing at once; render distance moves the fog and is re-sent to the server (C15) on the next
tick; GUI scale relays out the interface; fullscreen is borderless. They are saved in
`settings.json` (`video`, clamped to vanilla's ranges when read) and the app starts with them;
command-line and validation runs keep their flags. `--smoke-settings` opens the page for captures.

Regressions: Escape and button routing, slider dragging and ends, switches and the GUI scale cycle,
the vanilla slider labels, sheet geometry on small and large windows, and the video settings' round
trip and clamping. A capture checked the Options page over the world. Gate: 217 scoped tests,
strict Clippy/fmt, layers, public-tree and diff privacy checks, ARM64 release build and
`VANILLA_189_REFERENCE_GATE_PASS`.

## 48. The player in the inventory

Section 47's game menu is `e057035a76`.

The owner found no player in the inventory. `GuiInventory` draws the local player with
`drawEntityOnScreen` at `guiLeft + 51`, `guiTop + 75`, thirty interface pixels to a block,
`translate(x, y, 50)`, `scale(-30, 30, 30)`, `rotate(180, Z)`, pitched toward the mouse, with body yaw
`atan(dx / 40) * 20`, yaw and head yaw `atan(dx / 40) * 40` and pitch `-atan(dy / 40) * 20` (dx, dy
from the model's point and 50 pixels above it to the mouse). The client now builds the local player
as an entity (its skin, armour and held item) through the normal entity models
(`main::own_model`, `Renderer::set_inventory_model`), maps it with `models::inventory_model_matrix`
into clip space (interface pixels, y down, depth toward the viewer for reversed-Z), and draws it in
its own depth-cleared pass between the inventory's background and its items: `hud::inventory` starts
the overlay after the background, so slots, items and tooltips draw over the model. Regression: the
model's feet land on the box point, a player's head 54 pixels higher, +x to the right and the side
facing the viewer nearer. A capture showed the model holding a sword and turned toward the mouse.
Gate: 218 scoped tests, strict Clippy/fmt, layers, public-tree and diff privacy checks, ARM64
release build and `VANILLA_189_REFERENCE_GATE_PASS`.

## 49. Sprint, Speed, flight and bow FOV

Section 48's inventory model is `879856e9f1`.

The world FOV never changed with movement. Ported from the jar: `AbstractClientPlayer.getFovModifier`
(`bet.o`) starts at 1, times 1.1 when flying, times (movement speed / walk speed + 1) / 2, falling
back to 1 for a zero walk speed or a non-finite result, then for a bow in use times 1 - 0.15 x
min(1, (ticks / 20)^2). `EntityRenderer.updateFovModifierHand` (`bfk.l`) eases the hand modifier
halfway toward it each tick, clamped to 0.1 to 1.5, and `getFOVModifier` (`bfk.a(float, boolean)`)
multiplies the FOV setting by it at the partial tick, divides while dying by
(1 - 500 / (deathTime + partial + 500)) x 2 + 1, and scales by 60/70 when the eye is under water
(`getBlockAtEntityViewpoint`, including the liquid-surface check). The hand keeps its fixed 70.
`hand::fov_modifier`, `Tracked.fov` (starting at 1), `hand::world_fov` and `hand::eye_in_water`; the
walk speed comes from S39 and the speed attribute already carries sprint and potion modifiers. The
zoom mod multiplies after. Regressions: sprint 1.15, flight 1.1, bow draw 0.85 and 0.9625, zero walk
speed, the halfway easing and interpolation, under-water scaling and the surface check.

Gate: 220 tests.

## 50. F1 hides the GUI

Section 49's dynamic FOV is `5ea3e224af`.

F1 did nothing. From `Minecraft.runTick` (`ave.s`): with no screen open, key 59 toggles
`GameSettings.hideGUI` (`avh.aA`); an open screen takes the key instead. While hidden,
`updateCameraAndRender` skips the in-game overlay unless a screen is open (so chat, the inventory,
the death screen, the pause menu and the mod menu still draw it), and `renderHand`,
`isDrawBlockOutline` and `Minecraft.isGuiEnabled` skip the first-person hand, the block outline and
name tags. The HUD mods are part of the overlay. `view::View` holds the toggle (F3 and F5 come
next, from the same branch: `avh.aC` and `avh.aB`); `Renderer.names` gates name tags.
`--smoke-hide-gui` starts hidden for captures: on the bench server with a sword and three named
villagers the normal frame shows the HUD, mods, sword, crosshair and name tags, the F1 frame only
the world. Regression: the toggle only without a screen, the overlay returning with one, and the
world GUI staying off.

Gate: 221 tests.

## 51. F2 screenshots

Section 50's F1 is `a17d749e30`.

F2 did nothing. From `Minecraft.dispatchKeypresses` (`ave.Z`, which runs over any screen) and
`ScreenShotHelper` (`avj`): the frame goes to `screenshots/` as RGB PNG named
`yyyy-MM-dd_HH.mm.ss.png` in local time, then `_2`, `_3`… while the name is taken, and chat shows
`Saved screenshot as` with the file name underlined (or `Couldn't save screenshot: <reason>`). The
folder is `screenshots/` under the app's Application Support folder. The readback stays on the frame
(`Renderer::read_frame`, shared with `--capture`); encoding and writing run on a worker thread so
a screenshot never stalls a fight, and the result reaches chat through a channel. Local time comes
from `date +%z` since std has no time zones (UTC if that fails). Clicking the name to open the file
is not done yet. `--smoke-screenshot DIR` takes one near the end of a smoke run: the bench capture
showed the chat line and the folder held a 1920x1200 RGB PNG with the right local time.
Regressions: the stamp (epoch, a leap day, a Pacific offset), the free-name suffixes, the message,
and an opaque RGB write.

Gate: 223 tests.

## 52. F5 third person

Section 51's F2 is `a5a4832870`.

F5 did nothing. From the same `ave.s` branch: with no screen open the perspective key steps
`thirdPersonView` (`avh.aB`) 0, 1, 2, 0. `EntityRenderer.orientCamera` (`bfk.f`) backs the camera 4
blocks from the eye along the look, or for 2 along the look turned round (the camera then faces the
player: yaw + 180, pitch negated). Eight rays from the eye, offset 0.1 on each axis, pull it in to
the nearest block (`rayTraceBlocks`, air and liquids pass); each ray's end adds the z offset to x
too, as vanilla's does. `renderHand` draws the first-person hand only in mode 0; the crosshair
stays (`GuiIngame` reads the mode only for the pumpkin blur, which this client doesn't draw).
Block picking, the outline target and reach still use the player's eye; the frustum, name-tag
facing, outline width and the under-water FOV use the camera.

The local player's model: `Entity::tick_local` (world crate) steps a player entity to the player's
own position and look each client tick, through the same body-yaw and limb-swing update other
players get, with sneak, sprint and use flags; the network thread keeps it in `Shared.own_body` and
drops it on respawn. Each frame `own_body` adds the skin, armour, held item, swing, hurt flash,
death tilt and invisibility, and the head follows the live look. The renderer draws it with the
other entities (and from its hand for the fishing line) but never gives it a name tag
(`canRenderName` skips the view entity). Spectators get the camera but no model.
`Raycaster::trace_blocks` (physics crate) is the segment trace. `--smoke-perspective N` starts in a
perspective: on the flat reference server mode 1 shows Steve from behind four blocks back with the
crosshair on his head; on the bench server mode 2 shows him facing the camera with the sword in his
right hand. Not matched yet: vanilla turns name tags for mode 2 by yaw only, here they face the
camera. Regressions: the F5 cycle and hidden hand, open-air camera distance and front-view look,
the pull-in at a wall, the segment trace (stops at stone, passes water) and the local model's
position, head and body turn and limb swing.

Gate: 227 tests.

## 53. F3 debug screen

Section 52's F5 is `a5ac06fb0b`.

F3 did nothing. From the same `ave.s` branch, key 61 with no screen open toggles `showDebugInfo`
(`avh.aC`) on press, and `GuiOverlayDebug` (`avv`) draws it. Each non-empty line goes on a
`0x90505050` backing in `0xE0E0E0` without shadow, nine pixels apart from y 2, the left column at
x 2 and the right one right-aligned 2 in. The left column (`avv.b`), with the format strings from
`ave`, `bfr`, `bho` and `bcz`:
- `Minecraft 1.8.9 (1.8.9/vanilla)`, the brand this client sends.
- `N fps (N chunk updates) T: <limit|inf>[ vsync] vbo`.
- `C: drawn/(2D+1)^2*16 (s) D: N, pC/pU/aB`. Here `C` counts sections drawn with geometry, `pC`
  is columns queued or meshing, `pU` is meshes waiting to upload, and `aB` is idle meshers.
- `E: drawn/loaded, B: 0, I: rest` and `P: particles. T: All: loaded`.
- `MultiplayerChunkCache: columns, columns`.
- XYZ (feet, `%.3f / %.5f / %.3f`), Block, Chunk, and Facing (`getHorizontalFacing` with
  wrapped yaw and pitch).
- Biome (names from `ady`/`aeo`/`aeg`/`aeu`, mutated ones with " M"), and Light
  (`getLightSubtracted`, then sky and block).
- Local Difficulty: `DifficultyInstance` (`ok.a`) with no inhabited time, as a client's chunks have
  none, and the moon factors.
- Looking at, when the crosshair is on a block.

Reduced debug (Join Game, entity status 22/23) shows only the header and Chunk-relative, as
vanilla does. Section rebuilds per second count meshed columns times 16, since this client meshes
whole columns where vanilla rebuilds 16-high sections. The right column puts this client's runtime
in place of the JVM's: `Rust: Horizon <build> 64bit`, the CPU (`sysctl
machdep.cpu.brand_string`, read once), `Display: WxH (Apple)`, the GPU and `Metal`, then
`minecraft:<block>` when looking at a block. Block properties aren't listed yet. The HUD mods hide
while F3 is up, since they would sit under the left column.

Network: S03 sets the time of day (negative turns the daylight cycle off, as
`WorldClient.setWorldTime` does) and each tick advances it while the cycle runs. S41, Join and
Respawn set the difficulty. The mesher publishes its queue through `MeshStats`.
`--smoke-debug` opens it for captures; the bench capture shows every line (the bench player stands
in a cave at y 35, which is why bench shots are dark).

Regressions: the full left column against hand-checked values, the reduced column, the right
column's block name, local difficulty (peaceful, day 0, hard at full moon after 60 days, easy's
halved moon), biome names, the F3 toggle, and S03/S41/status 22 reaching `Shared` with the cycle
ticking.

Gate: 230 tests.

## 54. F3+B hitboxes, F3+D and F3+A

Section 53's F3 is `307db85a5d`.

These are the F3 combinations from the `ave.s` branch: a key pressed while F3 is held, with no
screen open.
- **F3+B** toggles `RenderManager.debugBoundingBox`. `renderDebugBoundingBox` draws, for every
  visible entity that isn't invisible:
  - its box in white;
  - for living entities, a red box 0.02 tall at eye height;
  - its look in blue, two blocks out from the eye. Living entities look along their interpolated
    head yaw and pitch, others along their yaw (`getVectorForRotation`, the MathHelper table).
  - The lines are depth tested without writing depth, at the 2-pixel width the block outline
    leaves set. Reduced debug info hides them, and in F5 your own box draws too.
  - Eye height is exact for players (1.62, 1.54 sneaking). Other mobs use the default 0.85 of
    their height, so mobs with their own `getEyeHeight` (zombies, endermen, spiders...) are close
    but not exact yet.
- **F3+D** is `GuiNewChat.clearChatMessages`, which clears the lines and the sent history.
- **F3+A** is `loadRenderers`, which remeshes everything.

As in 1.8.9, F3 itself toggles the overlay on press, and the combination's own key still acts
(A still strafes).

`outline::segment` and `edges` are shared by the block outline and `outline::hitbox`.
`--smoke-hitboxes` starts with them on: in F5 on the flat server, the player has the white
0.6x1.8 box and the red band at the eyes.

Regressions: the hitbox geometry (12 white edges, 12 red at 1.62, one blue line two blocks out),
the combos and that a screen takes them, and chat clearing its history.

Gate: 231 tests.

## 55. Exact eye heights, marker armour stands and baby pigmen

Section 54's hitboxes are `6a6719ec42`.

I mapped every mob's `getEyeHeight` override from the jar, using `EntityList` (`pm`) for the class
per network type and `aS()F` for the overrides. F3+B's red box now uses them:

| Entity | Eye height |
| --- | --- |
| Skeleton | 1.74 (wither skeletons keep the default) |
| Spider | 0.65 |
| Giant | 10.440001 |
| Zombie and zombie pigman | 1.74, children 1.74 - 0.81 in double |
| Slime and magma cube | 0.625 x height |
| Ghast | 2.6 |
| Enderman | 2.55 |
| Cave spider | 0.45 |
| Silverfish and endermite | 0.1 |
| Bat | height / 2 |
| Witch | 1.62 |
| Guardian and squid | height / 2 |
| Sheep | 0.95 x height |
| Cow, mooshroom, chicken and horse | height |
| Wolf | 0.8 x height |
| Snow golem | 1.7 |
| Villager | 1.62, children 0.81 |
| Armour stand | 0.9 x height, small 0.5 x height |
| Anything else | `Entity`'s 0.85 x height |

Two world-crate fixes came out of reading the same classes:
- `EntityArmorStand.updateBoundingBox` shrinks marker stands to 0 x 0, while small stands keep the
  full 0.5 x 1.975 box. Entity picking uses these sizes, so marker stands (common for holograms)
  had full-size boxes that could swallow clicks meant for a player behind them. Vanilla only hits a
  0.2 cube at the feet (the 0.1 collision border).
- Zombie pigmen inherit `EntityZombie`'s child flag (watcher 12), which only zombies read before.
  Baby pigmen now get the halved size.

Regressions: eye heights for zombie, baby zombie, enderman, spider, wither skeleton, creeper,
baby villager, cow and armour stand; marker and small armour-stand sizes; the baby pigman.

Gate: 232 tests.

## 56. F3+H advanced tooltips (and why F3+F does nothing)

Section 55's eye heights are `728a6610cd`.

**F3+H** toggles `advancedItemTooltips` and saves the options, as vanilla does. It's
`toggles.advanced_tooltips` in settings.json. `ItemStack.getTooltip` (`zx`) then:
- appends ` (#%04d/%d)` to the name for items with subtypes, or ` (#%04d)` for the rest;
- shows dyed leather as `Color: #<toHexString, upper case>` instead of "Dyed";
- ends with `Durability: left / max` when damaged, the registry name in dark grey, and
  `NBT: N tag(s)` (top-level keys) when the stack has a tag.

Without F3+H, a filled map's name now ends in ` #<map number>`, which was missing.

`items::REGISTRY` holds all 337 items' ids, `getHasSubtypes` and registry names. It was booted from
the owned jar with `Bootstrap.register` by a private dumper (not committed, like the earlier tables).

**F3+F** passes `RENDER_DISTANCE` to `GameSettings.setOptionValue(Options, int)`, which has no case
for it. In 1.8.9 the combination only re-saves the options, so it stays a no-op here.

Regressions:
- the sword's `(#0276)`, durability 1500 / 1561, registry name and `NBT: 1 tag(s)`;
- wool's `(#0035/14)` with no durability line;
- the map's ` #7` and `(#0358/7)`;
- leather's `Color: #A06540`;
- the H and F combos.

Gate: 233 tests.

## 57. Just-in-time vsync pacing

Section 56's F3+H is `a654be72b0`.

The vsync redraw timer fired every 96% of a refresh, free-running against the display. The window's
CAMetalLayer holds two drawables, one on screen and one being drawn. A frame presented before a
vsync is shown at it, and only then does the drawable it replaces come free. So the timer either
landed at a random phase, showing input anywhere from about 2 to 18 ms old, or drifted into
blocking on the drawable after sampling input, showing it about two refreshes old.

`pacing::Pacer` starts each frame a budget before the vsync after the one its predecessor is
shown at, so the drawable is already free and the frame shows about a budget old, every frame.
- **Phase.** Nothing reports vsync times to safe Rust, so the pacer learns them: an acquire that
  blocks ended at a vsync. While pacing works nothing blocks, so every 120 frames a sync frame
  starts at once to block and re-measure.
- **Misses.** A frame that missed its vsync makes the next acquire block. That both re-measures
  the phase and widens the budget by 1 ms, up to 3/4 of a refresh.
- **Budget.** It narrows by 0.25 ms per 240 clean frames, down to the slowest recent frame cost
  plus 1 ms, and never below 2.5 ms.
- **Wake lateness.** The timer plus a run-loop spin starts frames about 1–2 ms after the asked
  time. The pacer learns this as a decaying maximum and asks that much earlier.
- **Uploads.** In `about_to_wait`, mesh uploads only use the idle time before the next frame
  starts, so they can't push a frame past its vsync.

This applies with vsync, the default `fps_limit` 0. The other limits keep their timers, and
`--no-jit-pacing` restores the old vsync timer for A/B runs. Smoke runs print
`FRAME_INPUT_AGE_US`: per frame, the first vsync after present (on the learned phase) minus the
time input was sampled. They also print `PACING jit misses budget_us`.

On the bench world at 60 Hz, three interleaved pairs of 900 frames:

| | Input age p50 | p95 | p99 | Frame interval p95 | p99 |
| --- | --- | --- | --- | --- | --- |
| Just in time | 5.0–5.6 ms | 6.0–6.3 ms | 6.7 ms | 17.8–17.9 ms | 18.8–19.9 ms |
| Old timer | 17.4–26.2 ms | 18.0–32.1 ms | 22.7–32.8 ms | 17.7–17.8 ms | 18.0–18.4 ms |

Neither mode had an interval over twice the median. Each just-in-time run had 2 misses, while it
learned at startup. A first, wrong version aimed frames at the vsync their predecessor still
waited for, and blocked about 12 ms every frame; the per-frame trace that showed this is not
committed.

Regressions: the vsync grid, the sync frame locking the phase and the next frame starting a budget
before vsync + 2P and showing a budget old, misses widening the budget and clean frames narrowing
it to the floor, sync frames every 120, wake-lateness compensation, and a heavy frame raising the
floor.

Gate: 236 tests.

## 58. Blur only around the glass, skip empty passes

Section 57's pacing is `7704c85944`.

Hiding the GUI (F1) doubled bench FPS, and the glass alone cost GPU 0.57 → 0.37 ms and CPU
0.19 → 0.11 ms (measured with a temporary switch, not committed). Every frame it copied the whole
frame and ran a five-pass blur chain over it, for a few small HUD cards.

`glass::region` now gives each layer the physical rectangle its blur needs:
- its panels plus their shadow and rim refraction, padded by 64 px (the chain reaches about 40);
- snapped outward to a 64 px grid, so the half, quarter and eighth texels line up with a
  whole-frame blur;
- stopping exactly at the frame's edge where it reaches it, so edges clamp as before.

Each layer keeps textures sized to its region. The HUD region is small and its 64 px grid rarely
changes size; the menu region only exists while a menu is open. The frame is copied from the
region's origin, and panels sample through its offset (`Params.region`). The blur views are
created once instead of every pass. The hand pass is skipped when there's no first-person hand
(F1, F5, spectator), and the interface overlay pass when it's empty: each pass loads and stores
the whole frame on Apple's tile GPUs.

Equality: the previous commit and this one, built side by side, captured the same flat-world
frame with the HUD and with the settings menu. Every pixel more than 2/255 apart is inside the FPS
card's digits, which change between runs, and the slight shift beside it follows that card's
width. The menu frame differs only in 10 pixels by at most 2/255.

Bench terrain, HUD on, alternating builds with cool-downs (a fanless Air throttles):

| | FPS | GPU p50 | CPU p50 |
| --- | --- | --- | --- |
| Before | 1,170–1,187 | 0.558–0.573 ms | 0.189–0.193 ms |
| After | 1,375–1,379 | 0.474–0.476 ms | 0.174 ms |

One "after" run (1,030 FPS, CPU p50 0.232 ms) was disturbed by other load on the machine.

Regressions: region padding, the grid, the frame edge, two panels sharing one region, and no
panels (or only degenerate ones) meaning no blur.

Gate: 237 tests.

## 59. Stage entity and label vertices through a staging belt

Section 58's glass region is `312948ea3e`.

The bench player had been saved in a cave at y 35, and villagers summoned around it suffocated.
That's why earlier crowd numbers swung between runs. With the player back at the world spawn (a
private change to the bench server, backup kept), 441 villagers came to about 265 FPS: CPU 1.62 ms,
GPU 1.66 ms median and 4.7 ms p95.

A frame of that crowd uploads 136,656 entity and 81,294 label vertices (40 bytes each), about
8.7 MB. The profile put a third of the non-waiting main thread in `queue.write_buffer`. With both
the entity and label draws skipped, but the uploads kept, the GPU still took 1.34 ms against
terrain's 0.47 ms (temporary switches, not committed). `write_buffer` makes a fresh staging buffer
per call, and a fresh buffer's megabytes of pages are faulted in, zero-filled and made resident
every frame. That costs far more than the copy.

`Scene` now stages both through a `wgpu::util::StagingBelt` (8 MiB chunks, reused) into an upload
encoder. `Renderer::render` submits it ahead of the frame, recalls the belt and polls the device
without waiting each frame, so chunks come back. An upload left behind by a frame that didn't render
(occluded) is sent at the next `set_entities`. In a 40 s vsync crowd run the resident size stayed
at 260 MB.

Bench crowd of 441, alternating with the previous build:

| | FPS | CPU p50 | GPU p50 | GPU p95 |
| --- | --- | --- | --- | --- |
| Before | 259–261 | 1.64 ms | 1.73 ms | 5.2 ms |
| After | 373–399 | 1.02–1.04 ms | 1.36 ms | 1.7–3.0 ms |

The FPS card's on-screen frame cost in that scene went from 3.1 to 2.4 ms. Captures of the crowd
look the same; pixel diffs aren't meaningful there, since villagers spawn facing random ways. Under
this load the pacer's budget grows to about 10 ms, because GPU work after present is not in its cost
estimate. Input age stays about 9.8 ms, still below the old timer's.

No new regression test: this changes how bytes reach the GPU, not what is drawn, and the scene
tests that render through `Scene` cover the result.

Gate: 237 tests.

## 60. Pacing: misses from slow frames don't widen the budget

Section 59's staging belt is `3800111377`.

A miss widened the budget by 1 ms even when the frame that missed had overrun its own lead, which
happens while loading or during a hitch. It would have missed anyway, and the budget then took
about 30 s to shrink back. The pacer now records each frame's own cost and the lead it was started
with. The budget only grows when a frame cheaper than its lead still missed, which points at GPU or
compositor time.

In 2,400-frame vsync runs, terrain had 0 misses, a 4 ms budget and input age p50 4.8 ms (p99
5.8 ms). The 441-villager crowd still settles near an 8 ms budget (p50 9.3 ms): its misses are
genuine GPU time after present.

Regression: a miss after a frame slower than its lead leaves the budget alone.

Gate: 237 tests.

## 61. Vanilla menu option

Section 60's pacing fix is `dd6445fead`.

The owner asked for the default menu back on Escape, or a way to switch. The new "Vanilla Menu"
setting (`toggles.vanilla_menu`, saved; off by default) switches the Escape menu and its options
page between the glass sheets and vanilla's own screens. It can be flipped from the Right Shift mod
menu, the glass options sheet (a switch row) or the vanilla options page ("Menu Style" button).

- **Game menu.** It is `GuiIngameMenu` (`axp.b`) button for button: Back to Game (200 wide at
  height/4 + 8), Achievements and Statistics (98 wide at + 32), Options... and Open to LAN
  (+ 80), and Disconnect (+ 104). The title is at y 40 in white, over the default
  0xC0101010–0xD0101010 gradient. Achievements, Statistics and Open to LAN draw disabled, since
  this client has none of those screens (LAN is disabled in multiplayer anyway). The leave
  button keeps its meaning: Disconnect in launcher sessions, Quit otherwise.
- **Options page.** This client's settings on `GuiOptions`' grid: 150 wide, two columns 160
  apart, rows 24 apart from height/6 - 12, Done at height/6 + 168. Sliders are
  `GuiOptionSlider`: the disabled texture behind, a knob from (0, 66) and (196, 66) riding 4
  pixels in from each end, then the text.
- Buttons are `GuiButton.drawButton` from widgets.png (`avs`), through the existing
  `hud::button_widget`.

The glass settings rows are now 22 apart instead of 24, so the extra row still fits a 640x480
window. `--smoke-pause` and `--smoke-vanilla-menu` set up captures: the game menu, the vanilla
options page and the glass sheet with its new switch all render as described.

Regressions: every vanilla game-menu rect at 480x270, disabled buttons doing nothing, Options...
opening the page, the options grid and Done, the vanilla slider's 4-pixel inset at both ends, and
Menu Style switching to glass and back.

Gate: 238 tests.

## 62. Graphite style and HUD cards

Previous commit: 433d6e18a5. First stage of the launcher and menu redesign (Graphite + one
accent). A shared `style` module now holds the look: graphite surfaces with hairline edges and
square corners, one ember accent (#FF5A1F) for the single most important thing on a screen, and
corner brackets (like a hitbox's corners) marking whatever is selected.

- **HUD cards.** Keystrokes, CPS, FPS, armour, potions, coordinates and ping are drawn as
  graphite at 66% with a hairline edge instead of blue liquid glass. They stay legible over a
  bright sky, and the HUD no longer uses the glass blur at all, so normal play skips that pass.
  A pressed key fills near-white with dark text; the space bar is a drawn bar instead of dashes.
- **HUD editor.** Every card gets muted corner brackets; the card being dragged gets ember ones.
  Disabled mods draw at 75% fill with muted text. The old editor tinted every card blue.
- **Default layout.** CPS sat exactly under FPS and Coordinates under Ping, so the editor showed
  overlapping cards. CPS now defaults below Ping and Coordinates to the top right. Saved
  layouts are unchanged.
- **Potion placeholder.** With no effects active, the editor's "Potion Effects" text ran past
  its card; the card is now sized to the text.
- `--smoke-hud-editor` opens the HUD editor for captures.

Regressions: the default cards never overlap at 480x270, and `fade` scales only alpha.

Gate: 240 tests.

## 63. A launcher of its own

Previous commit: 0113bac9e2. Opening the app now shows a separate, Lunar-style launcher instead
of the glass card over the panorama, and Play starts the game as its own process.

- **Layout.** A 48-unit sidebar (the H mark in corner brackets, then Home, Servers, News, Mods
  and Settings as 8x8 pixel icons with labels; the open page has ember brackets) and a page
  area drawn in a fixed 480x300 design space scaled to the window, so it looks the same at any
  GUI scale. Graphite throughout; ember only on PLAY, the selected server and the open page.
- **Home.** "Welcome back" with the account name (or a sign-in prompt), a launch card (Minecraft
  1.8.9, Horizon, update status, PLAY with the selected server's player count), quick-play
  tiles with live status, and the newest News entry.
- **Live status.** `status.rs` does vanilla's server-list ping (`ServerPinger.ping`): handshake
  with next state 1, status request, then a ping whose round trip is the latency. Each address
  is queried on its own thread, at most every 20 s, keeping the last answer meanwhile. Hypixel
  showed about 30,000 online at 55-58 ms from here; a closed port reads "Can't reach".
- **Servers, News, Mods, Settings.** The server list keeps add, remove, select and double-click
  to play, and scrolls to keep the selection visible. News is `NEWS.md` (player-facing, newest
  first). Mods switches the seven HUD mods and nine toggles or cycles; Settings steps FOV,
  sensitivity, render distance, GUI scale and frame limit, switches fullscreen, view bobbing
  and vanilla menus, and shows the update status (Restart once one is installed).
- **Play.** The launcher starts `<program> --play host:port` and writes one JSON line to its
  stdin: username, UUID and, when signed in, the access token (never an argument, so it
  never shows in the process list; `Session::access_token` is the new getter). Without a token
  the game joins offline (servers on this Mac only, as before). The game's log goes to
  `game.log` in Application Support. The launcher then quits; if an update is still being
  checked or built it hides instead and quits once the update is installed.
- **The game just plays.** Started with `--play`, it does no update checks. Disconnect (game
  menu, mod menu or the connection screen's back button) reopens the launcher from the
  program on disk, so a freshly installed update is what opens.
- `--smoke-page N` opens a launcher page for captures.

Verified: every page captured at 960x600; `--play` with an offline session on stdin logs in
to the reference server and renders (`WORLD_METAL_SMOKE_PASS`).

Regressions: the session handoff round trip (online and offline) and `--play` argument rules,
the status query against a fake server (handshake bytes, players, ping) and against a closed
port, status text without formatting codes, every page fitting with no overlapping cards at
five window sizes, sidebar and card clicks (mod switch, FOV stepper both ways and its clamp,
frame-limit end stop, Restart only when ready), sign-in gating and the scrolling list, and
News parsing.

Gate: 245 tests.

## 64. Bob Client, and the mod menu rebuilt

Previous commit: 8a49ee57d0.

- **Name.** Everything a player sees says Bob Client: window titles, alerts, the F3 build line,
  the launcher and News. The app bundle is now `Bob Client.app`. The updater renames an
  installed `Horizon 1.8.9.app` in place after installing an update (a rename, never a copy, and
  never over an existing `Bob Client.app`) and rewrites its `Info.plist`; `--install` does the
  same. A running launcher keeps its launch path, so `updater::program()` finds the program in
  the renamed bundle for Play, Restart and reopening the launcher. Internal names (crates, the
  Application Support folder, the bundle identifier) are unchanged.
- **Mod menu (Right Shift).** A graphite sheet over the dimmed world (no glass blur): a
  category rail (All, HUD, Gameplay, Visual, General, with counts; the open one has the ember
  brackets), type-to-search on the open menu (Backspace deletes; Escape clears the search
  before it closes the menu), a card grid of up to three columns that the scroll wheel scrolls a
  row per notch with a scroll bar, monochrome square switches that still slide, and Edit HUD,
  Disconnect and Done in a footer. The HUD editor's hint pill is graphite too. The pause and
  options glass sheets keep the old shared helpers.

Regressions: renaming an old bundle in place (and not clobbering an existing one, and finding
the program afterwards); the sheet fitting five screen sizes at every scroll with cards inside
the content area; rail clicks, search, Escape order and wheel limits; the existing click, drag,
animation and editor-resize tests.

Gate: 247 tests.

## 65. The live arena launcher

Previous commit: 156c7b9827. The graphite launcher read as a generic dark dashboard (near-black,
one vermilion accent, hairline rules, all-caps labels). The launcher is now a live 1.8.9 scene
drawn by the client's own renderer, with the interface in Minecraft's own visual language.

- **The scene (`arena.rs`).** An in-memory world: a seeded floating island (grass over dirt
  over a tapering stone and gravel underside, poppies, dandelions, tall grass and a small oak),
  meshed once with `Registry::mesh_column` and uploaded with `apply_mesh`. The player stands
  in the middle holding a diamond sword, in a three-quarter pose facing the camera; the head
  follows the pointer (up to 40 degrees across, 25 up or down); the idle arm sway runs on
  wall-clock ticks. Each page has a camera framing (angle round the player, distance, height,
  and a sideways aim that puts the player left of centre); a page change glides there in 0.9 s
  (ease-out cubic) from wherever the camera was, and a slow 7-degree drift keeps it alive.
  Signed in, the player's own skin comes from Mojang's public profile lookup by UUID
  (`skins::profile`; no token).
- **The interface.** Minecraft's font for everything (the wordmark at 3x, Play at 2x), with its
  drop shadow. Pages are a vertical hotbar: hotbar slots from widgets.png holding a bed,
  compass, book, redstone and comparator, the open page in the hotbar's selection frame, slot
  numbers in the corner, and keys 1-5 to switch (not while typing an address). Servers sit in
  vanilla's tooltip frame (`drawHoveringText`'s violet fill and purple border; white for the
  selected one) with `GuiServerList`'s ping bars from icons.png (bars by ping, the animated
  pinging frames, the cross when unreachable). Play is a 2x vanilla button with a diamond
  sword and the server's name. The address box is `GuiTextField`. Mods and Settings are
  two-column lists of vanilla buttons in a tooltip frame; News is paged (Back and Next).
- **Crisp at any size.** The 480x300 design space is scaled by a whole number of physical
  pixels, so font and sprites never blur.

Verified with captures of every page at 960x600: live counts and ping bars, the island and
player, no overlaps.

Regressions: the island (solid under the player, floating, the same every launch), every page's
framing putting the player in front and left of centre, the glide starting from the last
framing and landing exactly, whole-pixel scales and no overlaps at six window sizes, hotbar
clicks and number keys (not while typing), stepper halves and end stops, Restart only when
ready, News paging bounds, and profile parsing for skins.

Gate: 251 tests.

## 66. The deterministic packet-trace benchmark

Previous commit: b5a1582f95. Sprint phase 1.1, first item (docs/sprint-queue.md).

- **`horizon-189-trace` (new tool crate).** `record` proxies one offline session between the
  client and a test server and writes every play packet both ways with its time (`BOBTRACE`
  format: direction, ms, length, body). `serve` is that server again: offline login with
  compression at 256, the recorded server packets on their recorded timeline (minus the
  recording's own disconnect), a seeded crowd of fighting players, `BOBTRACE PHASE <name>` and
  `BOBTRACE END` chat markers, and an optional log of everything the client sends (for the
  Watchdog suite). The crowd: tab list entries and spawns holding diamond swords, then every
  tick a strafe round its own spot, a head turn, swings every 3-8 ticks and a hurt flash every 23;
  removed at `--crowd-until`. The same seed gives the same packets.
- **The session** (`tools/bob-trace-record.sh`, private trace): 14 s standing at spawn, then up
  30 blocks and east at 16 blocks/s for 30 s with view distance 16 (865 chunk packets).
  `tools/bob-trace-bench.sh` replays it with 80 fighters from 1.5 to 15.3 s, phases lobby@4000
  and flyover@15300.
- **`--bench-trace`** runs uncapped until `BOBTRACE END`, counting every frame and finished mesh
  against the current phase in fixed microsecond histograms (nothing allocated per frame), then
  prints `BOBTRACE_RESULT` per phase: average FPS, 1% lows (mean of the slowest 1% of frame
  intervals), p50/p99/p99.9 frame interval, CPU and GPU p50/p99, world draw calls per frame,
  allocations per frame (n/a until the counting allocator lands), meshes and mesh latency.
  Mesh latency runs from a column first going dirty to its finished mesh reaching the renderer
  (a column dirtied again while meshing counts from its first dirty time). The script samples
  resident memory.

Baseline (M5, median of four runs):

| phase | avg FPS | 1% low | p99 frame | p99.9 | CPU p50/p99 | GPU p99 | draws/frame | mesh p50/p99 | RAM |
|---|---|---|---|---|---|---|---|---|---|
| lobby, 80 players | 657 | 374 | 2.27 ms | 2.94 ms | 0.22/0.94 ms | 1.67 ms | 704 | 2.4/7.8 ms | 265 MB peak |
| flyover, 16 chunks | 745 | 296 | 3.06 ms | 3.79 ms | 0.24/1.66 ms | 1.57 ms | 697 | 4.8/32.5 ms | |

Run-to-run noise: lobby average FPS within 0.5%, flyover within 4%, lobby CPU p99 0.55-0.94 ms.
The gate now runs the benchmark three times and fails on a median drop over 3% (lobby FPS) or
5% (flyover FPS), or a lobby CPU p99 rise over 25%. The first gated run: 646/717 FPS, CPU p99
0.99 ms, no regression. About 700 world draw calls a frame (one per run of visible sections)
and two `Vec`s allocated per frame in `Scene::draw` are the first targets for 1.2.

Regressions: trace round trip and damage, crowd packets decoded by the client's own decoders,
seeded determinism and bounded movement, chat markers, keep-alive and destroy encoding, serve
options and timeline ordering (crowd ticks, leave, phases, end), teleport parsing, not replaying
the recording's disconnect; the histogram (quantiles, 1% lows, overflow), phase markers and
splitting, and mesh latency from first dirty through a stale rebuild.

Gate: 254 client tests and 8 trace-tool tests.

## 67. The Watchdog packet-parity suite; Toggle Sneak removed

Previous commit: 4d421d79bc. Sprint standing rule (Watchdog gate) and the never-build list.

- **Toggle Sneak is gone** (it is on the never-build list): the setting, its key latch, and its
  cards in the Right Shift menu and the launcher's Mods page. Older settings files with it on
  still load and the key is never written back. Toggle Sprint stays (sprint only; the network
  thread still decides C0B by vanilla's rules).
- **`horizon-189-trace check`** runs the rules over everything a client sent: only packets a
  vanilla client sends in such a session; 18-22 movement packets in every whole second; a
  position at least every 21st tick (vanilla tests `positionUpdateTicks >= 20` before raising
  it, so a still player sends 20 look-only packets between positions - the first draft said 19
  and the client was right); every attack directly after its swing (C0A then C02); sneak and
  sprint start/stop strictly alternating; no sneak C0B while a screen is open (a stop in the
  first two ticks, from keys released on opening, is vanilla); settings and brand once.
- **`--watchdog`** switches every module on (all HUD cards, zoom, smooth zoom, Toggle Sprint,
  fullbright, scoreboard numbers, advanced tooltips, a custom crosshair and outline, hitboxes)
  and plays a fixed script through the same key and mouse handlers the window uses: five swings
  at a sparring partner, walk, latch sprint, jump, sneak, sneak held with the inventory open,
  sneak held with the Right Shift menu open, F5 three times, zoom, F3+B. Its clock is the
  number of movement packets the network thread has sent, which is exactly how the checker
  counts ticks, so the screen windows (110-140, 145-165) line up with the server's log. The
  window's keyboard and mouse handlers became `key_input` and `mouse_input` for this (no
  behaviour change).
- **`serve --sparring`** adds a still fighter 2.2 blocks straight ahead of the player's
  recorded facing, so the script's swings land real attacks; `tools/bob-watchdog.sh` fails if no
  attack reaches the server, so the swing-then-attack rule is always exercised. Client logs are
  flushed per record so they stay whole however the server stops.
- **Re-recorded trace.** The first recording started where an aborted one ended, mid-air at
  y 80.5, so its lobby had the player falling past a floating crowd. Section 66's absolute
  numbers belong to that trace. The test server's saved player was reset and the session
  recorded again from the ground (y 68.0, 47.9 s, rise at 14.5 s).
- **Benchmark gate is now A/B.** Benchmarks run on a Mac in use (a browser playing video took
  lobby from about 655 to 380 FPS), so a fixed baseline flags load, not code. The gate builds
  origin/main's client once per commit and runs three pairs in alternating order; the verdict is
  the median of the per-pair new/base ratios (lobby FPS -3%, flyover FPS -5%, lobby CPU p99
  +25%). This commit against 4d421d79bc: lobby -0.6%, flyover -3.7%, CPU p99 -3.1% (pairs
  -27.6%, -0.6%, +1.3% / -32.7%, -3.7%, +2.3%; the first pair straddled a load spike). An earlier
  six-pair run under lighter load showed no consistent difference either; nothing in the frame
  loop changed.

The suite on this commit: 235 packets (5 keep-alives, 4 attacks after 5 swings, 143 C03, 67 C04,
1 C06, 6 entity actions, inventory close, status, settings, brand), both windows clean,
WATCHDOG_PASS three runs in a row.

Regressions: every rule passes a vanilla session and catches its own break (foreign packet,
double-rate movement, 21 ticks without position and 20 allowed, attack without swing, double
sprint start, sneak inside a window with the opening stop allowed, settings twice, a window past
the end); window parsing; the sparring partner's placement for two facings and stillness; the
script's order, key pairing and windows; enabling everything; Toggle Sneak settings migration.

Gate: 257 client tests, 12 trace-tool tests, Watchdog pass.

## 68. mimalloc, allocation counts, the release-max profile and the profiling script

Previous commit: 6d642031a4. Sprint 1.1's last two items.

- **mimalloc is the client's global allocator** (`horizon_threads::GameAllocator`, the
  allocator the Horizon client already ships: mimalloc on macOS and Windows, the sampling
  allocator on Linux). It is C, built from the vendored `libmimalloc-sys`; the client crate
  still forbids `unsafe` because the `GlobalAlloc` impl lives in horizon-threads.
- **Allocations per frame.** `--features alloc-count` swaps in horizon-threads' counting
  mimalloc and `--bench-trace` reports the main thread's allocations per frame (it was "n/a").
  Measuring only; the gate benchmarks the default build. First numbers: lobby 356, flyover 220
  allocations per frame on the main thread.
- **`[profile.release-max]`**: fat LTO, one codegen unit, `panic = "abort"`;
  `tools/bob-release-max.sh [--native]` builds it (`--native` adds `-C target-cpu=native`
  into its own target directory, for this Mac only, never shipped). The app's updater still
  builds `release`: with `panic = "abort"` a panic on any thread ends the process, so
  switching the shipped build waits until the threads' panic paths are reviewed.
- **`tools/bob-profile.sh time|metal|alloc|game NAME`** replays the benchmark session under
  `xctrace record` (Time Profiler, Metal System Trace, Allocations, Game Performance) and
  `tools/bob-profile-summary.py` prints, from 4 s on: CPU per thread, the allocator's share
  of each thread, each busy thread's functions by self time (Rust v0 symbols demangled to
  paths) and Bob Client's own functions by total time; for Metal traces also the GPU work by
  channel and the command buffers' CPU encoding by label, with count, total, mean and p99.

Measurements (the Mac was playing video throughout: lobby ~370 FPS here against ~655 idle):

- Gate A/B against 6d642031a4, nine pairs over three runs: lobby average FPS -4.2% (pairs from
  -24% to +107%), flyover -3.0%, lobby CPU p99 -2.8%; frame p50 +0.2%, CPU p50 +0.3%. The lobby
  FPS median is past the -3% line, so the change was profiled before committing.
- Time Profiler, two runs each, system heap -> mimalloc: allocator share of all CPU 1.87% /
  2.01% -> 1.01% / 0.87%; main thread 2.42% / 2.43% -> 1.74% / 1.59%; mesh workers 1.4% / 2.2%
  -> 0.3% / 0.2%. mimalloc halves the allocator's cost; that is about 1% of frame CPU, below
  what the A/B resolves on a loaded Mac, so the flagged median is load, not this change.
- release vs release-max, three pairs: lobby frame p50 -3.1% (-3%, -3%, -4%), CPU p50 -3.6%,
  average FPS +4.9%, mesh p50 -4.2%; flyover flat (-0.3% FPS, CPU p50 -2.7%). The binary is
  7.8 MB against 10.7 MB. `target-cpu=native` not measured.

What the first profiles show for 1.2 (lobby, main thread): `terrain::Registry::item_mesh`
10-16% self time through `hand::generated_item` (the held item's mesh is rebuilt every frame),
`models::add` 7-11%, `memmove` 8-10%, `Renderer::set_entities` 25% total; on the mesh workers
SipHash (`DefaultHasher`) is 45-50% of `mesh_column`'s time; a blit encoder costs 0.36 ms of
CPU per frame on average (p99 1.8 ms).

Regressions: the main thread's allocation count, which counts only its own blocks and is 0
without counting; the earlier `--bench-trace` report test.

Gate: 258 client tests (and clippy with `alloc-count`), 12 trace-tool tests, Watchdog pass.

## 69. Palette-compressed sections and a fast hash for world lookups

Previous commit: 81428beeae. Sprint 1.2, first item.

- **Sections are stored compactly** (`horizon_world_189::palette`). The wire's 4096 16-bit
  states become one value for a uniform section, 4-bit indices into a palette of up to 16
  states, 8-bit indices for up to 256, or the raw array beyond that; each light plane is one
  level when every block shares it (block light 0 in most sections, sky light 15 above ground
  and 0 below) or the 2048-byte plane otherwise. A typical surface section drops from 12 KiB
  to about 4 KiB. Block changes widen the storage as new states arrive (uniform -> 4 -> 8 bits
  -> raw) and never lose a block. `Section`'s fields are private now: `state_at`, `states()`
  (`unpack` fills a 4096-entry array for the mesher's per-section loop), `light`,
  `has_sky_light`, `heap_bytes`. The mesher unpacks a section once and reads the array.
- **The world's coordinate maps hash with `horizon_world_189::Keys`**: multiply-rotate per
  integer with a splitmix64 finish, keyed randomly per map so a server cannot pick colliding
  columns. The first Time Profiler run showed SipHash at about half of every mesh worker's
  time (`hash_one` + `DefaultHasher::write` around `World::block` and `World::light`, a
  dozen lookups per block); it is gone from the profile, and each mesh worker used 363 ms of
  CPU against 1163 ms for the same session.
- "Shared immutable state tables": uniform sections and light planes need no table at all;
  palettes are per section and small (2-512 bytes), so they are not interned.

Measurements, A/B against 81428beeae (gate, three pairs, Mac playing video): lobby FPS -0.2%,
flyover -1.5%, lobby CPU p99 +0.9% (all inside noise), BENCH_REGRESSION=0. Chunk mesh latency:
lobby p50 4.1 -> 2.4 ms, p99 12.5 -> 7.7 ms; flyover p50 6.6 -> 4.1 ms, p99 66.7 -> 34.3 ms
(medians). Resident memory over the session, two runs each: peak 311 / 313 -> 268 / 261 MB,
mean 289 / 289 -> 245 / 241 MB.

Regressions: each storage form round-trips exactly; block changes widen through every form
with every block checked; light planes keep both nibbles and collapse only when uniform; a
decoded surface column's heap size; block changes through the world; the hash spreads a
render distance's columns over the table, differs per world and separates [1,2,3]/[1,2,4].

Gate: 263 tests, 12 trace-tool tests, Watchdog pass.

## 70. Clippy fix for section 69

Previous commit: 8c8efd13b3. That commit went out with two clippy errors in horizon-world-189
(`chunks_exact` with a constant size in the key hash, overlapping match ranges in the palette
choice): the gate printed CLIPPY=101 and the push went ahead on the benchmark lines alone. Both
are rewritten with identical behaviour (`as_chunks::<8>`, explicit `2..=16` / `17..=256`).
From here the whole gate line set is read before every push, not its tail.

Gate: 263 tests, 12 trace-tool tests, Watchdog pass; clippy clean. No benchmark: no change in
behaviour or generated code paths.

## 71. Greedy meshing

Previous commit: fee37526cd. Sprint 1.2, second item.

- **Neighbouring full faces that look the same become one quad** (`greedy.rs`). A face takes
  part when it covers its whole block side on the block boundary and maps exactly one sprite
  across it in any orientation (`Grid::of`, computed once per baked face); two cells merge
  only with the same plane, sprite, texture origin and orientation, and colour (tint x face
  shade x flat light), so light and biome-tint edges stay. Per section, each pass's faces are
  grouped by plane and merged row by row into rectangles.
- **Draw order on a plane is kept**: a block's faces in a pass wait for merging only when all
  of them can, and faces are merged by their rank on the plane, so the grass side overlay
  still draws after the side it covers. Liquids, translucent faces, block entities and partial
  faces are emitted as before.
- **Texture coordinates repeat**: terrain UVs are now unsigned 1/2048 sprites up to 31
  (`TerrainVertex::UV_SCALE`, `Uint16x2`) and the terrain sampler repeats; each sprite is its
  own array layer, and with nearest texels a 0-1 quad samples exactly as with clamping
  (a capture with merging switched off differs from the previous build in 7 pixels, the FPS
  text). A merged quad whose texture runs backwards is shifted by whole sprites to stay
  positive.

Measurements, A/B against fee37526cd (three pairs): lobby average FPS +7.1% (+4.3%, +13.5%,
+7.1%), flyover +6.3%, lobby CPU p99 -10.2% (every pair lower), GPU p50 -6% to -8% in every
pair. Mesh time, alternating in-process runs over a hilly test column: 788 us against 766 us
per column (+3%). Mesh latency (dirty to handed over) rose in the flyover: p50 4.3-4.6 ->
4.8-5.2 ms; lobby unchanged within noise. A capture of the replayed session differs from the
per-block build in 523 of 576,000 pixels, all along far grazing edges where a merged edge's
vertices snap to the pixel grid differently from per-block edges; the jar test below shows
every block cell keeps its sprite, colour and texels.

Regressions: grids only for whole boundary faces with one sprite per block (no cull side,
slab top, half sprite rejected; rotated texture accepted); a uniform floor is one quad
spanning the section with the sprite repeated per block; a lit square splits a dark floor
and the area is covered once; mirrored textures shift back to positive coordinates; an
overlay stays after its base; and with the owned jar, a 3 x 3-column hillside of grass, dirt,
stone, gravel, leaves and water under patchy sky light meshes with merging to exactly the
same block cells, passes, sprites, colours and texture corners as without it, the same
translucent geometry and visibility, and fewer than three quarters of the solid quads
(`BOB_MESH_TIMING=1` prints the alternating mesh timings).

Gate: 268 tests, 12 trace-tool tests, Watchdog pass.


## 72. Greedy validation seal and paired latency gate — `03fc5cfb16`

Takeover baseline: `0b45b1d30c` already pushed greedy meshing; comparison baseline `fee37526cd`.
The two dirty files were the following 12-byte packing milestone, not an unfinished greedy commit.
That exact partial diff is preserved privately and will be resumed after this checkpoint. The current
runtime still uses the validated 16-byte vertex layout; release-max remains opt-in.

The owned-jar equivalence fixture is explicitly required by `tools/test_189_reference.py`: a skipped,
filtered or failed greedy test now fails the gate. It decomposes merged and unmerged geometry into
block cells with matching pass, plane, texture corners and colours, and separately compares liquid
geometry and section visibility. The seeded hillside has 4,492 → 3,188 solid/cutout vertices and
2,246 → 1,594 triangles (29.0% fewer). Alternating in-process release timing: greedy p50/p99
1,344/2,269 us, per-block 1,247/2,072 us. This measures merge overhead, not an improvement in CPU
meshing time. Lighting is still flat; future per-vertex AO remains Phase 2 work.

`tools/bob-bench-ab.sh` runs three alternating A/B pairs with isolated runtime homes and a scratch game
directory. `tools/bob_bench_compare.py` gates the median of paired ratios for both lobby and flyover:
median frame interval at most +5%, CPU submit p99 at most +25%. Average FPS and 1% lows remain
reported, together with mesh p99, draws, allocation counters and RSS. It rejects incomplete/nonfinite
results and exits nonzero on a regression; five Python regressions cover verdicts and CLI exit status.
The trace runner also propagates client/server failure and cleans up its own processes. Flagged
latency deltas require `tools/bob-profile.sh` inspection, not a bypass based on average FPS.

Validation so far: 268 scoped Rust tests, 12 trace-tool tests, five benchmark-gate Python tests, strict
Clippy including alloc-count, fmt, layers/public-tree, ARM64 release build,
`VANILLA_189_REFERENCE_GATE_PASS`, and `WATCHDOG_PASS packets=235 windows=2`.
Fresh A/B against fee37526cd, three alternating pairs, default release allocator:

| Phase / metric | Baseline median | Greedy median |
| --- | --- | --- |
| Lobby average FPS / 1% low | 435 / 89 | 522 / 224 |
| Lobby frame p50 / CPU submit p99 (us) | 1,867 / 1,548 | 1,748 / 1,207 |
| Lobby mesh p99 (ms) / draws per frame | 6.3 / 742.4 | 8.4 / 742.7 |
| Flyover average FPS / 1% low | 528 / 139 | 557 / 147 |
| Flyover frame p50 / CPU submit p99 (us) | 1,690 / 2,007 | 1,581 / 2,206 |
| Flyover mesh p99 (ms) / draws per frame | 23.1 / 716.3 | 34.4 / 722.8 |
| Session peak / mean RSS (MiB, median of runs) | 237 / 203 | 267 / 243 |

Gate verdict uses the median of *paired ratios*, not the ratio of table medians: lobby frame
p50 -4.61%, CPU p99 -12.02%; flyover frame p50 -5.68%, CPU p99 +11.66%.
`BENCH_REGRESSION=0`. Mesh p99 and RSS rose, so this is a geometry/GPU improvement, not a claim of
faster meshing or lower memory. Default-build allocation counters report n/a; the separate counted
run below measures allocations without changing the latency gate's allocator configuration.
Evidence stays private under the `h189-nov23-*` prefixes and `h189-trace/bench-nov23-greedy-*`.

The prior palette/hash commits through fee37526cd do not include held-item mesh caching; that remains
part of the zero-allocation work. The partial packing diff remains the next item after this seal.


Section 72 capture/allocator evidence: both default ARM64 releases captured the same recorded lobby
at 960x600, GUI hidden and pitch 35 degrees. Both images were inspected: no new surface holes,
texture stretching or visible lighting seams. Of 3,089 pixels differing by more than 2/255, 3,023
are in the top 100 rows over distant water; repeating each identical binary also changes that band
(2,564 baseline / 4,129 greedy pixels), while the remaining 500 rows are byte-identical between
same-binary repeats. The cross-build difference in those rows is 66 grazing-edge pixels. This does
not claim AO coverage: the renderer still has flat face lighting. Captures and repeats remain private
in `h189-nov23-captures` and `h189-nov23-captures-repeat`.

A separate alloc-count release run reports 352.2 / 208.4 main-thread allocations per frame
(lobby / flyover), peak RSS 278 MiB. It is telemetry, not a paired latency result. The default release
was restored afterward and its SHA-256 matches the binary used for A/B and captures. Zero-allocation
rendering and held-item mesh caching are not complete. The final source gate was rerun after the
harness changes, including all five Python tests; no public server/account action was performed.


## 73. Twelve-byte terrain vertices — `b1fa6fb180`

Previous checkpoint: `03fc5cfb16` / `4a37fcd98b`. Resumed the preserved partial packing diff and
completed both sides of the format: CPU packer, Uint32x3 vertex layout, and the production Metal
shader decoder. Each corner is three u32 words instead of sixteen bytes (25% less vertex payload).
Positions use 1/64 blocks, texture coordinates 1/128 sprites (including sixteen-block greedy spans),
colour retains three 8-bit gamma channels, and texture-layer indices have 11 bits. Ordinary model
sixteenths remain exact; liquid heights round within 1/128 block. Excess texture-layer counts are
rejected before upload rather than silently selecting a different material. No native OS API enters
meshing/storage, and release-max remains opt-in.

`packed_terrain_fields_decode_on_metal` executes the actual production WGSL decoder through Metal
compute, reads the results back and compares 24 cases against independent position/UV/colour values,
including the layer bit crossing at 1023/1024 and the format's bounds. The existing jar-backed
cell/texel equivalence test still runs with the new quantization; both ignored GPU/meshing tests are
now explicitly required by the managed reference gate. The test helpers' block-index scaling and
position access were completed rather than weakening their assertions.

Source gate: 269 scoped Rust tests, 12 trace-tool tests, five benchmark-gate Python tests, strict
Clippy with and without alloc-count, fmt, layers/public-tree, ARM64 release,
`VANILLA_189_REFERENCE_GATE_PASS`, and `WATCHDOG_PASS packets=235 windows=2`.
Paired A/B against the default 16-byte release from the previous checkpoint (three pairs):

| Phase / metric | 16-byte median | 12-byte median |
| --- | --- | --- |
| Lobby average FPS / 1% low | 526 / 229 | 509 / 186 |
| Lobby frame p50 / CPU submit p99 (us) | 1,788 / 1,350 | 1,766 / 1,485 |
| Lobby mesh p99 (ms) / draws per frame | 9.9 / 743.3 | 6.8 / 742.9 |
| Flyover average FPS / 1% low | 543 / 163 | 572 / 163 |
| Flyover frame p50 / CPU submit p99 (us) | 1,597 / 1,970 | 1,566 / 1,897 |
| Flyover mesh p99 (ms) / draws per frame | 21.4 / 718.5 | 22.9 / 718.3 |
| Session peak / mean RSS (MiB, median of runs) | 270 / 248 | 273 / 251 |

The gate's paired ratios: lobby frame p50 -1.17%, CPU p99 -12.39%; flyover frame p50 +0.19%,
CPU p99 -3.71%; `BENCH_REGRESSION=0`. The first lobby CPU-tail pair was +57.12%, the other two
-43.04% and -12.39%; the verdict uses all three as specified, with no selected reruns. No FPS or
whole-app RAM improvement is claimed from packing alone.

Both 960x600 release captures were inspected: every pixel below the top 100 rows is identical;
1,565 differing pixels are in the distant-water band that also varies in same-binary repeats
(Section 72). No new visible holes, seams, light or texture changes. Private evidence:
`h189-nov23-packed-*` and `h189-trace/bench-nov23-packed-*`. Default-build allocation fields remain
n/a; the separate counted measurement is recorded below. Bindless Metal argument buffers are the
next separate sub-milestone; the existing texture array is not counted as that work.


Section 73 counted telemetry: 352.3 / 208.0 main-thread allocations per frame (lobby / flyover),
peak/mean RSS 275/253 MiB, in a separate alloc-count release replay. These are not mixed into the
paired default-release verdict. The restored default binary's SHA-256 matches the A/B/capture build.
The complete source gate was rerun after requiring the production decoder test in the runner.


## 74. Paged Metal argument-buffer terrain resources — `cd50e589b9`

Previous checkpoint: `b1fa6fb180` / `e2bf2f5579`. Terrain now capability-selects a persistent binding
array, encoded by the existing Metal HAL as an argument buffer of GPU resource IDs. Each resource
is a 128-sprite view of the same owned texture storage: vanilla's 361 sprites use three pages, not
361 individually tracked resources. The existing 11-bit material index selects page and layer;
no mesh format, texture upload, lighting or protocol semantics change. The table is constructed once
per scene and reused by solid, cutout and translucent terrain. Entity, hand and UI texture bindings
remain separate. No new C dependency or direct OS API enters the client/core modules.

Only supported devices request texture binding arrays, non-uniform sampled-texture indexing and the
required array-element limit. Missing capabilities/limits choose the existing array path; no new
player-facing flag is needed. Shader assembly enables wgpu's binding-array extension only for the
native path. The initial GPU probe caught the missing extension declaration; it was fixed before
any release gate or push.

`bindless_terrain_pages_match_array_sampling_on_metal` runs both production sampling helpers on
Metal across 260 distinct materials, crossing both page boundaries and a partially populated final
page. Both paths match the independent reference colours and each other byte for byte. The managed
reference gate now explicitly requires this test as well as the prior decoder and jar equivalence
checks. Its normal native smoke logs `TERRAIN_RESOURCES mode=argument-buffer pages=3 layers=361`,
so the integrated path is exercised, not merely a helper. The old no-array-feature device tests
continue to compile/render the fallback shader.

Source gate: 270 scoped Rust tests, 12 trace-tool tests, five benchmark-gate Python tests, strict
Clippy including alloc-count, fmt, layer/public-tree checks, ARM64 release,
`VANILLA_189_REFERENCE_GATE_PASS`, and `WATCHDOG_PASS packets=235 windows=2`.
Paired A/B versus the preceding 12-byte array build, three alternating pairs:

| Phase / metric | Array median | Argument-buffer median |
| --- | --- | --- |
| Lobby average FPS / 1% low | 576 / 298 | 575 / 309 |
| Lobby frame p50 / CPU submit p99 (us) | 1,678 / 783 | 1,682 / 764 |
| Lobby mesh p99 (ms) / draws per frame | 6.1 / 742.7 | 6.2 / 743.0 |
| Flyover average FPS / 1% low | 630 / 266 | 631 / 270 |
| Flyover frame p50 / CPU submit p99 (us) | 1,484 / 1,649 | 1,502 / 1,613 |
| Flyover mesh p99 (ms) / draws per frame | 18.7 / 720.1 | 20.3 / 719.9 |
| Session peak / mean RSS (MiB, median of runs) | 274 / 251 | 272 / 248 |

The paired-ratio gate passes: lobby frame p50 +0.24%, CPU p99 +1.67%; flyover frame p50 +1.21%,
CPU p99 +3.00%; `BENCH_REGRESSION=0`. Lobby CPU-tail pairs were +1.67%, -2.43%, +433.21%.
That last outlier is retained, not replaced with a selected rerun. It triggered two fresh 20-second
Time Profiler recordings through `tools/bob-profile.sh`, excluding startup before 4 s: sampled
main-thread CPU 4,426 ms array vs 4,416 ms argument-buffer; Renderer::render total 2,470 vs 2,406 ms,
Scene::draw_terrain total 370 vs 345 ms. The profiles do not reproduce a sustained CPU cost increase;
they do not identify the cause of the earlier tail spike or certify an absence of hitches. The
existing item-mesh rebuild remains the largest own self-time hotspot (12-13%). No FPS gain is claimed.

Both 960x600 release captures were inspected: all pixels below the top 100 rows match exactly;
1,555 pixels differ only in the already-variable distant-water band. The new capture logs the
argument-buffer path with three pages. Separate counted telemetry is 364.1 / 219.9 main-thread
allocations per frame (lobby / flyover), peak/mean RSS 272/250 MiB: about twelve more allocations per
frame than the preceding counted build, so zero-allocation work remains open. The default binary
was restored and SHA-256 checked against the A/B/capture build afterward.

Evidence is private under `h189-nov23-bindless-*`, including captures and both profiler summaries;
raw Instruments traces stay with the private trace data. Full source gate rerun: 270 scoped tests,
12 trace tests, five Python tests, strict Clippy both modes, fmt, layers/public-tree,
`VANILLA_189_REFERENCE_GATE_PASS`, and Watchdog parity. No public-server or account action.
Next in order: distance/frustum-prioritized async meshing and macOS thread QoS requests. QoS is a
scheduling request, not a guarantee of physical P/E-core affinity.


## 75. View-prioritized asynchronous meshing and macOS QoS — b895d0be5d

Previous checkpoint: `cd50e589b9` / `9618caed17`. The window publishes its actual current frustum
(FOV, perspective and mesh origin included) into a latest-value mailbox using try_write. The mesher
samples it using try_read: no render-thread wait, per-frame channel allocation or unbounded camera
queue. Camera data and column/frustum math remain portable Rust. Invalid/nonfinite camera planes
fall back to distance ordering.

Ordinary dispatch turns keep the immediate 3x3 column neighbourhood first, then work waiting at least
250 ms, then visible columns before off-camera columns. Distance and coordinates break ties.
One turn in eight reserves service for aged jobs, ordered by least-recent service and dirty age;
a continuously re-dirtied nearby column cannot monopolize that turn. Completed jobs release their
service records. An additional stress regression caught starvation in the first priority draft;
the final fair-service revision gets its own complete gate and A/B run. This only
changes scheduling: no columns are discarded, one column is never meshed twice concurrently, and
old-origin results are still rebuilt. A new regression reproduced signed distance overflow on
(i32::MIN, i32::MIN); u128 distance arithmetic now orders every server column coordinate safely.
Tests cover nearby terrain, view priority, starvation prevention, large rebased coordinates and
invalid camera fallback without sleeping.

A portable scheduling policy seam requests Interactive QoS for the render/input and tick threads,
and Utility for mesh workers/coordinator, packet decoding, skin decoding, screenshots, server status
and updater work. The macOS implementation calls the existing native backend; other targets retain
a no-op policy. macOS accepted every observed request in the native smoke. An independent kernel
getter test verifies the Utility class on a spawned thread; the existing Interactive getter test
also passes. These are scheduling requests, not measured physical-core affinity: see Apple's
[Apple silicon performance guide](https://developer.apple.com/documentation/apple-silicon/tuning-your-code-s-performance-for-apple-silicon).
No protocol fields, simulation rules, TCP settings or player input bindings change.

The managed reference gate now also runs horizon-threads tests. Scope is 273 legacy tests plus
36 thread/backend tests (309 total), with 12 trace-tool tests and five Python gate tests separately.
Source gate: strict Clippy (including alloc-count and thread tests), fmt, layers/public-tree, ARM64
release, `VANILLA_189_REFERENCE_GATE_PASS`, and `WATCHDOG_PASS packets=235 windows=2`.
Review also caught the camera publication attached to the launcher-only branch, where the network
is absent. It now follows the live camera's final FOV and pose, including camera effects. The
reference gate requires MESH_VIEW_READY from that live path, so a disconnected publication can no
longer pass the native smoke. Earlier priority-fair timings/captures were pilots for the incomplete
wiring and are not the shipped revision's evidence. The corrected live-camera revision receives
a fresh full gate, three alternating A/B pairs, captures and counted telemetry before shipping.

Final corrected live-camera evidence (three alternating pairs against the argument-buffer default):

| Phase / metric | Previous default median | Priority / QoS median |
| --- | --- | --- |
| Lobby average FPS / 1% low | 546 / 253 | 545 / 253 |
| Lobby frame p50 / CPU submit p99 (us) | 1765 / 1110 | 1715 / 1125 |
| Lobby frame p99 / GPU p99 (us) | 3296 / 2305 | 3334 / 2176 |
| Lobby mesh p99 (ms) / draws per frame | 7.8 / 743.1 | 8.9 / 743.4 |
| Flyover average FPS / 1% low | 579 / 152 | 590 / 204 |
| Flyover frame p50 / CPU submit p99 (us) | 1577 / 2013 | 1554 / 1756 |
| Flyover frame p99 / GPU p99 (us) | 4624 / 2965 | 4140 / 3028 |
| Flyover mesh p99 (ms) / draws per frame | 25.3 / 721 | 24.5 / 719.5 |
| Peak / mean RSS (MiB) | 277 / 251 | 275 / 250 |

The paired-ratio gate passes: lobby frame p50 -0.51%, CPU submit p99 +1.35%; flyover frame p50
-0.57%, CPU p99 -11.28%; BENCH_REGRESSION=0. All six phase/pair CPU-tail deltas remain below the
+25% threshold; no performance gain beyond noise is claimed. The earlier incomplete-wiring pilot
had a +43.92% lobby-tail pair; its two 20-second diagnostic profiles sampled 4,595 vs 4,620 ms on
the main thread, without reproducing that sustained increase. Those pilot profiles are not used
as the final revision's performance gate.

Both corrected 960x600 release captures were inspected: below the top 100 rows every pixel is
identical, and all 2,564 differences are in the previously variable distant-water band. Separate
counted telemetry is 362.2 / 220.4 main-thread allocations per frame (lobby / flyover), peak/mean
RSS 277/253 MiB. Default shipping binary restored and compared byte-for-byte with the captured
and A/B-tested binary afterward. Zero-allocation rendering remains open.

Final evidence is private under h189-nov23-priority-live-* (incomplete pilot: priority-fair-*).
The full corrected source gate passes 309 scoped tests, 12 trace tests, five Python gate tests,
strict Clippy both modes, fmt, layers/public-tree, ARM64 default release, native live-camera
publication, VANILLA_189_REFERENCE_GATE_PASS and WATCHDOG_PASS packets=235 windows=2.
No public-server or account action. Next: persistent GPU terrain render regions, followed by
compute frustum/occlusion culling and actual native MTLIndirectCommandBuffer execution.


## 76. Persistent terrain render regions — 2da5b095bc

Previous checkpoint: b895d0be5d / 4ea22f3b83. Terrain passes now lease disjoint quad-aligned
ranges inside persistent 3 MiB GPU pages instead of allocating one Metal buffer per column/pass.
Larger passes get a suitably sized page, bounded by device limits and signed base-vertex range.
The portable free-range allocator coalesces adjacent holes; RAII leases return space after mesh
replacement/removal and on partial construction failure. Live meshes retain their pages. Trimming
keeps at most one spare standard empty page, while disconnect drops all terrain pages. Surface
format recreation transfers geometry and its page pool together.

Drawing uses the shared quad indices plus each lease's base vertex. Consecutive meshes in the
same page share a vertex binding; near-to-far solid/cutout order and far-to-near translucent order
remain intact. Page pooling does not yet implement compute culling or MTLIndirectCommandBuffer;
those remain the next ordered work. Block-entity/entity buffers are unchanged.

A 10,000-operation fragmented allocate/free regression checks that live ranges never overlap,
free plus live ranges exactly cover capacity, and final releases coalesce to the full page.
A required native Metal readback regression submits a copy of old geometry, releases its lease,
uploads replacement bytes into the same range, and verifies the earlier submission retains its
original bytes while the replacement and adjacent live geometry remain correct. It also checks
page spill, reuse, empty-page trimming and invalid quad payload rejection. Terrain uploads occur
outside render-pass encoding, after the preceding frame was submitted; queue ordering protects
those earlier frames. Both new regressions pass. Full verification is recorded below.


Final source gate: 311 scoped Rust tests (including the required region Metal readback), 12
trace-tool tests, five benchmark-gate Python tests, strict Clippy including alloc-count, fmt,
layer/public-tree checks, ARM64 default release, VANILLA_189_REFERENCE_GATE_PASS and
WATCHDOG_PASS packets=235 windows=2. Three alternating A/B pairs against the preceding priority/QoS
default give:

| Phase / metric | Previous default median | Persistent regions median |
| --- | --- | --- |
| Lobby average FPS / 1% low | 547 / 288 | 622 / 280 |
| Lobby frame p50 / CPU submit p99 (us) | 1767 / 970 | 1542 / 891 |
| Lobby frame p99 / GPU p99 (us) | 3113 / 2293 | 3075 / 2350 |
| Lobby mesh p99 (ms) / draws per frame | 6.6 / 742.8 | 6.3 / 742.7 |
| Flyover average FPS / 1% low | 580 / 179 | 661 / 203 |
| Flyover frame p50 / CPU submit p99 (us) | 1570 / 1847 | 1370 / 1663 |
| Flyover frame p99 / GPU p99 (us) | 4292 / 3113 | 4066 / 2966 |
| Flyover mesh p99 (ms) / draws per frame | 25.9 / 720.1 | 25.8 / 718.2 |
| Peak / mean RSS (MiB) | 274 / 251 | 255 / 239 |

The paired-ratio gate passes: lobby frame p50 -10.97%, CPU submit p99 +18.48%; flyover frame
p50 -12.80%, CPU p99 -9.96%; BENCH_REGRESSION=0. Lobby CPU-tail pairs were +18.48%, -56.21%,
+56.08%; all are retained. Fresh 20-second Time Profiler recordings (excluding startup before
4 s) sample 4,350 ms of main-thread CPU in each build; Renderer::render total 2,408 vs 2,215 ms,
Scene::draw_terrain total 297 vs 270 ms. The profiles do not reproduce a sustained CPU increase;
the earlier tail spike remains unexplained, and no absence-of-hitches claim is made. Draw counts
stay essentially unchanged: this is resource reuse, not compute culling or indirect execution.

Both 960x600 default release captures were inspected and are pixel-identical, including the
water band. Counted telemetry is 361.0 / 212.9 main-thread allocations per frame (lobby / flyover),
peak/mean RSS 262/242 MiB; the zero-allocation milestone remains open. The default binary was
restored and compared byte-for-byte with the captured/A/B-tested build. Private evidence prefix:
h189-nov23-regions-*. No public-server/account action. Next: genuine Metal compute frustum culling
into native indirect command buffers, followed by conservative GPU occlusion.


## 78. Isolated benchmark checkpoint (26b5baca5a)

The benchmark harness changes are now verified independently of the parked native ICB branch.
Bench windows stay hidden at 960x600 physical pixels, hold a scoped macOS activity guard, and
release that guard on exit. Paired evidence rejects cursor capture and inconsistent drawable
size/backing scale. Ordinary game rendering and requested frame limits are unchanged.

Fresh validation: 312 scoped Rust tests, 12 trace-tool tests, seven Python benchmark-gate tests,
strict Clippy including alloc-count, fmt, layer/public-tree/privacy checks, ARM64 default release,
VANILLA_189_REFERENCE_GATE_PASS and WATCHDOG_PASS packets=235 windows=2. Inspected hidden
release captures are pixel-identical. Three alternating pairs against the matching private
regions reference pass: lobby frame p50 +0.20%, CPU submit p99 -0.78%; flyover +0.16% / +0.21%.
Average FPS is 671 -> 667 / 765 -> 762, 1% lows 387 -> 386 / 316 -> 313; mesh p99 6.1 -> 6.0 /
21.9 -> 20.9 ms (lobby/flyover). No speedup is claimed for this measurement-only change.
Evidence prefix: resume-harness. The native culling/occlusion commits remain on local work
branches and are not included in this checkpoint. Entity geometry caching and input/pacing
changes are being reviewed separately; no unverified FPS/latency claims are made for them.

## 79. Bounded entity and item geometry caches

Parent checkpoint: 995992c8fb. Local cuboid geometry is cached per render thread with a hard
256-entry bound; pose, lighting, tint, skin/cape layer and glint remain dynamic. Item sprite
extrusions are built once per registry layer, and each distinct point is transformed once per
emission. Held-item layer selection no longer allocates. Skin lookup/pruning avoids repeated
linear searches. No packets, player identity or animation timing change.

Validation passes 316 scoped Rust tests, 12 trace tests, seven Python gate tests, strict Clippy
including alloc-count, fmt, layer/public-tree/privacy checks, ARM64 default release,
VANILLA_189_REFERENCE_GATE_PASS and WATCHDOG_PASS packets=235 windows=2. Independent legacy
geometry references match byte for byte, including all 4,824 owned item layers. Inspected
terrain captures are pixel-identical. Separate timed 80-player crowd captures show player skins,
held items and hurt tint correctly; their animation poses differ and are not a pixel-equality
test. The earlier crowd capture without a spawn delay had no visible actors and is not evidence.

Three alternating release pairs against the isolated harness checkpoint all pass:

| Phase / metric | Previous default | Entity cache |
| --- | --- | --- |
| Lobby average FPS / 1% low | 668 / 386 | 683 / 393 |
| Lobby frame p50 / CPU submit p99 (us) | 1485 / 630 | 1448 / 545 |
| Lobby frame p99 / GPU p99 (us) | 1971 / 1371 | 1945 / 1392 |
| Lobby mesh p99 (ms) | 5.9 | 5.8 |
| Flyover average FPS / 1% low | 762 / 315 | 762 / 313 |
| Flyover frame p50 / CPU submit p99 (us) | 1292 / 1455 | 1290 / 1488 |
| Flyover frame p99 / GPU p99 (us) | 2879 / 1391 | 2898 / 1391 |
| Flyover mesh p99 (ms) | 21.8 | 21.3 |

Paired frame p50 ratios are -2.49% / -0.15%, CPU submit p99 -11.59% / +2.27%
(lobby/flyover), BENCH_REGRESSION=0. All pairs are retained; no input-to-photon improvement is
claimed. The native ICB/occlusion work remains separate and unpublished after its earlier
failed performance gate. Pacing and terrain gamma lookup candidates are also unshipped.

Separate counted run: 321.6 / 208.8 main-thread allocations per frame, peak/mean RSS 259/240 MiB.
Uninstrumented paired-run median peak RSS stays 253 MiB; mean RSS 234 -> 234 MiB. Zero-allocation
rendering remains open. Rebuilding after counting changed only mimalloc's embedded build date/time
in file-backed sections (all executable and other data sections match), plus binary identity/signature
metadata. The exact previously captured/A/B-tested default binary was retained and restored.
Private evidence prefix: resume-entities (paired evidence: nov23-resume-entities).
Next: validate the separate presentation/pacing candidate, without claiming raw input or late sampling.

## 80. Presentation and pacing bookkeeping

Parent: 793232ea31. Cache presentation capabilities per surface instead of querying them every
frame. Preserve the requested Vsync/numeric/Unlimited policy across window modes and surface
recreation. Reset the learned phase on presentation-mode changes. Timestamp presentation before
CPU housekeeping and exclude screenshot readbacks from the pacing model. A due Vsync-paced frame
starts without a mesh upload first; short idle intervals retain bounded upload progress. This
does not add late mouse sampling or change the OS event delivery rate.

Validation: 320 scoped Rust tests, 12 trace tests, seven Python gate tests, strict Clippy including
alloc-count, fmt, layers/public-tree/privacy, ARM64 default release, VANILLA_189_REFERENCE_GATE_PASS
and WATCHDOG_PASS packets=235 windows=2. Both inspected 960x600 release captures are pixel-identical.
Three alternating pairs against 793232ea31 pass: lobby frame p50 +0.77%, CPU submit p99 -5.09%;
flyover +0.16% / -0.74%, BENCH_REGRESSION=0. No FPS improvement is claimed.

| Phase / metric | Previous default | Pacing candidate |
| --- | --- | --- |
| Lobby average FPS / 1% low | 681 / 384 | 681 / 393 |
| Lobby frame p50 / p99 (us) | 1446 / 1975 | 1451 / 1927 |
| Lobby CPU submit p99 / GPU p99 (us) | 570 / 1358 | 559 / 1401 |
| Lobby mesh p99 (ms) | 6.6 | 6.2 |
| Flyover average FPS / 1% low | 765 / 311 | 764 / 313 |
| Flyover frame p50 / p99 (us) | 1284 / 2935 | 1286 / 2904 |
| Flyover CPU submit p99 / GPU p99 (us) | 1490 / 1342 | 1459 / 1364 |
| Flyover mesh p99 (ms) | 21.4 | 19.9 |
| Median peak / mean RSS (MiB) | 256 / 234 | 254 / 233 |

All pairs retained; evidence prefix resume-pacing (paired logs: nov23-resume-pacing).
Four alternating 720-frame visible Vsync runs at about 60 Hz give modeled input-age p50
6,564 / 7,659 us for the parent and 8,385 / 6,400 us for this change; modeled misses are
4 / 3 versus 5 / 3. The order reverses the apparent gain, so this is inconclusive and no
latency reduction is claimed. This estimate is not a physical input-to-photon measurement.
Separate counted telemetry: 316.7 / 203.8 main-thread allocations per frame, peak/mean RSS
249/231 MiB. The exact captured and A/B-tested default binary is restored after counting.
Next: measure the separate byte-domain terrain gamma lookup; native ICB work remains private
until its own correctness and performance gates pass.

## 81. Allocation-free world-text glyph traversal

Parent: c412f0b7b0. World labels and sign text use a cloneable two-pass glyph iterator instead
of allocating an intermediate glyph vector on every call. Output vertex buffers are unchanged.
Formatting/color/reset handling, ASCII filtering, floating-point summation and centering retain
their prior behavior. Independent legacy-reference tests compare output bytes across formatting
edge cases, signed transforms and a long label whose width crosses the f32 exact-integer boundary.

Full source gate: 321 scoped Rust tests, 12 trace tests, seven Python benchmark-gate tests,
strict Clippy including alloc-count, fmt, layers/public-tree/privacy, ARM64 default release,
VANILLA_189_REFERENCE_GATE_PASS and WATCHDOG_PASS packets=235 windows=2.

Three alternating release pairs all use 960x600 at backing scale 1 and pass:

| Phase / metric | Previous default | World-text iterator |
| --- | --- | --- |
| Lobby average FPS / 1% low | 640 / 332 | 673 / 380 |
| Lobby frame p50 / p99 (us) | 1523 / 2137 | 1471 / 2034 |
| Lobby CPU submit p99 / GPU p99 (us) | 845 / 1456 | 584 / 1400 |
| Lobby mesh p99 (ms) | 7.0 | 6.3 |
| Flyover average FPS / 1% low | 702 / 244 | 741 / 292 |
| Flyover frame p50 / p99 (us) | 1370 / 3501 | 1306 / 2970 |
| Flyover CPU submit p99 / GPU p99 (us) | 1823 / 1514 | 1480 / 1465 |
| Flyover mesh p99 (ms) | 24.5 | 23.2 |

The paired ratios, rather than ratios of the aggregate table, gate this noisy batch: lobby
frame p50 -0.27%, CPU p99 -1.06%; flyover +0.08% / +0.48%, BENCH_REGRESSION=0. Lobby frame
pairs -0.27%, -5.34%, +6.17%, CPU-tail pairs -1.06%, -42.35%, +43.08%, all retained. Fresh
20-second profiles (excluding startup before 4 s) sample main-thread CPU 4,286 -> 4,230 ms;
Renderer::render total 2,354 -> 2,314 ms, Scene::draw_terrain 324 -> 308 ms. They do not
reproduce a sustained 43% CPU increase; the individual spike remains unexplained. No FPS or
latency gain is claimed. Both inspected terrain captures are pixel-identical. Separate timed
80-player crowd captures show correctly centered labels; animation poses differ, so those
images are visual checks, not pixel-equality evidence.
Uninstrumented median peak/mean RSS is 262/235 -> 254/233 MiB. Separate matched counted runs
at the same viewport/scale report lobby allocations 316.6 -> 246.2 per frame (about 22% fewer),
flyover 203.8 -> 203.7. Counted peak/mean RSS is 260/236 -> 265/240 MiB; no RAM reduction claimed.
The candidate's isolated counted flyover has mesh p99 98.3 ms versus 26.8 ms in its counted
parent run; these diagnostic timings do not replace the uninstrumented release gate, and no
absence-of-hitches claim is made. The exact captured/A/B-tested default binary is restored.
Evidence prefixes: resume-world-text (paired logs: nov23-resume-world-text), resume-text profiles.
Next: integrate and remeasure the native ICB/occlusion candidate against this current default.

The separate gamma-table experiment is parked privately at 5fc2ed82cb, not included here.
Its all-byte GPU test and 321-test source gate pass, and terrain captures are pixel-identical,
but paired frame p50 +0.53% / -0.08% (lobby/flyover) shows no useful speedup. One +31.46% lobby
CPU-tail pair is retained; paired median +8.73%. Fresh profiles sample main-thread CPU
4,257 -> 4,278 ms and do not reproduce that sustained increase. The spike remains unexplained.
No energy or latency benefit is claimed for that private candidate.

## 82. Native GPU-driven terrain: frustum, facing and occlusion culling into indirect commands

Previous commit: 4ea200cdf3. Sprint 1.2 row "Metal compute frustum/occlusion culling into
actual native MTLIndirectCommandBuffer".

- **Native terrain leaf** (`horizon-metal-terrain`, macOS/Metal only behind `cfg`; every other
  adapter keeps the WGSL renderer). A compute kernel tests each section draw against the five
  frustum planes and writes or resets its indexed command in a real `MTLIndirectCommandBuffer`;
  pages of terrain geometry are bound through a Tier 2 argument table, so terrain is at most a
  handful of executions per frame (draw calls 743 -> about 11 in the lobby, 720 -> 8 in flight).
  Vendored source-only wgpu / wgpu-core 30.0.1 add the opt-in native-hooks seam
  (`third_party/wgpu-core/NATIVE_HOOKS.md` records upstream checksums, patch scope and the
  safety contracts); callbacks hold only immutable native handles and are retained through the
  submission fence.
- **Facing groups**: the mesher groups each section's solid and cutout quads by winding normal
  (six axis directions, then any). A group whose every face lies behind its own plane range as
  seen from the eye is never submitted; back faces were culled by the pipeline anyway, so this
  only removes work. The owned jar's 12,235 cull faces all file under their own side.
- **GPU occlusion**, same frame only: solid terrain within two chunks is drawn first; a depth
  pyramid (farthest depth per texel, padded texels far) is built from it; every later command
  whose whole box, projected with this frame's camera, lies behind every pyramid texel it
  touches is reset. Boxes reaching the eye plane always stay. No history, so nothing pops.
- Kernel resets culled commands itself (no reset blit); Metal's optimize pass stays on.

Measurements (three alternating pairs against 4ea200cdf3, hidden 960 x 600, all pairs kept):
lobby frame p50 +2.54% (pairs +8.29%, -2.22%, +2.54%), CPU p99 +7.74% (one pair +65.84%,
profiled: sampled main-thread CPU 3,959 ms against 4,202 ms, not reproduced); flyover frame p50
-5.16% (+1.47%, -8.15%, -5.16%), CPU p99 -20.96%. 1% lows: lobby 230 -> 345 FPS, flyover
205 -> 288 FPS (medians); mesh p99 11.7 -> 6.2 ms lobby. Terrain captures pixel-identical with
occlusion active (`NATIVE_OCCLUSION_ACTIVE`); crowd captures render players, labels, items and
water correctly (crowd poses differ by capture time). Counted build: main-thread allocations
246 -> 275 per frame lobby, 204 -> 234 flyover; peak RSS 263 -> 295 MB (pyramid, command and
input slots). GPU p50 in the lobby is still about 0.1 ms above the WGSL path in single runs:
the extra compute/blit encoders cost more there than culling saves; occlusion and facing win
in the flyover.

Measured along the way (not shipped): cutout terrain is about 45% of GPU frame time in the
lobby and Fancy leaves alone about 37% (skip experiments); ordering faces nearest-first per
facing group made GPU time worse (+45 to +60 us) and was dropped.

Regressions: native frustum include/cull/reset across frames, page alternation and growth,
translucency after entities with producer teardown, packed-vertex boundaries over 2,048 layers,
occlusion drops only commands hidden behind occluders (pixels identical with it off), near-plane
boxes, occluder holes, rectangular depth reduction, mapped-input rejection; facing groups and
runs; jar winding check. Gate: 336 scoped tests, 12 trace-tool tests, 7 Python gate tests,
VANILLA_189_REFERENCE_GATE_PASS, WATCHDOG_PASS packets=235 windows=2.

## 83. Vanilla Graphics: Fast / Fancy

Previous commit: 05b2106276. Not a sprint row: added because leaves dominate GPU time.

- Skip experiments on 26b5baca5a (single runs, lobby GPU p50): cutout terrain about 470 us of
  1,080 us per frame; Fancy leaves alone about 400 us; plants about 30 us; solid terrain about
  270 us; translucent and entities about 30 us each; no world at all about 310 us.
- **Graphics: Fancy / Fast**, vanilla's setting, in the Video settings (both menu styles),
  saved as `video.fancy_graphics`, default Fancy; `--fast-graphics` for runs. Verified from the
  1.8.9 jar (`ahs`/`akg`): `setGraphicsLevel(fancy)` makes `isOpaqueCube` `!fancy` and the
  block layer CUTOUT_MIPPED when fancy, SOLID when fast. `leaves.json` has no cullface, so
  vanilla draws every leaf face in both modes; Fast draws them opaque (no alpha test, so the
  GPU's hidden-surface removal skips covered layers), culls neighbours' faces against leaves
  and treats leaves as opaque in VisGraph. The mesher reads the setting atomically; changing it
  bumps an epoch and the mesh coordinator re-meshes every loaded column.
- Fast against Fancy, same binary, three alternating runs: lobby 641/633 -> 690/687 FPS
  (+8%), GPU p50 1,185 -> 1,075 us; flyover 789/792 -> 888/879 FPS (+12%), GPU p50 1,048 ->
  932 us. Per-pixel savings grow with resolution.
- Gate: default Fancy is pixel-identical to 05b2106276; three paired runs lobby frame p50
  +0.13%, flyover -0.99%, CPU p99 +3.05% / -6.31% (no regression). The Fast capture shows
  opaque leaves with dark texels where Fancy is transparent, as vanilla Fast does.

Regressions: jar test (Fancy: 162 leaf quads in cutout, none solid; Fast: 162 in solid, none in
cutout, the grass side against the leaves culled, a leaf layer seals the VisGraph, epoch counts
changes only); settings default/round-trip and pre-setting files keep Fancy; the vanilla menu
row and the switch flip the setting; `--fast-graphics` option. Gate: 338 scoped tests, 12
trace-tool tests, 7 Python gate tests, VANILLA_189_REFERENCE_GATE_PASS, WATCHDOG_PASS
packets=235 windows=2.

## 84. Entity culling by the occlusion walk, texture animations with Smart Animations

Previous commit: b2887724d7. Sprint row 1.2 "Vanilla VisGraph cave occlusion, entity culling,
Smart Animations"; the VisGraph walk itself shipped earlier (`reachable`, HANDOFF 71-82).

- **Entity culling.** Verified in the 1.8.9 jar (`bfr`, `RenderGlobal.renderEntities`): vanilla
  iterates `renderInfos` (the sections `setupTerrain`'s walk reached) and draws only the
  entities `Chunk.addEntity` filed under them (`floor(posY / 16)` clamped to 0..15), then
  applies `shouldRender`. Bob Client now does the same before its frustum test, so players and
  mobs sealed behind walls or in caves are skipped with their name tags and hitboxes. No walk
  (camera outside the loaded world) culls nothing. The walk is refreshed from the entity pass
  too, so both use this frame's camera section.
- **Texture animations.** Water, lava, fire, portal, prismarine and sea lantern were frozen on
  their first frame. Each terrain sprite with a `.mcmeta` now plays as
  `TextureAtlasSprite.updateAnimation` does (`bmi`): `frametime`, `frames` lists with bare
  or timed entries, and `interpolate` (prismarine) blending each colour channel as
  `(int)(a * d + b * (1 - d))` with the current frame's alpha. Every frame's mip chain is built
  once; the shown frame is computed from 20 Hz ticks, so nothing is stored per tick.
- **Smart Animations.** Meshes record which animations their quads sample (a 32-bit mask per
  column, computed on the mesh worker). A sprite's layer is rewritten only when a column drawn
  this frame samples it and its shown frame changed; out-of-view sprites catch up when seen.
- Per-frame allocations: `prepare` reuses its culled-column and offset lists and sorts
  unstably (the stable sort allocated a merge buffer); the non-native terrain draw no longer
  collects index lists; the native encoder presizes its buffer lists and uses fixed arrays for
  its ranges. Counted builds: lobby 275.0 -> 252.2 allocations/frame, flyover 233.2 -> 210.3.
- Frames in flight were already capped: `desired_maximum_frame_latency` 1 gives the layer
  `maximumDrawableCount` 2, the minimum.
- A/B against b2887724d7, three alternating pairs: culling + allocations lobby frame p50
  -0.06%, flyover -0.08%, CPU p99 -7.23% / +0.13%; with animations added lobby +0.00%, flyover
  +0.00%, CPU p99 +6.28% / -1.28%. The bench crowd stands in the open, so it draws the same
  entities (identical per-frame entity counts); the gain is where entities are hidden.
- Captures: terrain pixel-identical with culling alone; with animations only the water band
  differs (a later water frame), inspected. The 80-player crowd differs only by animation
  timing, inspected.

Regressions: entity section culling (sealed section, unreached and unloaded columns, below and
above the world, no walk); `.mcmeta` parsing and playback against `updateAnimation`
(frametime, listed and timed frames, interpolation, still and malformed sprites); interpolated
blend and mip-chain layout; jar test (water 32 frames at 2 ticks, lava's 38-entry ping-pong,
an interpolated sprite, a water and lava pool marks exactly those animations). Gate: 322 scoped
tests plus 12 jar tests, 12 trace-tool tests, 7 Python gate tests,
VANILLA_189_REFERENCE_GATE_PASS, WATCHDOG_PASS packets=235 windows=2.
