# Code Intelligence and Agent Workflow Design

## Product Goal

Hematite should feel like a capable desktop IDE while staying ultra-lightweight. This design strengthens two weak areas:

- rich language-aware editing
- seamless interactive agent workflows

The change must not turn Hematite into an always-running agent shell or a heavy VS Code clone. The editor remains the primary surface. Agents and language services are modular tools that wake only when useful.

## Non-Negotiable Constraints

- Rust owns language service coordination, process management, indexing, diff generation, and agent orchestration.
- The frontend renders state and captures intent; it does not become the backend.
- No background polling loops for language services or agents.
- Expensive work is explicit, debounced, cached, cancellable, or tied to a user-visible session.
- Every provider advertises precise capabilities instead of pretending full IDE compatibility.
- Each language and each agent provider is an adapter behind a narrow interface.
- Missing tools degrade gracefully with clear status and install guidance.

## Design Option Chosen

Use a shared backend spine for language features and agent sessions.

Rejected alternatives:

- A broad IDE sweep across Search, Git, terminal, language support, and agents at once. This would create high churn and risk feature-shaped panels without strong integration.
- An agent-first rebuild that leaves language intelligence shallow. That would make Hematite feel more like a chat client than an IDE.

The chosen design improves real IDE capability while preserving lightweight modularity.

## Code Intelligence Spine

Introduce a backend language-feature layer with provider adapters.

Each provider reports a `LanguageCapabilityStatus`:

- language id
- provider name
- availability state
- resolved executable path when applicable
- supported features
- inactive or degraded reason
- recommended setup action

Supported feature flags:

- diagnostics
- hover
- outline
- workspace symbols
- document symbols
- semantic tokens
- formatting
- organize imports
- code actions
- references
- rename

Initial provider targets:

- Python: Ruff plus ty, keeping existing diagnostics, hover, semantic tokens, formatting, fixes, and import workflows.
- Rust: rust-analyzer plus Cargo actions for hover, semantic tokens, diagnostics, formatting, tests, docs, and metadata-aware status.
- C/C++/CUDA: clangd-focused availability, compile database awareness, hover, diagnostics, and semantic-token readiness.
- Dart/Flutter: adapter slot and detection first, richer support later.

The frontend should consume one capability model instead of hardcoding separate status copy for each language.

## Code Intelligence UX

The editor should expose language intelligence where developers naturally expect it:

- hover cards from the active provider
- diagnostics in editor gutters and the Problems tab
- semantic coloring when available
- document outline and symbols from the best available provider
- formatting and organize-import actions from toolbar or command palette
- active-file status that explains what is running, inactive, missing, or unsupported

The UI should avoid claiming full LSP parity until each feature exists. For example:

- "Rust hover and diagnostics active"
- "clangd found; compile_commands.json missing"
- "Dart detected; language server adapter not enabled yet"

## Lightweight Operation

Language providers are lazy.

- Opening a file performs cheap language detection and capability lookup.
- Hover requests are demand-driven.
- Diagnostics run on explicit save, explicit check, or debounced active-file changes where already supported.
- Workspace-wide analysis is never implicit on app startup.
- Provider processes are reused while useful and shut down when idle if that is cheaper than keeping them alive.
- Large files keep existing byte limits and return clear degraded states.

## Agent Session Spine

Introduce a backend `AgentSession` model that supports interactive threaded chat.

Each session contains:

- session id
- provider id
- model id
- permission level
- workspace root
- messages
- attached context
- pending approvals
- running or idle state
- cancellation token

Agent providers implement a common adapter interface:

- start session
- send message
- stream events
- request approval
- submit approval response
- cancel turn
- retry turn
- close session

Provider-specific implementation details remain behind the adapter. Codex, Gemini, Claude, and Kilo can differ internally while the UI sees the same session contract.

## Interactive Agent UX

The Agents pane should become a real threaded chat surface:

