---
name: metal-renderer-and-pvp-hud
description: Work on the 1.8.9 client's wgpu/Metal renderer (terrain meshing, culling, entities, particles, shaders) and its PvP HUD and glass interface while keeping frames cheap - measured with --bench before and after, no per-frame allocation creep.
---

# Metal renderer and PvP HUD

## When to Activate
- Edits to rendering, shaders, meshing, entity models, particles, the HUD, mod cards, menus, the
  launcher screen, fonts or glass.
- Any performance claim.

## Repo Anchors
- Device, surface, passes and frame order: `crates/horizon-189-client/src/renderer.rs`
  (`render`: scene, labels, hand, then glass and interface parts 0 and 1).
- Terrain: `src/terrain.rs` (`TerrainVertex` 16 bytes, `mesh_column`, `visibility` VisGraph),
  `src/scene.rs` (`prepare` culling and occlusion walk, `draw`, shared quad indices), `src/scene.wgsl`.
- Mesher threads: `src/network.rs` (`mesh_columns`, `Dispatch`).
- Entities and items: `src/models.rs`, `src/mob_models.rs`, `src/hand.rs`, `src/particles.rs`.
- Interface: `src/ui.rs` (`Canvas`, atlas layers, `system_text`), `src/ui.wgsl`, `src/sysfont.rs`,
  `src/glass.rs` + `glass.wgsl`, `src/hud.rs`, `src/hud_mods.rs`, `src/menu.rs`, `src/pause.rs`,
  `src/launcher.rs`, `src/outline.rs`.
- Settings persistence: `src/settings.rs`.

## Core Rules
- Hot paths reuse buffers (`entity_vertices`, label lists, `frame`); grow GPU buffers by powers of
  two; no new per-frame `Vec` or `HashMap` without a measurement.
- Terrain: column-local packed corners, solid before cutout (no `discard` in the solid shader),
  translucent far to near, per-section frustum and VisGraph occlusion. Keep reversed-Z
  (`GreaterEqual`, clear 0).
- Interface text that should be crisp uses `Canvas::system_text` (rasterised at the scaled pixel
  size); vanilla screens keep the Minecraft font. Interface colours are gamma-space.
- Glass panels go before the overlay split for HUD cards and after `canvas.overlay()` for sheets;
  fades use `Panel::opacity`.
- Mods: Hypixel-allowed only; vanilla look by default; every option persisted in `settings.json`.
- macOS shows a windowed app at the display's refresh rate: judge frame cost and latency, not
  counter FPS.

## Verification Checklist
- [ ] Baseline vs change with `--smoke-frames N --bench` (prints `BENCH_CPU_US`, `BENCH_GPU_US`,
      `BENCH_FPS`), alternating builds, several runs, medians reported.
- [ ] A `--capture PNG` screenshot checked (`--smoke-menu`, `--smoke-settings`, `--smoke-pitch`,
      `--smoke-inventory`, `--smoke-launcher` set up scenes).
- [ ] Draw calls and uploaded bytes considered for crowds (`set_entities`) and terrain.
- [ ] Tests for geometry and layout helpers; full gate from `protocol-47-and-189-parity`.
