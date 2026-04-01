# Hematite Architecture

## Goals

1. Deliver a cross-platform editor UI via Flutter (Linux desktop runner committed in-repo).
2. Keep backend performance-critical services in Rust.
3. Support VS Code extensions incrementally via compatible protocols and package formats.

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
      Monaco/Code widgets                                      LSP / DAP / Extension host
      (incremental upgrades)                                   adapters (roadmap)
```

## Backend modules

- `protocol.rs`: JSON-RPC request/response contracts.
- `workspace.rs`: in-memory workspace file model.
- `extensions.rs`: extension metadata/install registry.
- `main.rs`: WebSocket server, dispatch loop, event fan-out.

## Compatibility roadmap

### Phase 1 (present)

- Workspace open/read/save operations.
- Extension metadata install/list workflow.
- Capability declaration endpoint.

### Phase 2

- Open VSX package download and unpack.
- Manifest validation (`package.json` contribution points).
- Command registry + UI contribution hydration.

### Phase 3

- Sandboxed extension host runtime.
- VS Code extension API subset (`commands`, `window`, `workspace`).
- IPC bridge between extension host and Flutter UI.

### Phase 4

- Full LSP client orchestration in Rust.
- DAP session lifecycle and debug UI.
- Theme/tokenization parity and settings sync.

## Packaging

Release packaging scripts live under `packaging/` and generate single-file installers per platform:

- Linux: `.deb` via `packaging/linux/package_deb.sh`
- macOS: `.dmg` via `packaging/macos/package_dmg.sh`
- Windows: `.msi` via `packaging/windows/package_msi.ps1`
