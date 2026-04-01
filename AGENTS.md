# AGENTS.md

# Hematite

Hematite is a cross-platform IDE designed around a **Flutter frontend** and a **Rust backend**, aiming for:

- **very high performance**
- **very low idle power usage**
- **lightweight memory footprint**
- **broad VS Code extension compatibility**
- **stable operation across macOS, Windows, and Linux**

The project philosophy is simple:

> Do not build a heavier VS Code clone.  
> Build a faster, leaner, lower-power IDE that preserves the workflows developers already expect.

This document defines how coding agents should operate in this repository.

---

# 1. Core product intent

Hematite is intended to be:

- a **desktop-first IDE**
- visually modern, but not graphically wasteful
- strongly optimized for responsiveness, startup time, file operations, indexing throughput, and long-session battery behavior
- compatible with a substantial subset of the VS Code ecosystem, especially:
  - AI coding agents
  - language support extensions
  - LSP-based tooling
  - themes and productivity integrations where feasible

Hematite is **not** intended to:

- blindly reproduce all Electron-era architecture decisions
- prioritize flashy UI over speed, clarity, and thermal efficiency
- accumulate abstraction layers that harm latency or memory usage
- claim full VS Code compatibility unless that behavior is actually implemented and tested

Agents must preserve this product direction.

---

# 2. Technology assumptions

Primary stack:

- **Frontend:** Flutter
- **Backend/core services:** Rust
- **Desktop targets:** macOS, Windows, Linux

Expected architectural split:

- Flutter handles:
  - window UI
  - layout
  - panels
  - settings UI
  - editor shell integration
  - command palette UI
  - extension management UI
  - workspace views

- Rust handles:
  - file system operations
  - indexing
  - search
  - Git-heavy operations where performance matters
  - terminal/PTY integration
  - process orchestration
  - language service coordination
  - extension host/runtime bridging
  - IPC and resource management
  - performance-critical business logic

Agents should avoid pushing performance-sensitive core logic into Dart unless there is a strong reason.

---

# 3. Architectural principles

## 3.1 Performance-first by default

Every meaningful change should be evaluated with these questions:

- Does it increase startup cost?
- Does it increase idle CPU usage?
- Does it increase RAM floor?
- Does it add unnecessary redraws or re-layouts?
- Does it increase IPC chatter between Flutter and Rust?
- Does it introduce blocking behavior on the UI thread?
- Does it worsen laptop battery life during long sessions?

If the answer is yes, the agent should either:
- avoid the design,
- reduce its cost,
- or document the tradeoff explicitly.

---

## 3.2 Low-power operation is a feature, not an afterthought

Hematite should feel efficient not only under load, but also while idle.

Agents should prefer designs that:
- minimize wakeups
- batch background work
- debounce file and UI events
- avoid polling when event-driven mechanisms exist
- suspend or degrade non-essential background tasks when inactive
- avoid needless animations or continuous repaints

Idle efficiency matters.

---

## 3.3 Rust owns the heavy lifting

Rust is the source of truth for:
- high-throughput operations
- long-lived services
- concurrency-sensitive systems
- file watching
- workspace state engines
- extension/runtime orchestration
- terminal and process management

Flutter should not be turned into the de facto backend.

---

## 3.4 Flutter should remain clean and responsive

Flutter exists to provide:
- a polished desktop UI
- deterministic and maintainable layout
- low-jank interactions
- consistent multi-platform presentation

Agents should:
- keep widget trees understandable
- avoid deeply tangled state flows
- avoid unnecessary rebuilds
- use async boundaries carefully
- isolate expensive computations from the UI isolate

---

## 3.5 Compatibility claims must be precise

Hematite may aim for **broad VS Code extension compatibility**, but agents must not overstate support.

Use these distinctions clearly:

- **Supported**
- **Partially supported**
- **Experimental**
- **Not supported**

Never describe a feature as compatible merely because it is planned.

---

# 4. Extension compatibility policy

## 4.1 Compatibility goal

The goal is to support **most practically important VS Code extensions**, especially:

