---
name: agent-delegation-and-model-routing
description: Route work in the Horizon 1.8.9 client by cost - send file search, grep, directory scans, log filtering and cargo check/test summaries to Haiku subagents, docs research and routine tests or review to Sonnet, and keep Opus for architecture, Protocol 47 desyncs, Metal pipeline work, auth and multi-file refactors.
---

# Agent delegation and model routing

## When to Activate
- Before a broad search, a multi-file read, or any command whose output is long (cargo, clippy, the
  reference harness).
- When planning a task that splits into mechanical and judgement parts.

## Repo Anchors
- 1.8.9 crates: `crates/horizon-189-client`, `horizon-protocol-47`, `horizon-world-189`,
  `horizon-physics-189`, `horizon-assets-189`, `horizon-auth`, `horizon-client-platform`.
- Agents in `.claude/agents/`: `repo-scout` (haiku), `build-log-runner` (haiku),
  `docs-researcher` (sonnet), `code-reviewer` (sonnet). Built-in `Explore` also works for sweeps.
- Running log of decisions and measurements: `HANDOFF.md` (numbered sections).

## Core Rules
Routing table:

| Work | Agent / model |
| --- | --- |
| Find files, symbols, packet IDs, call sites; list a module's fns | `repo-scout` (haiku, low) |
| Run `cargo check/test/clippy`, `tools/*.py` checks; summarise failures | `build-log-runner` (haiku, low) |
| Version-sensitive external facts (wiki.vg protocol 47, docs.rs, Apple Metal, Microsoft OAuth) | `docs-researcher` (sonnet, low) |
| Read-only review of a finished diff | `code-reviewer` (sonnet, medium) |
| Routine tests or a small, specified component | Sonnet subagent with an exact file list |
| Architecture, Protocol 47 desyncs, Metal shaders/pipelines, auth/Keychain/updater security, multi-file refactors, bytecode parity calls | main Opus session |

- Prompts to subagents are self-contained: the goal, exact paths, what to return. Ask for file:line,
  signatures and condensed error lines, never whole files or raw logs.
- Don't over-orchestrate: a single known-path edit, a one-line check, or an answer already in context
  is done directly.
- One subagent per independent question; run independent ones in parallel.
- Parity decisions against the 1.8.9 jar stay in the main session: subagents gather, Opus decides.
- Never let a subagent push, commit, open the installed app, or touch the saved Microsoft sign-in.

## Verification Checklist
- [ ] Each delegated prompt names files and a return format.
- [ ] Summaries cite file:line; nothing pasted wholesale into the main context.
- [ ] Final gate still runs in the main session before any push (see `protocol-47-and-189-parity`).
