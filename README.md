# Hematite

Hematite is a full-stack, VS Code-inspired editor/IDE prototype with:

- **Frontend**: Flutter desktop UI (Linux runner included in-repo).
- **Backend**: Rust service exposing JSON-RPC over WebSocket.
- **Compatibility goal**: VS Code-style extension workflows (starting with extension metadata + install lifecycle, then extension host execution).

> This repository contains a runnable MVP foundation rather than full VS Code parity.

## Project layout

- `frontend/` — Flutter editor UI + Linux desktop runner files.
- `backend/` — Rust backend for workspace state + extension registry + RPC.
- `docs/` — Architecture notes and roadmap.

## Linux support

Linux desktop is now first-class in the repo:

- Checked-in `frontend/linux/` runner and CMake files.
- Backend bind address can be configured with `HEMATITE_BACKEND_ADDR` (default `127.0.0.1:8989`).
- Frontend WebSocket backend endpoint can be configured with `--dart-define=HEMATITE_BACKEND_WS=...`.

### Linux prerequisites

- Rust toolchain (`cargo`, `rustc`)
- Flutter SDK with Linux desktop enabled
- GTK3 development headers and CMake/Ninja

On Debian/Ubuntu, typical packages are:

```bash
sudo apt-get install clang cmake ninja-build pkg-config libgtk-3-dev
```

## Quick start (Linux)

### 1) Start backend

```bash
cd backend
cargo run
```

Custom address example:

```bash
HEMATITE_BACKEND_ADDR=0.0.0.0:8989 cargo run
```

### 2) Start frontend

```bash
cd frontend
flutter pub get
flutter run -d linux
```

Custom backend endpoint example:

```bash
flutter run -d linux --dart-define=HEMATITE_BACKEND_WS=ws://127.0.0.1:8989/rpc
```


### Troubleshooting: "No Linux desktop project configured"

If your local checkout still reports:

```
No Linux desktop project configured
```

run this inside `frontend/` once to regenerate missing local desktop tooling files:

```bash
flutter create --platforms=linux .
```

Then run:

```bash
flutter run -d linux
```

## Current RPC methods

- `workspace/open`
- `workspace/save`
- `workspace/read`
- `extensions/install`
- `extensions/list`
- `capabilities`

## VS Code compatibility strategy

Hematite's backend is designed around JSON-RPC to align with:

- Language Server Protocol (LSP)
- Debug Adapter Protocol (DAP)
- Extension-host-style command/event exchange

See `docs/architecture.md` for the phased roadmap.
