# Lean Workbench Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Start the Tauri-based Hematite workbench refresh by making agent workflows a first-class utility layer and moving diagnostics into a dedicated Problems surface.

**Architecture:** Keep the existing Solid/Tauri/CodeMirror stack. Make the smallest UI/data changes needed to clarify the workbench information architecture: `Agents` instead of `Chat`, a new `Problems` utility tab, and Outline focused on symbols/context.

**Tech Stack:** SolidJS, TypeScript, CSS, Tauri command surface unchanged.

---

## Files

- Modify: `src/App.tsx`
- Modify: `src/App.css`
- Modify: `.gitignore`

## Task 1: Utility Tab Contract

- [x] **Step 1: Update utility tab type**

Add `problems` to the `UtilityTab` union and keep `chat` as the internal state key for compatibility during the first pass.

- [x] **Step 2: Rename visible Chat surface to Agents**

Change labels and titles from `Chat` / `Agent Chat` to `Agents` / `Agent Workflows` while preserving the current chat implementation.

- [x] **Step 3: Add Problems tab to activity rail and utility tab strip**

Add `Problems` as a separate selection that uses the active file diagnostics.

- [x] **Step 4: Move diagnostics rendering out of Outline**

Render diagnostics in the new Problems tab. Keep Outline for symbols and compact context.

- [x] **Step 5: Adjust CSS grid density**

Update `.utility-tabs` from four columns to five columns so the new tab fits without wrapping.

## Task 2: Verification

- [ ] **Step 1: Run frontend build**

Blocked in-session after implementation: escalated shell calls were rejected by the app usage limit, and sandboxed Windows shell execution failed with `CreateProcessAsUserW failed: 1920`.

Run:

```bash
rtk npm run build
```

Expected: PASS.

- [ ] **Step 2: Run relevant Rust tests**

Run:

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: PASS for the existing Rust command/helper surface.

- [ ] **Step 3: Check git status**

Run:

```bash
rtk git status --short --untracked-files=all
```

Expected: only intended workbench files plus pre-existing unrelated changes remain.
