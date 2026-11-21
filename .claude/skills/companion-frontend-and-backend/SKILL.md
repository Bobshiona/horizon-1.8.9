---
name: companion-frontend-and-backend
description: Build the 1.8.9 client's companion surfaces - the in-app launcher and updater today, and any future website, cosmetics, leaderboards or update service - with an Apple-grade minimal design and strict auth, token, contract and updater security.
---

# Companion frontend and backend

## When to Activate
- Work on the launcher screen, self-updater, sign-in, or any planned web or backend service.

## Repo Anchors
- Launcher screen: `crates/horizon-189-client/src/launcher.rs` (account, servers, Play).
- Self-update and install: `src/updater.rs` (`Source`, `update`, `install_program`, `install`);
  app data lives in `~/Library/Application Support/Horizon 1.8.9/`.
- Sign-in: `crates/horizon-auth/` (device code, refresh, Keychain via `saved.rs`);
  `src/options.rs` (`client_id`, `remember`).
- Design primitives: `src/glass.rs`, `src/menu.rs` (`button`, `switch`, `ease`, `fade`),
  `src/sysfont.rs`. No web or backend service exists in this repo yet.

## Core Rules
Frontend:
- Minimal, deliberate layouts: one glass card per task, San Francisco text, vanilla panorama
  backdrops, motion under 250 ms with ease-out; no clutter or placeholder copy.
- Verify interactive flows with a capture (`--smoke-launcher`) and unit tests on geometry and input.
- A website, if built, follows the same visual language and is checked in a browser.

Backend and security:
- Tokens never reach logs, files in the repo, URLs or error text; refresh tokens stay in the
  Keychain. Print fixed markers only.
- The updater only accepts plain `https://` git sources without option-like or shell characters,
  builds with `--locked`, and swaps the program by rename so no old copy remains.
- Future services: parameterised queries, versioned migrations, rate limits, explicit JSON or
  WebSocket contracts with tests on both sides, signed update manifests.
- Nothing is published without the owner's decision; updates currently build on his Mac.

## Verification Checklist
- [ ] Auth and updater changes reviewed by `code-reviewer` for token and command-injection risks.
- [ ] Contracts and parsers have tests for malformed input.
- [ ] Capture checked; full gate passes before push.
