# Bob Client sprint queue

Feature freeze 2026-11-30, final deadline 2026-12-02. Core phases 1 to 4 in order, each milestone
pushed on its own after the full commit gate; Stretch phase 5 only after every core gate passes.
Tick an item when it ships and note its commit, test count and benchmark numbers below it.

Baseline: b5a1582f95, 251 tests, Live Arena launcher.

## Standing rules

- Stack lock: native Rust and Apple Metal only. No JVM, JNI, OpenGL translation or WebView.
- Portable core: protocol, physics and combat, chunk storage, meshing, resource-pack parsing,
  audio mixing, UI layout and state, and config schemas stay OS-agnostic. macOS APIs live behind
  traits in `#[cfg(target_os = "macos")]` modules. No Windows backends yet.
- Performance gate: every UI or feature commit runs the deterministic release packet-trace
  benchmark; paired A/B median frame time (+5%) and CPU submit p99 (+25%) are gated for both phases.
  Average FPS, 1% lows, mesh p99, draw calls, allocations and RAM remain diagnostic. Any
  flagged delta is verified with `tools/bob-profile.sh` before proceeding.
- Watchdog gate: an automated suite enables every PvP module at once and asserts vanilla 1.8.9
  packet parity (C03-C06 cadence, C0A before C02, no C0B while a menu or container is open, no
  packets from HUD, visual, audio or animation modules).
- Never build: 360-degree Freelook, AutoText or command macros, automated `/locraw`, enemy
  health/armor/reach overlays, entity or cave minimaps, SafeWalk/NoSlowDown, ToggleSneak,
  HitDelayFix, MetalFX frame interpolation, post-process motion blur.

## Commit gate (every push)

1. Scoped and new regression tests pass, Watchdog suite included.
2. `cargo clippy` strict and `cargo fmt --check`.
3. Layer, public-tree and diff privacy scans.
4. ARM64 release build, `VANILLA_189_REFERENCE_GATE_PASS`, no benchmark regression.
5. Release screenshots captured and inspected for UI and rendering changes.
6. `HANDOFF.md` and this file updated with commit, test count and before/after numbers; no agent
   co-author trailers.

## Found during setup

- [x] ToggleSneak is on the never-build list but ships today (`toggles.toggle_sneak`, the Mods
      page, the Right Shift menu, the launcher). Remove it and migrate saved settings.
      Removed in HANDOFF 67.
- [x] The Watchdog suite does not exist yet; it is built in 1.1 alongside the packet-trace
      server, since both drive the client through a scripted session. HANDOFF 67.

## Phase 1: Native Metal/Rust performance ceiling, input and network latency

Targets against Java 1.8.9 + OptiFine (Lunar/OneClient on M5): 2.5-4x average FPS, 4-10x
smoother 1% lows, 5-15x faster chunk meshing, 5-10x lower RAM.

### 1.1 Benchmark and allocator foundation
- [x] Deterministic Protocol 47 packet-trace benchmark: 80-player lobby burst, 16/32-chunk
      flyover, combat entities; reports CPU submit ms, GPU ms, p99/p99.9 frame interval, average
      FPS, 1% lows, chunk mesh latency, draw calls per frame, heap allocations per frame, RAM MB.
      Shipped in the commit after b5a1582f95 (HANDOFF 66): 262 tests. Lobby 657 FPS (1% 374,
      CPU p99 0.94 ms), flyover 745 FPS (1% 296, mesh p99 32.5 ms), 265 MB. Allocations per
      frame wait for the counting allocator.
- [x] Watchdog packet-parity suite (every PvP module on, C03-C06 cadence, C0A before C02, no
      C0B in menus, no extra packets).
      HANDOFF 67: `tools/bob-watchdog.sh`, in the gate. 257 + 12 tests. A/B vs 4d421d79bc:
      lobby -0.6%, flyover -3.7%, CPU p99 -3.1% (no regression). The benchmark gate is now A/B
      against origin/main (median of paired ratios); the trace was re-recorded from the
      ground.
- [x] CLI Metal System Trace / Instruments profiling script.
      HANDOFF 68: `tools/bob-profile.sh` + `tools/bob-profile-summary.py`.
- [x] Global allocator `mimalloc`; `release-max` profile (fat LTO, one codegen unit,
      `panic = "abort"`, optional `-C target-cpu=native`).
      HANDOFF 68: 258 + 12 tests. Allocator share of CPU 1.9-2.0% -> 0.9-1.0% (Time
      Profiler); A/B vs 6d642031a4 lobby FPS -4.2% median of nine noisy pairs (frame p50
      +0.2%), flagged and profiled, see HANDOFF. release-max: lobby frame p50 -3.1%, binary
      -26%; the app still ships `release`. Main thread 356 / 220 allocations per frame
      (lobby / flyover).