- LSP-backed language tooling
- AI coding assistants / agent integrations
- themes and UI customizations that map cleanly
- formatter / lint / code action workflows
- Git-related productivity tools
- common developer conveniences

## 4.2 Non-goal

Full compatibility with every extension ever published is not required.

Agents should assume some extensions may remain difficult or out of scope, especially those that depend on:
- Electron-specific assumptions
- undocumented VS Code internals
- tightly coupled workbench APIs
- Node-specific process models that cannot be bridged cleanly
- embedded webviews with heavy custom assumptions
- platform-specific native binaries without a stable portability path

## 4.3 Compatibility work must be categorized

When implementing extension compatibility, agents should identify which bucket the work belongs to:

- UI/workbench compatibility
- extension host/runtime compatibility
- VS Code API surface emulation
- Node/process compatibility
- webview compatibility
- filesystem/workspace semantics
- command and keybinding integration
- settings/configuration mapping

Do not mix all of these into one vague “extension support” task.

---

# 5. Cross-platform policy

Hematite targets:

- macOS
- Windows
- Linux

Agents must treat cross-platform support as a first-class engineering constraint.

## 5.1 Avoid platform tunnel vision

Do not implement a solution that only works well on one OS unless:
- it is guarded properly,
- alternatives are documented,
- and the limitation is explicit.

## 5.2 Prefer shared abstractions, not fake uniformity

Cross-platform support should come from:
- a stable Rust core
- explicit platform adapters
- clear capability boundaries

Do not pretend all platforms behave identically if they do not.

## 5.3 Linux is not optional

Linux support must be treated seriously, not as an afterthought.

Agents must be careful with:
- filesystem watching
- terminal behavior
- permissions
- desktop integration
- packaging assumptions
- input method behavior
- font/rendering differences

---

# 6. Performance rules for agents

When contributing code, agents should follow these rules.

## 6.1 Avoid wasteful frontend patterns

Avoid:
- rebuild-heavy widget structures
- global mutable UI state without discipline
- frequent large list re-rendering
- unnecessary animation controllers
- synchronous file or compute work in Dart
- chatty backend calls on every keystroke unless justified

Prefer:
- memoization where sensible
- virtualization for large views
- throttling/debouncing for event floods
- incremental updates instead of full refreshes
- batched backend responses

## 6.2 Avoid wasteful backend patterns

Avoid:
- unbounded task spawning
- coarse locking where finer concurrency is possible
- excessive cloning of large structures
- polling loops without strong justification
- duplicate indexing or parsing work
- loading entire large workspaces into memory when streaming/incremental approaches are possible

Prefer:
- bounded concurrency
- incremental pipelines
- backpressure-aware systems
- careful memory ownership
- profiling-driven optimization

## 6.3 Do not optimize blindly

Performance matters, but speculative complexity is not free.

Agents should:
- keep designs simple first
- profile second
- optimize third

However, agents should still avoid obviously inefficient designs from the start.

---

# 7. UI/UX philosophy

Hematite should feel:
- fast
- precise
- focused
- modern
- restrained

It should not feel:
- bloated
- toy-like
- over-animated
- visually noisy
- mobile-styled in a way that harms desktop productivity

Agents should preserve a **desktop IDE feel**.

## 7.1 UI design guidance

Prefer:
- dense but readable layouts
- strong keyboard accessibility
- low-latency command execution
- clear information hierarchy
- predictable sidebars, tabs, panels, and status areas

Avoid:
- oversized touch-first spacing unless justified
- decorative motion that consumes power
- visual churn during routine editing workflows
- novelty over usability

---

# 8. Repository behavior expectations for agents

## 8.1 Respect existing architecture

Before introducing a new subsystem, agents must check:

- Does this overlap with an existing service?
- Can the current abstraction be extended instead?
- Is a new dependency really necessary?
- Does this duplicate Flutter-side and Rust-side logic?

## 8.2 Keep boundaries explicit

Agents should maintain clear boundaries between:

- UI concerns
- core application state
- IPC contracts
- backend services
- extension runtime logic
- platform adapters

Do not let convenience erode architecture.

