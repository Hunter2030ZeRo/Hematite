# Slint Frontend Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a parallel Slint frontend MVP for Hematite without disturbing the current Tauri/SolidJS app.

**Architecture:** Create a pure Rust `hematite-ui-state` crate for testable shell/document state, then build a `src-slint` Slint binary that renders that state and wires simple callbacks. The first UI uses static sample workspace data and a simple text editor surface; file IO and full editor parity come after this MVP proves the Slint path builds cleanly.

**Tech Stack:** Rust 2021, Slint, `slint-build`, standard-library Rust tests.

---

## File Structure

- Create `crates/hematite-ui-state/Cargo.toml`: dependency-free state crate.
- Create `crates/hematite-ui-state/src/lib.rs`: shell state, document state, utility tab transitions, and tests.
- Create `src-slint/Cargo.toml`: Slint desktop app crate.
- Create `src-slint/build.rs`: Slint UI compiler hook.
- Create `src-slint/src/main.rs`: Rust entry point and Slint callback wiring.
- Create `src-slint/ui/main.slint`: desktop IDE shell layout.
- Modify no existing Tauri/SolidJS files in the first pass.

## Task 1: Pure UI State Crate

**Files:**
- Create: `crates/hematite-ui-state/Cargo.toml`
- Create: `crates/hematite-ui-state/src/lib.rs`

- [x] **Step 1: Write the failing state tests**

Create `crates/hematite-ui-state/src/lib.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_starts_with_no_workspace_and_agents_tab() {
        let state = ShellState::default();

        assert_eq!(state.workspace_title(), "No workspace");
        assert_eq!(state.status_message(), "No workspace open.");
        assert_eq!(state.utility_tab(), UtilityTab::Agents);
        assert!(state.open_documents().is_empty());
    }

    #[test]
    fn opening_document_adds_active_tab() {
        let mut state = ShellState::with_sample_workspace();

        let index = state.open_document("src/main.rs", "fn main() {}\n");

        assert_eq!(index, 0);
        assert_eq!(state.active_document().unwrap().title, "main.rs");
        assert_eq!(state.active_document().unwrap().content, "fn main() {}\n");
        assert_eq!(state.tab_labels(), vec!["main.rs"]);
    }

    #[test]
    fn opening_existing_document_reuses_tab() {
        let mut state = ShellState::with_sample_workspace();

        state.open_document("src/main.rs", "first");
        let index = state.open_document("src/main.rs", "second");

        assert_eq!(index, 0);
        assert_eq!(state.open_documents().len(), 1);
        assert_eq!(state.active_document().unwrap().content, "first");
    }

    #[test]
    fn editing_active_document_marks_it_dirty() {
        let mut state = ShellState::with_sample_workspace();
        state.open_document("src/main.rs", "first");

        state.edit_active_document("changed");

        assert!(state.active_document().unwrap().dirty);
        assert_eq!(state.tab_labels(), vec!["main.rs *"]);
    }

    #[test]
    fn saving_active_document_clears_dirty_state() {
        let mut state = ShellState::with_sample_workspace();
        state.open_document("src/main.rs", "first");
        state.edit_active_document("changed");

        let saved = state.save_active_document().unwrap();

        assert_eq!(saved.path, "src/main.rs");
        assert_eq!(saved.content, "changed");
        assert!(!state.active_document().unwrap().dirty);
    }

    #[test]
    fn selecting_utility_tab_updates_state() {
        let mut state = ShellState::default();

        state.select_utility_tab(UtilityTab::Outline);

        assert_eq!(state.utility_tab(), UtilityTab::Outline);
        assert_eq!(state.status_message(), "Utility panel: Outline");
    }
}
```

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test --manifest-path crates/hematite-ui-state/Cargo.toml
```

Expected: FAIL because `ShellState`, `UtilityTab`, and related methods are not implemented.

- [x] **Step 3: Add minimal state implementation**

Add the crate manifest:

```toml
[package]
name = "hematite-ui-state"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
```

Implement the tested API:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UtilityTab {
    Agents,
    Tooling,
    Outline,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenDocument {
    pub path: String,
    pub title: String,
    pub content: String,
    pub dirty: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedDocument {
    pub path: String,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellState {
    workspace_root: Option<String>,
    explorer_entries: Vec<WorkspaceEntry>,
    open_documents: Vec<OpenDocument>,
    active_document: Option<usize>,
    utility_tab: UtilityTab,
    status_message: String,
}
```

- [x] **Step 4: Run tests to verify they pass**

Run:

```bash
rtk cargo test --manifest-path crates/hematite-ui-state/Cargo.toml
```

Expected: PASS.

## Task 2: Slint App Skeleton

**Files:**
- Create: `src-slint/Cargo.toml`
- Create: `src-slint/build.rs`
- Create: `src-slint/ui/main.slint`

- [x] **Step 1: Add Slint app manifest**

Create `src-slint/Cargo.toml`:

```toml
[package]
name = "hematite-slint"
version = "0.1.0"
edition = "2021"
build = "build.rs"

[dependencies]
hematite-ui-state = { path = "../crates/hematite-ui-state" }
slint = "1.16.1"

[build-dependencies]
slint-build = "1.16.1"
```

- [x] **Step 2: Add Slint build hook**

Create `src-slint/build.rs`:

```rust
fn main() {
    println!("cargo:rerun-if-changed=ui/main.slint");
    slint_build::compile("ui/main.slint").expect("compile Slint UI");
}
```

- [x] **Step 3: Add first shell UI**

Create `src-slint/ui/main.slint` with a window, sidebar, editor, utility pane, and status bar. Use only static MVP sections and properties that Rust can update.

- [x] **Step 4: Check Slint app build**

Run:

```bash
rtk cargo check --manifest-path src-slint/Cargo.toml
```

Expected: PASS after Slint dependencies are available.

## Task 3: Wire Slint Callbacks To State

**Files:**
- Create: `src-slint/src/main.rs`

- [x] **Step 1: Add Rust entry point**

Create `src-slint/src/main.rs`:

```rust
use hematite_ui_state::{ShellState, UtilityTab};
use slint::{ModelRc, SharedString, VecModel};
use std::{cell::RefCell, rc::Rc};

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let window = MainWindow::new()?;
    let state = Rc::new(RefCell::new(ShellState::with_sample_workspace()));

    refresh_window(&window, &state.borrow());
    wire_callbacks(&window, state);

    window.run()
}
```

- [x] **Step 2: Add refresh helpers and callbacks**

Implement helpers that convert Rust vectors into Slint models, open a sample document, save the active document, update editor text, and switch utility tabs.

- [x] **Step 3: Check the app again**

Run:

```bash
rtk cargo check --manifest-path src-slint/Cargo.toml
```

Expected: PASS.

## Task 4: Verification

**Files:**
- No new files.

- [x] **Step 1: Run state tests**

Run:

```bash
rtk cargo test --manifest-path crates/hematite-ui-state/Cargo.toml
```

Expected: PASS.

- [x] **Step 2: Run Slint build check**

Run:

```bash
rtk cargo check --manifest-path src-slint/Cargo.toml
```

Expected: PASS, unless dependency download is blocked by sandbox/network policy. If blocked, rerun with approval.

- [x] **Step 3: Inspect git status**

Run:

```bash
rtk git status --short --untracked-files=all
```

Expected: only new Slint/UI-state files plus pre-existing unrelated Tauri changes.
