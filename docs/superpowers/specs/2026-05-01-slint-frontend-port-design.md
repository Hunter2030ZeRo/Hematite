# Slint Frontend Port Design

Date: 2026-05-01

## Decision

Build a parallel Slint frontend for Hematite while keeping the current Tauri/SolidJS app intact as the functional reference. The goal is not to assume Slint will beat the current Tauri build immediately; the current app already demonstrates excellent idle behavior at roughly 0% idle CPU and 4.5 MB RAM. The Slint track is a long-term architecture experiment for a Rust-native UI, simpler frontend/backend boundaries, and lower dependency surface.

Flutter was an early direction and is out of scope for this port.

## Why Slint Still Fits

Tauri remains a valid lightweight baseline for Hematite. It is especially strong because the current implementation can reuse CodeMirror and web tooling while staying unusually efficient. Slint is worth pursuing because it lets Hematite test a native Rust UI path without a WebView, JavaScript runtime, npm dependency tree, or Tauri command bridge as the primary UI contract.

The success criterion is therefore architectural, not only numerical:

- Preserve or improve idle CPU and memory floor.
- Reduce frontend dependency weight over time.
- Keep performance-sensitive logic in Rust.
- Avoid weakening cross-platform desktop behavior.
- Keep compatibility claims precise and measured.

## Migration Shape

The port will be additive first:

- Add a new Slint application crate under `src-slint/`.
- Add a small Rust UI-state library under `crates/hematite-ui-state/`.
- Keep the existing Tauri application untouched during the first pass.
- Use the Tauri app as the behavior reference for shell layout, workspace browsing, editor tabs, agent surfaces, diagnostics, and status reporting.

This avoids a high-risk rewrite and keeps the known-efficient Tauri app available for comparison.

## First MVP Scope

The first Slint MVP should provide a serious IDE shell, not a full CodeMirror replacement:

- desktop window titled Hematite
- activity/sidebar area
- workspace explorer model seeded from a small static MVP list
- tab strip model
- central editor surface using Slint text editing primitives
- right utility area with static MVP sections for agents, tooling, and outline
- bottom status bar
- deterministic UI state transitions covered by Rust tests

The MVP may use Slint `TextEdit` for initial editing. Advanced editor features such as syntax highlighting, diagnostics gutters, semantic tokens, hover cards, minimap behavior, and CodeMirror-level key handling are separate milestones because Slint does not provide a full code editor widget out of the box.

## Architecture

```text
┌────────────────────────────┐
│ Slint UI                   │
│ - layout and widgets       │
│ - user callbacks           │
└──────────────┬─────────────┘
               │ typed Rust calls
┌──────────────▼─────────────┐
│ hematite-ui-state          │
│ - shell state              │
│ - document/tab state       │
│ - testable transitions     │
└──────────────┬─────────────┘
               │ future extraction after MVP parity
┌──────────────▼─────────────┐
│ Hematite core services     │
│ - file operations          │
│ - language tooling         │
│ - agent orchestration      │
│ - terminal/process control │
└────────────────────────────┘
```

The first step deliberately separates pure UI state from Slint generated bindings. This gives the port test coverage without needing GUI automation for every state transition.

## Boundaries

Slint owns:

- visual composition
- low-frequency UI callbacks
- presenting data from Rust models
- simple text entry for MVP editing

Rust core owns:

- file IO
- workspace indexing/search
- terminal and process orchestration
- agent invocation and streaming
- language tooling and diagnostics
- persistence and cross-platform adapters

The Slint UI must not become a second backend. It should call narrow Rust APIs and render structured results.

## Tauri Parity Strategy

Do not delete or replace Tauri yet. The Tauri app is the benchmark and behavior reference.

Milestones:

1. Slint shell compiles and runs.
2. Slint shell state is test-covered.
3. Basic file open/edit/save lands behind Rust APIs.
4. Explorer/tabs/status match the existing workflow shape.
5. Performance measurement compares startup, idle CPU, memory floor, and binary/package size against Tauri.
6. Only after Slint reaches useful parity should Hematite choose whether Tauri remains, becomes legacy, or is removed.

## Performance Guardrails

- No polling loops in the UI.
- No continuous animation in idle state.
- Batch UI model updates.
- Keep file and process work off the UI thread.
- Avoid large cloned strings except at explicit document boundaries.
- Prefer typed Rust structs over ad hoc JSON between Rust layers.
- Measure before claiming Slint is lighter than the current Tauri build.

## Licensing

Hematite can evaluate Slint for a desktop app, but release packaging must honor Slint's license choice. If using Slint's royalty-free desktop/mobile/web license instead of GPLv3, Hematite needs visible Slint attribution, such as an About dialog or project download page attribution.

The MVP should include a small About/attribution surface before any distributed build is treated as release-ready.

## Testing

Initial tests focus on Rust state:

- default shell state
- opening a document creates or reuses a tab
- editing marks a document dirty
- saving clears dirty state
- selecting a utility tab updates shell state

GUI smoke validation comes after the first Slint binary builds.

## Open Risks

- Slint `TextEdit` is not a production code editor.
- Native editor parity may require custom rendering, a third-party editing component, or a dedicated Hematite editor engine.
- Packaging will need a new path independent of Tauri's bundler.
- Current Tauri metrics are already excellent, so Slint must earn replacement status with measurements, not assumptions.

## Approved Direction

Proceed with a parallel Slint port and keep the Tauri app as the reference implementation until measured parity justifies changing the default frontend.
