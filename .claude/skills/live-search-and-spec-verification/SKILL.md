---
name: live-search-and-spec-verification
description: Stop guessed APIs in the Horizon 1.8.9 client - look up local source, dependency source and external specs (Protocol 47, docs.rs crates such as wgpu 30 and winit, Apple Metal, Microsoft/Xbox sign-in) before writing code that depends on them.
---

# Live search and spec verification

## When to Activate
- Before using a struct field, method or enum variant not already seen in this session.
- Before relying on a version-sensitive external fact.

## Repo Anchors
- Workspace `Cargo.toml` and `Cargo.lock` pin versions (wgpu 30, winit, glam, ab_glyph, ureq 2,
  security-framework, serde_json).
- Dependency source is local: `~/.cargo/registry/src/*/<crate>-<version>/`.
- Owned 1.8.9 jar for vanilla behaviour, models, language and textures.
- Auth: `crates/horizon-auth/src/lib.rs` (`begin`, `finish`, `refresh`, `Session`), `saved.rs`.

## Core Rules
- Local first: `repo-scout` for this repo and the cargo registry source; read the real signature.
- External second: `docs-researcher` for wiki.vg Protocol 47 pages, docs.rs at the locked version,
  Apple developer docs, Microsoft identity docs. Ask for a fact sheet with URLs.
- Vanilla behaviour comes from the jar's bytecode, not from wikis or memory.
- Keep outputs summarised; never paste whole files, pages or logs into the main session.
- If sources disagree, prefer the locked crate source and the jar; note the conflict.

## Verification Checklist
- [ ] Every new external API use traced to source or a cited doc.
- [ ] Version matches `Cargo.lock`.
- [ ] Protocol facts cross-checked against the jar when they affect behaviour.