### 1.2 OptiFine-class native engine and GPU pipeline
- [x] Palette-compressed sections (4/8-bit indirect palettes, shared immutable state tables).
      HANDOFF 69: 263 + 12 tests. Peak RSS 311 -> 268 MB; mesh p99 flyover 66.7 -> 34.3 ms
      (with the world-map hash); FPS within noise of 81428beeae.
- [x] Greedy meshing and face culling that respects AO and light boundaries.
      HANDOFF 71: 268 + 12 tests. Lobby FPS +7.1%, flyover +6.3%, CPU p99 -10%, GPU p50 -6 to
      -8% vs fee37526cd; flyover mesh p50 +0.5 ms. Terrain is flat-lit (no AO yet); merging
      keys on the full colour, so smooth lighting/AO would split quads the same way.
      Reverified November 23 in `03fc5cfb16` (HANDOFF 72): 268 scoped + 12 trace + 5 gate tests; jar equivalence
      is mandatory. Fresh paired frame p50 -4.61% / -5.68% (lobby / flyover), CPU submit p99
      -12.02% / +11.66%; 29.0% fewer vertices/triangles in the hillside fixture. Mesh p99 and
      RSS increases are recorded, not hidden by the faster frame median.
- [ ] Bit-packed 8-12 byte chunk vertices and bindless Metal argument buffers.
- [ ] Distance/frustum-prioritized async meshing; QoS pinning (USER_INTERACTIVE for tick, raw
      mouse and submission; UTILITY for meshing, decoding and pack rebuilds).
- [ ] Persistent GPU render regions; Metal compute frustum/occlusion culling into
      `MTLIndirectCommandBuffer`.
- [ ] Vanilla VisGraph cave occlusion, entity culling, Smart Animations.
- [ ] Zero-allocation hot loop, unified-memory staging rings, capped frames in flight,
      `MTLBinaryArchive` pipeline cache with startup warmup.

### 1.3 macOS display, scaling, hardware and thermal efficiency
- [ ] Direct-to-display `CAMetalLayer` (opaque, framebufferOnly when no menu blur), VSync toggle
      through `displaySyncEnabled`, `LSApplicationCategoryType` games.
- [ ] Decoupled 3D render scale (50-100%) under a full-resolution 2D layer.
- [ ] Background FPS throttling (15-30 FPS, paused uploads when unfocused, minimized, occluded).
- [ ] Notch safe-area clearance in fullscreen, 60-240 Hz display hotplug, scroll and trackpad
      debouncing for single-slot hotbar steps.

### 1.4 Input, networking and telemetry
- [ ] Raw mouse at 1000 Hz+ with delta aggregation, no acceleration when locked, cursor
      confinement on fast flicks, late look sampling before submission.
- [ ] Scan-code bindings for movement and hotbar; no press-and-hold accent popup in game; full
      macOS text editing in chat and search.
- [ ] `TCP_NODELAY`; socket I/O, zlib and AES/CFB8 on a dedicated thread feeding a lock-free
      queue into the tick loop.
- [ ] Frametime sparkline (CPU, GPU, p99, draw calls, chunks, RAM) and an `MTL_HUD_ENABLED=1`
      launch toggle.

## Phase 2: World completeness, 32+ render distance, resource packs, audio, parity

### 2.1 32+ render distance and world rendering
- [ ] Render distance 2-32 plus optional 48-64 client-side chunk cache; fog, storage and GPU
      regions scale with it.
- [ ] Instanced block entities with culling: chests (single, double, lid), ender chests, signs
      (formatted text), skulls, banners, beds.
- [ ] Sky (sun, moon, stars, day-night colors), weather (rain, snow), smooth lighting with
      per-vertex AO.

### 2.2 Zero-freeze vanilla resource packs
- [ ] Resource Packs screen (available/selected, reorder, `pack.mcmeta`, `pack.png`, open
      folder).
- [ ] Async atlas, font and language rebuild on efficiency cores; atomic swap on a frame
      boundary.
- [ ] Stacked overrides (blocks, items, models, GUI, fonts, colormaps, sky); server packs
      (S48/C19) with SHA-1 cache and rollback on disconnect.
- [ ] Mipmap alpha-bleed padding and anisotropic filtering (off, 2-16x).
- [ ] OptiFine pack features baked at load: CTM, CIT, custom skies, clear water, emissive.

### 2.3 Native 1.8.9 PvP sound engine
- [ ] Portable mixer on CoreAudio with positional 1.8.9 sounds from local assets, wired to the
      nine volume sliders.

