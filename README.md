# Hematite

Hematite is a cross-platform text editor/IDE built with a Flutter frontend and a Rust backend, targeting Windows, macOS, and Linux.

Hematite is designed as a native desktop app for each OS, with distribution artifacts for:

- **Linux**: `.deb`
- **macOS**: `.dmg`
- **Windows**: `.exe` / `.msi`

Release builds are intended to be uploaded to the GitHub Releases tab for end-user installation.

## Product direction

Hematite focuses on its own ecosystem. The core direction is:

- **Native UX + low overhead**: fast startup, low idle CPU, and a small memory footprint.
- **Language support**: first-class tooling for Python, Rust, C/C++, CUDA, and Dart/Flutter.
- **AI agent workflows**: Codex, Gemini CLI, and Claude Code integrations.
- **Flutter + Rust split**: responsive Flutter desktop UI with performance-critical services in Rust.

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

See `docs/architecture.md` for the phased roadmap.
