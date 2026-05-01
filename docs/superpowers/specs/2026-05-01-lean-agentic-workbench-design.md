# Lean Agentic Workbench Design

Date: 2026-05-01

## Product Direction

Hematite remains an editor-first, IDE-grade desktop application. It should be capable of replacing traditional editors and IDEs such as VS Code, Zed, Xcode, JetBrains IDEs, and Eclipse for day-to-day development while also covering LLM-era agentic workflows well enough that a separate Cursor-style environment is not required.

The product must not become AI-chat-first. Agents are first-class productivity tools, but the primary surface is still editing, navigation, search, diagnostics, language tooling, Git, terminal, and project understanding.

## Non-Negotiable Performance Constraint

The current Tauri version has measured dramatically better idle behavior than the Slint prototype:

- Current Tauri Hematite: approximately 0% idle CPU and 4.5 MB RAM in the user's measurement.
- Slint prototype: approximately 0.1% idle CPU and 34.4 MB RAM in the user's measurement.

Hematite should optimize for measured overhead, not framework theory. Tauri remains the default frontend. Slint stays experimental unless future measurements prove otherwise.

## Slint Resource Finding

The Slint prototype is likely heavier because its default desktop stack pulls a native windowing and rendering stack into the Hematite process:

- `slint` default features include `backend-default`, `renderer-femtovg`, `renderer-software`, `std`, and `accessibility`.
- The app brings in `i-slint-backend-winit`, `glutin`, `femtovg`, software rendering, font/text shaping, accessibility, clipboard, image, and windowing support.
- Both `renderer-femtovg` and `renderer-software` are present through default features.
- Tauri delegates much of its webview cost to OS WebView2 infrastructure on Windows, so the main app process can remain very small. Any future comparison must measure the full process tree, but the user's current measurements are sufficient to keep Tauri as the performance baseline.

The practical conclusion is to invest in the existing Tauri/Solid/CodeMirror workbench and avoid a Slint rewrite for now.

## Workbench Principle

The default screen should feel like a serious desktop IDE:

- immediate file editing
- dense but readable explorer
- fast tab and breadcrumb navigation
- command palette as the primary command/search entry point
- Problems, Outline, Search, Git, Terminal, Tools, and Agents as workbench surfaces
- no dashboard-first home page
- no always-running agent background work
- no decorative or animated UI that costs idle power

The interface should feel modern and precise, but restrained. The goal is not a marketing-style app; it is a durable tool surface for long sessions.

## Agentic Workflow Positioning

Agents are a utility layer, not the center of gravity.

Agent features should include:

- command-palette entry points
- selected text/file/workspace context handoff
- diff preview
- approval, cancel, retry, and resume controls
- clear model/agent selection
- compact context visibility
- status streaming inside a utility panel

Agents should not:

- occupy the default first viewport
- poll or watch in the background while idle
- degrade the base editor when unused
- replace regular IDE functions such as Problems, Search, Git, or Terminal

## Language Support Goals

Python, Rust, and C/C++ are the first-class language targets for the next workbench phase.

Python:

- Ruff diagnostics, formatting, fixes, organize imports
- ty diagnostics, hover, semantic tokens where available
- uv environment management and missing import installation

Rust:

- rust-analyzer hover and semantic tokens
- cargo check, clippy, build, test, doc, metadata
- rustfmt
- active-file diagnostic filtering

C/C++:

- language identification
- tree-sitter outline
- C/C++/CUDA file handling
- next milestone: clangd availability detection, diagnostics, hover, and compile database awareness

## Phase 1 Scope

The first implementation phase should improve usability without replacing the current architecture:

1. Rename the current `Chat` utility surface to `Agents` to reflect that agentic workflows are broader than chat.
2. Add a dedicated `Problems` utility tab so diagnostics are not buried inside Outline.
3. Keep Outline focused on symbols and compact context.
4. Adjust utility tab density to fit `Problems`, `Outline`, `Search`, `Git`, `Agents`, and `Tools` over time, starting with `Problems`, `Agents`, `Access`, `Project`, and `Outline`.
5. Add workbench copy that is precise about idle behavior and agent positioning.
6. Preserve the current CodeMirror editor and Tauri backend command surface.

## Later Phases

Phase 2:

- command palette and quick file/symbol search
- Problems panel across open files
- Search panel with backend-backed workspace search
- Git status and diff preview

Phase 3:

- clangd detection and C/C++ diagnostics
- unified LSP session management across Python, Rust, C/C++
- language server status and restart controls

Phase 4:

- agent diff preview and approval UX
- command-palette agent actions
- workspace-context controls with bounded token/cost reporting

## Testing

Phase 1 should include frontend tests for pure UI helpers where possible and backend tests when command contracts change. For the initial tab/copy restructuring, verification is:

- TypeScript/Vite build passes.
- Existing Tauri Rust tests still pass where touched behavior is relevant.
- Manual browser/app inspection confirms the utility panel shows `Problems` separately from `Outline`.

## Approved Direction

Proceed with a Tauri-based lean workbench: editor-first, language-tooling-heavy, agent-capable, and strict about preserving near-zero idle overhead.
