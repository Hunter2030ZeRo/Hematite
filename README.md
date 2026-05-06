# Hematite

Hematite is a lightweight desktop IDE built with a Rust backend and a SolidJS frontend on top of Tauri.

The current app focuses on:

- fast native startup and low overhead
- a CodeMirror-based editor with Python diagnostics from Ruff and ty
- integrated agent workflows for Codex, Gemini CLI, Claude Code, and Kilo Code CLI
- tree-sitter powered outline and compact context generation
- Python environment and dependency management with `uv`

## Stack

- `src-tauri/` - Rust backend and native desktop packaging
- `src/` - SolidJS frontend
- `CodeMirror` - editor runtime
- `tree-sitter` - symbols and context compaction
- `astral-uv` - Python venv and package management
- `Ruff` and `ty` - bundled Python linting, type diagnostics, semantic tokens, and hover

## Current capabilities

- workspace browsing and multi-tab editing
- agent chat and access management
- internal terminal panel
- Python missing-import scan from ty diagnostics with batch install through `uv`
- Python hover and semantic token coloring through the bundled ty language server
- Windows packaging to `.msi` and `.exe`

## Bundled CLI sidecars

Windows builds include sidecar binaries under `src-tauri/binaries/`:

- `ruff-x86_64-pc-windows-msvc.exe`
- `ty-x86_64-pc-windows-msvc.exe`
- `kilo-x86_64-pc-windows-msvc.exe`

## Development

Install dependencies:

```bash
npm install
```

Run the desktop app in development:

```bash
npm run tauri -- dev
```

Build production bundles:

```bash
npm run tauri -- build
```

## Prerequisites(Optional)

- [Codex CLI](https://github.com/openai/codex)/[Gemini CLI](https://github.com/google-gemini/gemini-cli)/[Claude Code](https://github.com/anthropics/claude-code) - Install in case you wish to use integrated agentic coding feature.
