# Hematite

Hematite is a lightweight desktop IDE built with a Rust backend and a SolidJS frontend on top of Tauri.

The current app focuses on:

- fast native startup and low overhead
- a CodeMirror-based editor with Python-first semantic highlighting
- integrated agent workflows for Codex, Gemini CLI, and Claude Code
- tree-sitter powered outline and compact context generation
- Python environment and dependency management with `uv`

## Stack

- `src-tauri/` - Rust backend and native desktop packaging
- `src/` - SolidJS frontend
- `CodeMirror` - editor runtime
- `tree-sitter` - symbols, context compaction, Python semantic analysis
- `astral-uv` - Python venv and package management

## Current capabilities

- workspace browsing and multi-tab editing
- agent chat and access management
- internal terminal panel
- Python missing-import scan with batch install through `uv`
- semantic hover and token coloring for Python
- Windows packaging to `.msi` and `.exe`

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