- select provider, model, and permission level
- type follow-up messages without rebuilding one large prompt manually
- stream assistant output into the current thread
- cancel a running turn
- retry the last turn
- show command, file, and permission approvals as native cards
- preserve recent sessions per workspace within bounded storage

Agents should feel attached to the editor:

- attach active file
- attach selected text
- attach open tabs
- attach current diagnostics
- attach outline or compact workspace context
- attach recent terminal excerpt only when requested

Context attachments should show an approximate size before submission. This keeps agent runs predictable and avoids silently sending too much.

## Agent Edit Workflow

Agent file changes should be inspectable before they land.

The first implementation should support:

- proposed file edits as structured events
- diff preview in the app
- approve, reject, or request revision
- apply approved edits through Rust file APIs
- refresh affected editor tabs after apply

This keeps command approval and file edit approval separate. A shell command approval does not imply write approval.

## Modularity Boundaries

Suggested backend modules:

- `language`: provider registry, capability status, request routing
- `language::python`: Ruff and ty adapter
- `language::rust`: rust-analyzer and Cargo adapter
- `language::c_family`: clangd adapter
- `agents`: session registry and shared events
- `agents::codex`
- `agents::gemini`
- `agents::claude`
- `agents::kilo`
- `diff`: structured file edit and preview generation

Suggested frontend boundaries:

- `CodeEditor`: editor rendering, diagnostics, semantic tokens, hover requests
- `LanguageStatus`: capability and setup status
- `Problems`: diagnostics across open files
- `AgentsPane`: session list, model controls, threaded chat
- `ContextAttachments`: explicit context picker
- `ApprovalCard`: command, file edit, and permission approval UI
- `DiffPreview`: inspect proposed agent edits

## Phase 1 Scope

Phase 1 should be small enough to verify quickly:

1. Define shared language capability status and expose it through Tauri.
2. Adapt existing Python, Rust, and C-family status into the shared model.
3. Add backend `AgentSession` state transitions without changing provider internals yet.
4. Replace prompt-only chat flow with session-based interactive follow-ups for the currently selected provider.
5. Add explicit context attachments for active file, selected text, diagnostics, and open tabs.
6. Add cancel and retry controls for active agent turns.
7. Add tests for language capability reporting and agent session state transitions.

## Later Phases

- Diff preview and apply workflow for agent edits.
- Workspace symbols and references.
- Rename support through provider adapters.
- Command palette integration for language and agent actions.
- Dart/Flutter language provider.
- Persisted multi-session workspace history with strict size caps.
- Unified LSP lifecycle manager if provider-specific adapters become too duplicated.

## Testing

Backend tests should cover:

- capability status when tools are present, missing, or partially configured
- byte-limit and unavailable-provider degradation
- agent session creation, message append, running, cancellation, retry, and close transitions
- approval state transitions

Frontend tests should cover pure helpers where practical:

- capability status labels
- attachment size summaries
- approval card state labels
- retry/cancel button enablement

Manual validation should cover:

- opening Python, Rust, and C/C++ files
- checking that idle CPU stays quiet with no active language or agent work
- starting an agent chat, sending a follow-up, cancelling a turn, and retrying
- attaching context explicitly and confirming the attachment summary before submit

## Performance Notes

The design improves functionality without requiring continuous work while idle.

Expected costs:

- small static capability model in frontend memory
- short backend checks for provider availability
- active provider processes only during language or agent activity
- bounded persisted chat/session history

Expected protections:

- no startup workspace-wide analysis
- no automatic whole-repository context upload
- no hidden agent background work
- byte caps for editor analysis and persisted documents
- explicit user action for expensive checks and agent turns

## Approved Direction

Build Hematite around a lightweight, modular code-intelligence spine and an interactive agent-session spine. The result should make Hematite feel substantially more capable as an IDE while preserving its core promise: fast startup, low idle power, narrow backend contracts, and honest compatibility claims.
