# Hematite Architecture

## Goals

1. Deliver a cross-platform editor UI via Flutter.
2. Keep backend performance-critical services in Rust.
3. Build Hematite's own ecosystem for language tooling and AI-assisted workflows.
4. Ship native installable artifacts for Linux (`.deb`), macOS (`.dmg`), and Windows (`.exe`/`.msi`) via Releases.

## High-level design

```text
┌──────────────────────────┐       JSON-RPC/WebSocket      ┌──────────────────────────┐
│ Flutter Frontend         │ <────────────────────────────> │ Rust Backend             │
│ - Editor surface         │                                │ - Workspace service      │
│ - Panels / explorer      │                                │ - Extension registry     │
│ - Commands / keymap      │                                │ - Event bus              │
└──────────────────────────┘                                └──────────────────────────┘
              │                                                           │
              │                                                           │
              ▼                                                           ▼
      Desktop editor widgets                                   Language services / Agent runtime
      (incremental upgrades)                                   adapters (roadmap)
```

## Backend modules

- `protocol.rs`: JSON-RPC request/response contracts.
- `workspace.rs`: in-memory workspace file model.
- `extensions.rs`: extension metadata/install registry.
- `main.rs`: WebSocket server, dispatch loop, event fan-out.

## Ecosystem roadmap

### Phase 1 (present)

- Workspace open/read/save operations.
- Extension metadata install/list workflow.
- Capability declaration endpoint.
- Baseline JSON-RPC transport between UI and backend.

### Phase 2

- Language tooling integration for Python, Rust, C/C++, CUDA, and Dart/Flutter.
- Command registry + UI contribution hydration.
- Packaging pipeline for `.deb`, `.dmg`, and Windows installer artifacts.

### Phase 3

- Sandboxed plugin/agent runtime for Hematite ecosystem packages.
- AI agent orchestration for Codex CLI, Gemini CLI, and Claude Code.
- IPC bridge between runtime services and Flutter UI.

### Phase 4

- Full LSP client orchestration in Rust.
- DAP session lifecycle and debug UI.
- Lightweight theme/tokenization system and settings sync.
