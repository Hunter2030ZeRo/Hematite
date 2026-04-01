# Hematite

Hematite is a full-stack, VS Code-inspired editor/IDE prototype with:

- **Frontend**: Flutter desktop/web UI.
- **Backend**: Rust service exposing JSON-RPC over WebSocket.
- **Compatibility goal**: VS Code-style extension workflows (starting with extension metadata + install lifecycle, then extension host execution).

> This repository now contains a runnable MVP foundation rather than a complete parity implementation of VS Code.

## Project layout

- `frontend/` — Flutter editor UI.
- `backend/` — Rust backend for workspace state + extension registry + RPC.
- `docs/` — Architecture notes and roadmap.

## Quick start

### 1) Start backend

```bash
cd backend
cargo run
```

Backend runs on `127.0.0.1:8989`.

### 2) Start frontend

```bash
cd frontend
flutter pub get
flutter run -d linux
```

(Use your target device in place of `linux`.)

## Current RPC methods

- `workspace/open`
- `workspace/save`
- `workspace/read`
- `extensions/install`
- `extensions/list`
- `capabilities`

## VS Code compatibility strategy

Hematite's backend is deliberately designed around JSON-RPC to align with:

- Language Server Protocol (LSP)
- Debug Adapter Protocol (DAP)
- Extension-host-style command/event exchange

See `docs/architecture.md` for the phased roadmap.
