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

Next: finish the parity gaps below, connect skin/cape rendering hooks, then validate a live Hypixel lobby.

## 2. Verified State

- 81 scoped tests pass with `--include-ignored` (client 16, login CLI 2, assets 1, auth 2, physics 16,
  protocol 29, world 15 — counts include the Metal readback and the owned-jar registry test).
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
| `horizon-189-client` | `keyboard.rs` macOS key state; `network.rs` Protocol 47 actor + mesher thread; `terrain.rs` column meshing; `models.rs` entity/block-entity geometry; `scene.rs` Metal resources; `renderer.rs` surface, camera, capture; `gameplay.rs` ordered interaction controller; `inventory.rs` authoritative snapshots/transactions; `hardness.rs` and `items.rs` queried legacy registry facts. |

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
   prediction/crack effects, full first-person item transforms, consumable durations, crafting/shift-click
   and special-container rules. Adventure restrictions are enforced conservatively. Live Hypixel remains
   unverified.
2. Player armor and sword/bow sprites are present, with depth-tested profile nametags. Signed texture
   properties are preserved as skin/cape hooks. Remote texture loading, capes, team label rules, leather
   overlay/glint, first-person hands and exact item display transforms remain pending. Generated item sprites and held
   block geometry now reuse owned textures/models.
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
```

The private `horizon-189-interaction-gate.py` runner starts the official reference server, configures a
local fixture, runs all scoped tests with `--include-ignored`, independently checks placement/mining, and
always stops the server. It sets `HORIZON_189_REFERENCE_GATE`, `HORIZON_189_FIXTURE_READY` and the owned-jar
path for the ignored interaction test. Run that test only through the managed fixture. The private
`horizon-189-mobs-gate.py` runner covers the second player, armor, held items, names, common mobs,
block entities and flight. Both scripts and all server files live in the system temporary directory.
The official server jar SHA-1 is `b58b2ceb36e01bcd8dbf49c8fb66c55a9f0676cd`, using installed Zulu Java 8.

Latest gate outcomes are 81 scoped tests passing, strict scoped Clippy and formatting passing, layer and
public-tree checks passing, interaction gate passing, and native equipment/flight smoke passing with zero
server movement flags. Earlier broad-workspace validation recorded macOS path/tool-assumption failures and disk exhaustion.
This checkpoint revalidates the scoped client packages; the whole upstream workspace remains unverified.

Manual play: run the canonical binary with `--offline --username <name> --server 127.0.0.1` against the
reference server. The cursor captures on join; Escape releases it; click recaptures. Keys are WASD, Space,
Shift for sneak, Control or Left Option for sprint, and double-tap Space to fly where allowed. Left/right
mouse drive attack/dig and interact/use. Number keys or the wheel select hotbar slots. E opens the player
inventory; E or Escape closes it. Container slots accept normal left/right pickup clicks. Tokens remain
memory-only and a later authenticated session requires another device-code login or direct token.
