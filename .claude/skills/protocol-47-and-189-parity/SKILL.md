---
name: protocol-47-and-189-parity
description: Keep the native client exactly 1.8.9 / Protocol 47 - packet IDs and field order, window and hotbar sync, 20 TPS movement, combat, knockback, respawn and dimension handling - verified against the owned 1.8.9 jar's bytecode, never modern Minecraft or Fabric assumptions.
---

# Protocol 47 and 1.8.9 parity

## When to Activate
- Any change to packets, the network actor, gameplay, inventory, physics, raycasting or entities.
- A report like "X behaves differently from Lunar/vanilla 1.8.9" or a server kick/flag.

## Repo Anchors
- Packets: `crates/horizon-protocol-47/src/` (`play.rs` decode/encode, `login.rs`, `interaction.rs`,
  `entities.rs`, `player.rs`, `nbt.rs`, `packet.rs` framing, `cipher.rs`).
- Client actor and shared state: `crates/horizon-189-client/src/network.rs` (`run`, `Shared`, the
  per-tick block), tests in `network_tests.rs`.
- Gameplay (`PlayerControllerMP`, digging, use, swing): `src/gameplay.rs`; windows and slots:
  `src/inventory.rs`, `src/container_input.rs`.
- Movement and collision: `crates/horizon-physics-189/src/player.rs`, `shapes.rs`, `raycast.rs`.
- World and entities: `crates/horizon-world-189/src/`.
- Reference harness: `tools/test_189_reference.py` (managed vanilla server, prints
  `VANILLA_189_REFERENCE_GATE_PASS`).

## Core Rules
- Vanilla 1.8.9 is the specification. Read it from the owned jar
  (`~/Library/Application Support/minecraft/versions/1.8.9/1.8.9.jar`): find the class by a unique
  string, disassemble with Java 8 `javap -c -p` in a temp dir, cite the method in a code comment.
  Known names: bew EntityPlayerSP, ave Minecraft, bda PlayerControllerMP, bcy NetHandlerPlayClient,
  ayl GuiContainer, xi Container, pr EntityLivingBase, wn EntityPlayer, bjh RenderItem.
- Never bring in 1.9+ behaviour (attack cooldown, offhand, block states by name, 1.13 flattening)
  or Fabric/Forge/Mojang-mapped Java assumptions.
- Packet layouts: confirm ID and field order and widths (VarInt vs int, float vs double, signed
  bytes, angles in 1/256 turns, fixed-point 1/32 positions) before writing a decoder or encoder.
- Windows: keep action numbers, the cursor stack and C0E/C0F confirmation semantics; a hotbar change
  is C09 sent by `sync_slot` at the start of a tick, as `syncCurrentPlayItem` does.
- Movement runs at 20 TPS with vanilla float/double widths and order; report position with the
  vanilla C03-C06 choice and counters.
- Write the failing regression test first, then the fix.
- Live Hypixel testing and the saved Microsoft login are the owner's; use local servers.

## Verification Checklist
- [ ] Regression test added, citing the vanilla method.
- [ ] `cargo test --offline -p horizon-189-client -p horizon-protocol-47 -p horizon-world-189 -p horizon-physics-189 --target aarch64-apple-darwin`
- [ ] `cargo clippy --offline <same crates> -p horizon-auth -p horizon-assets-189 -p horizon-client-platform --all-targets --target aarch64-apple-darwin -- -D warnings` and `cargo fmt --check`
- [ ] `python3 tools/check_layers.py --base 4e60c05893` and `python3 tools/check_public_tree.py`
- [ ] Diff has no local paths, usernames, tokens, jars, assets or captures.
- [ ] `python3 tools/test_189_reference.py` ends with `VANILLA_189_REFERENCE_GATE_PASS`.
- [ ] HANDOFF.md section added; commit with the no-reply identity, no agent co-author line.