## 8.3 Keep interfaces narrow

For Flutter↔Rust communication:
- prefer stable, versionable contracts
- avoid overexposing backend internals
- return structured results
- expose explicit error cases
- avoid fragile ad hoc string protocols

---

# 9. Code quality policy

## 9.1 General

Agents must produce code that is:
- readable
- maintainable
- explicit
- reasonably documented
- consistent with surrounding style

## 9.2 Dart/Flutter

Prefer:
- clear widget composition
- limited rebuild scope
- typed models
- explicit async handling
- separation of view, state, and service concerns

Avoid:
- giant stateful widgets
- hidden side effects in build methods
- business logic embedded deeply in UI code

## 9.3 Rust

Prefer:
- clear module boundaries
- explicit ownership semantics
- strong typing
- robust error handling
- measured concurrency

Avoid:
- panic-prone control flow
- global mutable state without strict need
- opaque macro-heavy designs when simpler code would do
- needless unsafe code

Any `unsafe` must be justified and minimized.

---

# 10. Dependency policy

Agents should be conservative with dependencies.

Before adding one, ask:

- Does the standard library or existing dependency already solve this?
- Is the dependency mature?
- Is it maintained?
- Is it cross-platform?
- Does it add startup, binary size, or memory costs?
- Does it create future lock-in?

For frontend dependencies especially, avoid importing large ecosystems for tiny features.

For backend dependencies, prioritize:
- reliability
- portability
- performance
- maintainability

---

# 11. AI and agent integration policy

Hematite should support AI-assisted development, including integrations such as:
- OpenAI Codex-style flows
- Claude Code-style flows
- agentic coding workflows
- code transformation pipelines
- conversational code actions

Agents implementing AI features should prioritize:

- responsiveness
- cancellation support
- bounded resource use
- privacy clarity
- transparent status reporting
- minimal disruption to normal editing

AI integrations must not degrade the base IDE experience for users who do not actively use them.

---

# 12. Testing expectations

Agents should add or update tests when practical, especially for:

- IPC contracts
- extension compatibility behavior
- workspace operations
- indexing/search logic
- state transitions
- platform-sensitive backend logic

Where full automated coverage is difficult, agents should at least provide:
- clear manual validation steps
- edge cases considered
- known limitations

Performance-sensitive changes should include at least a brief note about expected performance impact.

---

# 13. Benchmark and profiling mindset

When working on hot paths, agents should think in terms of:

- startup latency
- idle CPU
- memory floor
- file tree load time
- global search latency
- indexing throughput
- command palette responsiveness
- editor interaction latency
- terminal responsiveness
- extension activation overhead
- battery impact during long sessions

Do not optimize vanity metrics at the expense of perceived responsiveness.

---

# 14. Naming and communication rules

Agents should describe work precisely.

Say:
- “partial VS Code API coverage for X”
- “experimental extension host bridge”
- “reduced idle CPU in workspace watcher”
- “incremental search indexing”

Do not say:
- “full compatibility” without proof
- “zero overhead”
- “instant”
- “blazing fast” without evidence

Keep claims engineering-grade.

---

# 15. Decision heuristic

When multiple designs are possible, prefer the one that best satisfies this order:

1. correctness
2. responsiveness
3. low idle power
4. maintainability
5. cross-platform robustness
6. extension compatibility
7. feature breadth
8. implementation cleverness

Cleverness is the lowest priority.

---

# 16. What agents should optimize for

If you are an agent making changes in this repository, optimize for this outcome:

> Hematite should feel like a serious desktop IDE that starts quickly, stays cool, uses little power, responds instantly, and supports the workflows developers actually care about.

If a proposed change harms that outcome, reconsider it.

---

# 17. Summary

Hematite is a:

- Flutter + Rust desktop IDE
- performance-first project
- low-power project
- extension-aware project
- cross-platform engineering project

Agents working here must preserve these values:

- **speed**
- **efficiency**
- **clarity**
- **precision**
- **honesty about compatibility**
- **desktop-grade usability**

Build the lean IDE people wanted when they got tired of unnecessary overhead.