### 2.4 Remaining handoff and Hypixel parity fixes
- [ ] Crafting grid consumes ingredients on screen when taking the result.
- [ ] Heart symbols and bold formatting on Hypixel nametags and holograms.
- [ ] Tab-list health objectives as hearts.
- [ ] Full chunk, entity and skin-cache cleanup on every lobby or server transfer.

## Phase 3: Complete vanilla ESC menu, every option, function keys

### 3.1 ESC menu and option screens (persisted, synced through C15)
- [ ] ESC menu: Back to Game, Achievements, Statistics (S37), Options, Open to LAN (disabled in
      multiplayer), Disconnect to the launcher.
- [ ] FOV slider (30-110, Quake Pro) and dynamic FOV with its toggle.
- [ ] Complete Video Settings, vanilla and native performance controls.
- [ ] Controls: remapping, sensitivity, invert mouse, reset one or all.
- [ ] Skin Customization (seven toggles through C15).
- [ ] Music & Sounds (nine sliders wired to the sound engine).
- [ ] Multiplayer Settings (chat visibility, colors, links, opacity, scale, sizes, reduced
      debug info).
- [ ] Language (async hot reload, C15) and the Resource Packs screen.

### 3.2 Function keys and F3 chords
- [ ] F1, F2 (screenshot, optional clipboard copy, corner thumbnail toast), F3 with Metal
      stats, F5 (three views), F11 native fullscreen.
- [ ] F3+B, F3+H, F3+G (chunk borders), F3+A, F3+T (async pack reload), F3+D.

## Phase 4: Metal-native Right Shift menu, HUD editor, PvP mods, unified launcher

### 4.1 Visual and interaction engine
- [ ] Floating modal (~76% x 78%, 18 px corners, top-lit border, soft shadow) over the world.
- [ ] Half-resolution dual-Kawase blur only while open.
- [ ] Instanced SDF rounded-rect pipeline, batched glyphs, critically damped springs at the
      display rate.
- [ ] Pixel bitmap font or MSDF font toggle.
- [ ] Top bar: badge, profiles (Default, BedWars, Duels, SkyWars; hotkeys; import/export;
      read-only auto-profiles from server address or S3B title), type-anywhere search,
      Cmd+K palette, Edit HUD Layout.
- [ ] Navigation: All, Combat, HUD, Visuals, World & Perf, Settings, Keybinds, Themes
      (Obsidian Glass, Arctic Frost, Minecraft Native, custom accent).
- [ ] Three-column mod cards with live 3D item icons, pill toggles and a settings drawer with
      live preview.

### 4.2 Spatial HUD editor
- [ ] Outlines, corner scale handles, snap guides (centres, notch and safe margins, edges),
      arrow nudging, right-click to settings, Reset and Reset All.
- [ ] Per-widget scale, background opacity, border, text shadow, accent/chroma, hide while Tab.
- [ ] Cached HUD text geometry; under 0.1 ms CPU submit with every HUD module on.

### 4.3 Watchdog-safe module roster
- [ ] HUD: FPS, CPS, Ping (S38), Keystrokes, Coordinates, Armor Status, Potion Effects,
      Scoreboard Customizer, Boss Bar Customizer, Local Inventory Resource Counter, Combo
      Counter, Frametime Graph.
- [ ] Combat: ToggleSprint (sprint only, vanilla C0B rules), 1.7 Animations, Hitboxes, Block
      Overlay, Custom Crosshair, Custom Hit Color, TNT Timer, Snaplook (vanilla F5 only).
- [ ] Visuals and QoL: Fullbright, Zoom and Smooth Zoom, FOV & Hurt Cam, Time & Weather
      Changer, Glint Colorizer, Nick Hider, Particle Control, Low Fire, No Vignette, Clear
      Water, No Pumpkin Blur, Low-HP Damage Tint, Item Physics, Enhanced PvP Chat, Advanced
      Tooltips.

### 4.4 Unified launcher and crash safe mode
- [ ] One live config and profile schema shared by the launcher and Right Shift; no H
      monogram or flat grey cards; Vanilla Menus toggle clarified.
- [ ] One saved server list and Direct Connect across the launcher and reconnect flow.
- [ ] Panic and Metal-failure capture, PII-scrubbed crash log, Relaunch in Safe Mode.

## Phase 5 (stretch, only after phases 1-4 pass)

- [ ] 5.1 MetalFX spatial upscaling in the render scale; opt-in dynamic subrectangle floor.
- [ ] 5.2 Metal 4 path, runtime-detected on macOS 26+.
- [ ] 5.3 PGO build script trained on the packet-trace benchmark.
- [ ] 5.4 Multi-account switcher (Keychain, no token logging) and launcher screenshot gallery.
- [ ] 5.5 Production `Bob Client.app`, stable signing, SHA-256 verified updates with rollback.
