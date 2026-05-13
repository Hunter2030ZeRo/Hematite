# Code Intelligence and Agent Workflow Implementation Plan

Spec: `docs/superpowers/specs/2026-05-13-code-intelligence-agent-workflow-design.md`

Goal: add a lightweight modular spine for language capability reporting and interactive agent sessions without adding idle polling, startup-wide analysis, or always-running agents.

## Constraints

- Keep heavyweight work in Rust.
- Keep providers lazy and demand-driven.
- Keep frontend state as rendering and intent capture only.
- Preserve current dirty worktree changes; do not revert unrelated edits.
- Add tests before production code for backend behavior changes.

## Task 1: Shared Language Capability Model

**Files:**

- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add failing backend tests**

Add tests in the existing `#[cfg(test)]` module in `src-tauri/src/lib.rs`:

```rust
#[test]
fn language_capability_status_reports_missing_tool_without_features() {
    let status = language_capability_status_for_tool(
        SourceLanguage::Rust,
        "rust-analyzer",
        None,
        false,
    );

    assert_eq!(status.language_id, "rust");
    assert_eq!(status.provider_name, "rust-analyzer");
    assert_eq!(status.availability, LanguageProviderAvailability::Missing);
    assert!(status.resolved_path.is_none());
    assert!(status.supported_features.is_empty());
    assert_eq!(
        status.inactive_reason.as_deref(),
        Some("rust-analyzer is not installed on PATH.")
    );
    assert_eq!(
        status.recommended_action.as_deref(),
        Some("Install rust-analyzer or open Hematite from an environment where rust-analyzer is on PATH.")
    );
}

#[test]
fn language_capability_status_reports_active_rust_features() {
    let status = language_capability_status_for_tool(
        SourceLanguage::Rust,
        "rust-analyzer",
        Some(PathBuf::from("/tools/rust-analyzer")),
        true,
    );

    assert_eq!(status.availability, LanguageProviderAvailability::Active);
    assert_eq!(status.resolved_path.as_deref(), Some("/tools/rust-analyzer"));
    assert!(status.supported_features.contains(&LanguageFeature::Diagnostics));
    assert!(status.supported_features.contains(&LanguageFeature::Hover));
    assert!(status.supported_features.contains(&LanguageFeature::SemanticTokens));
    assert!(status.inactive_reason.is_none());
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml language_capability_status
```

Expected: FAIL because `LanguageProviderAvailability`, `LanguageFeature`, and `language_capability_status_for_tool` do not exist yet.

- [ ] **Step 3: Add the shared language types**

Add near the existing editor/language structs in `src-tauri/src/lib.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum LanguageProviderAvailability {
    Active,
    Degraded,
    Missing,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum LanguageFeature {
    Diagnostics,
    Hover,
    Outline,
    WorkspaceSymbols,
    DocumentSymbols,
    SemanticTokens,
    Formatting,
    OrganizeImports,
    CodeActions,
    References,
    Rename,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct LanguageCapabilityStatus {
    language_id: String,
    provider_name: String,
    availability: LanguageProviderAvailability,
    resolved_path: Option<String>,
    supported_features: Vec<LanguageFeature>,
    inactive_reason: Option<String>,
    recommended_action: Option<String>,
}
```

- [ ] **Step 4: Add the minimal status builder**

Add in `src-tauri/src/lib.rs` near language detection helpers:

```rust
fn language_id(language: SourceLanguage) -> &'static str {
    match language {
        SourceLanguage::Python => "python",
        SourceLanguage::Rust => "rust",
        SourceLanguage::C => "c",
        SourceLanguage::Cpp => "cpp",
        SourceLanguage::Cuda => "cuda-cpp",
        SourceLanguage::JavaScript => "javascript",
        SourceLanguage::TypeScript => "typescript",
        SourceLanguage::Tsx => "typescriptreact",
    }
}

fn features_for_language_provider(
    language: SourceLanguage,
    provider_name: &str,
    project_ready: bool,
) -> Vec<LanguageFeature> {
    match (language, provider_name, project_ready) {
        (SourceLanguage::Python, "ruff", true) => vec![
            LanguageFeature::Diagnostics,
            LanguageFeature::Formatting,
            LanguageFeature::OrganizeImports,
            LanguageFeature::CodeActions,
        ],
        (SourceLanguage::Python, "ty", true) => vec![
            LanguageFeature::Diagnostics,
            LanguageFeature::Hover,
            LanguageFeature::SemanticTokens,
        ],
        (SourceLanguage::Rust, "rust-analyzer", true) => vec![
            LanguageFeature::Diagnostics,
            LanguageFeature::Hover,
            LanguageFeature::Outline,
            LanguageFeature::DocumentSymbols,
            LanguageFeature::SemanticTokens,
            LanguageFeature::Formatting,
            LanguageFeature::References,
            LanguageFeature::Rename,
        ],
        (SourceLanguage::C | SourceLanguage::Cpp | SourceLanguage::Cuda, "clangd", true) => vec![
            LanguageFeature::Diagnostics,
            LanguageFeature::Hover,
            LanguageFeature::Outline,
            LanguageFeature::DocumentSymbols,
            LanguageFeature::SemanticTokens,
            LanguageFeature::References,
            LanguageFeature::Rename,
        ],
        _ => Vec::new(),
    }
}

fn missing_tool_action(provider_name: &str) -> String {
    format!(
        "Install {provider_name} or open Hematite from an environment where {provider_name} is on PATH."
    )
}

fn language_capability_status_for_tool(
    language: SourceLanguage,
    provider_name: &str,
    resolved_path: Option<PathBuf>,
    project_ready: bool,
) -> LanguageCapabilityStatus {
    let resolved_path_string = resolved_path
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    let supported_features =
        features_for_language_provider(language, provider_name, project_ready && resolved_path.is_some());

    let availability = if resolved_path.is_none() {
        LanguageProviderAvailability::Missing
    } else if !project_ready {
        LanguageProviderAvailability::Degraded
    } else {
        LanguageProviderAvailability::Active
    };

    let inactive_reason = match availability {
        LanguageProviderAvailability::Missing => {
            Some(format!("{provider_name} is not installed on PATH."))
        }
        LanguageProviderAvailability::Degraded => {
            Some("Project metadata is incomplete for this provider.".to_string())
        }
        LanguageProviderAvailability::Active | LanguageProviderAvailability::Unsupported => None,
    };

    let recommended_action = match availability {
        LanguageProviderAvailability::Missing => Some(missing_tool_action(provider_name)),
        LanguageProviderAvailability::Degraded => {
            Some("Open a configured project or add the metadata this language server expects.".to_string())
        }
        LanguageProviderAvailability::Active | LanguageProviderAvailability::Unsupported => None,
    };

    LanguageCapabilityStatus {
        language_id: language_id(language).to_string(),
        provider_name: provider_name.to_string(),
        availability,
        resolved_path: resolved_path_string,
        supported_features,
        inactive_reason,
        recommended_action,
    }
}
```

- [ ] **Step 5: Run tests and verify pass**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml language_capability_status
```

Expected: PASS.

## Task 2: Expose Language Capabilities Through Tauri

**Files:**

- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add failing command-contract test**

Add this test in `src-tauri/src/lib.rs`:

```rust
#[test]
fn language_capabilities_include_python_rust_and_c_family() {
    let statuses = build_language_capabilities_for_paths(
        ToolPathSnapshot {
            ruff: Some(PathBuf::from("/tools/ruff")),
            ty: Some(PathBuf::from("/tools/ty")),
            rust_analyzer: Some(PathBuf::from("/tools/rust-analyzer")),
            clangd: Some(PathBuf::from("/tools/clangd")),
        },
        ProjectMetadataSnapshot {
            has_pyproject: true,
            has_cargo_toml: true,
            has_compile_commands: false,
        },
    );

    let providers: Vec<_> = statuses
        .iter()
        .map(|status| (status.language_id.as_str(), status.provider_name.as_str(), status.availability))
        .collect();

    assert!(providers.contains(&("python", "ruff", LanguageProviderAvailability::Active)));
    assert!(providers.contains(&("python", "ty", LanguageProviderAvailability::Active)));
    assert!(providers.contains(&("rust", "rust-analyzer", LanguageProviderAvailability::Active)));
    assert!(providers.contains(&("c", "clangd", LanguageProviderAvailability::Degraded)));
    assert!(providers.contains(&("cpp", "clangd", LanguageProviderAvailability::Degraded)));
    assert!(providers.contains(&("cuda-cpp", "clangd", LanguageProviderAvailability::Degraded)));
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml language_capabilities_include
```

Expected: FAIL because `ToolPathSnapshot`, `ProjectMetadataSnapshot`, and `build_language_capabilities_for_paths` do not exist yet.

- [ ] **Step 3: Add snapshot structs and builder**

Add in `src-tauri/src/lib.rs`:

```rust
#[derive(Debug, Clone, Default)]
struct ToolPathSnapshot {
    ruff: Option<PathBuf>,
    ty: Option<PathBuf>,
    rust_analyzer: Option<PathBuf>,
    clangd: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Default)]
struct ProjectMetadataSnapshot {
    has_pyproject: bool,
    has_cargo_toml: bool,
    has_compile_commands: bool,
}

fn build_language_capabilities_for_paths(
    tools: ToolPathSnapshot,
    metadata: ProjectMetadataSnapshot,
) -> Vec<LanguageCapabilityStatus> {
    vec![
        language_capability_status_for_tool(
            SourceLanguage::Python,
            "ruff",
            tools.ruff,
            metadata.has_pyproject,
        ),
        language_capability_status_for_tool(
            SourceLanguage::Python,
            "ty",
            tools.ty,
            metadata.has_pyproject,
        ),
        language_capability_status_for_tool(
            SourceLanguage::Rust,
            "rust-analyzer",
            tools.rust_analyzer,
            metadata.has_cargo_toml,
        ),
        language_capability_status_for_tool(
            SourceLanguage::C,
            "clangd",
            tools.clangd.clone(),
            metadata.has_compile_commands,
        ),
        language_capability_status_for_tool(
            SourceLanguage::Cpp,
            "clangd",
            tools.clangd.clone(),
            metadata.has_compile_commands,
        ),
        language_capability_status_for_tool(
            SourceLanguage::Cuda,
            "clangd",
            tools.clangd,
            metadata.has_compile_commands,
        ),
    ]
}
```

- [ ] **Step 4: Add Tauri command**

Add a command near existing environment/bootstrap commands:

```rust
#[tauri::command]
async fn get_language_capabilities(workspace: Option<String>) -> Result<Vec<LanguageCapabilityStatus>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let workspace_root = workspace.map(PathBuf::from);
        let metadata = ProjectMetadataSnapshot {
            has_pyproject: workspace_root
                .as_ref()
                .is_some_and(|root| root.join("pyproject.toml").exists()),
            has_cargo_toml: workspace_root
                .as_ref()
                .is_some_and(|root| root.join("Cargo.toml").exists()),
            has_compile_commands: workspace_root
                .as_ref()
                .is_some_and(|root| root.join("compile_commands.json").exists()),
        };

        Ok(build_language_capabilities_for_paths(
            ToolPathSnapshot {
                ruff: probe_command("ruff"),
                ty: probe_command("ty"),
                rust_analyzer: probe_command("rust-analyzer"),
                clangd: probe_command("clangd"),
            },
            metadata,
        ))
    })
    .await
    .map_err(|error| error.to_string())?
}
```

Register `get_language_capabilities` in the existing `tauri::generate_handler![...]` list.

- [ ] **Step 5: Run tests and build**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml language_capabilities_include
npm run build
```

Expected: Rust test PASS and frontend build PASS.

## Task 3: Backend Agent Session State

**Files:**

- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add failing session state tests**

Add tests:

```rust
#[test]
fn agent_session_appends_user_message_and_enters_running_state() {
    let mut session = AgentSession::new(
        "session-1".to_string(),
        "codex".to_string(),
        Some("gpt-5.2".to_string()),
        "ask".to_string(),
        Some("C:/work/project".to_string()),
    );

    session.append_user_message("Review this file".to_string(), Vec::new());
    session.mark_running("turn-1".to_string());

    assert_eq!(session.messages.len(), 1);
    assert_eq!(session.messages[0].role, AgentSessionRole::User);
    assert_eq!(session.state, AgentSessionRunState::Running);
    assert_eq!(session.active_turn_id.as_deref(), Some("turn-1"));
}

#[test]
fn agent_session_cancel_moves_running_turn_to_idle() {
    let mut session = AgentSession::new(
        "session-1".to_string(),
        "codex".to_string(),
        None,
        "ask".to_string(),
        None,
    );

    session.mark_running("turn-1".to_string());
    session.cancel_running_turn();

    assert_eq!(session.state, AgentSessionRunState::Idle);
    assert!(session.active_turn_id.is_none());
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml agent_session_
```

Expected: FAIL because session types do not exist yet.

- [ ] **Step 3: Add session data structures**

Add in `src-tauri/src/lib.rs` near the existing agent payload structs:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum AgentSessionRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum AgentSessionRunState {
    Idle,
    Running,
    AwaitingApproval,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentContextAttachment {
    kind: String,
    label: String,
    content: String,
    estimated_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentSessionMessage {
    id: String,
    role: AgentSessionRole,
    content: String,
    attachments: Vec<AgentContextAttachment>,
    created_at_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentSession {
    id: String,
    provider_id: String,
    model_id: Option<String>,
    permission_level: String,
    workspace_root: Option<String>,
    state: AgentSessionRunState,
    active_turn_id: Option<String>,
    messages: Vec<AgentSessionMessage>,
}
```

- [ ] **Step 4: Add session methods**

Add:

```rust
fn monotonic_message_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

impl AgentSession {
    fn new(
        id: String,
        provider_id: String,
        model_id: Option<String>,
        permission_level: String,
        workspace_root: Option<String>,
    ) -> Self {
        Self {
            id,
            provider_id,
            model_id,
            permission_level,
            workspace_root,
            state: AgentSessionRunState::Idle,
            active_turn_id: None,
            messages: Vec::new(),
        }
    }

    fn append_user_message(&mut self, content: String, attachments: Vec<AgentContextAttachment>) {
        self.messages.push(AgentSessionMessage {
            id: format!("message-{}", self.messages.len() + 1),
            role: AgentSessionRole::User,
            content,
            attachments,
            created_at_ms: monotonic_message_timestamp_ms(),
        });
    }

    fn mark_running(&mut self, turn_id: String) {
        self.state = AgentSessionRunState::Running;
        self.active_turn_id = Some(turn_id);
    }

    fn cancel_running_turn(&mut self) {
        if self.state == AgentSessionRunState::Running {
            self.state = AgentSessionRunState::Idle;
            self.active_turn_id = None;
        }
    }
}
```

Ensure `std::time::{SystemTime, UNIX_EPOCH}` is imported if not already present.

- [ ] **Step 5: Run tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml agent_session_
```

Expected: PASS.

## Task 4: Agent Session Commands

**Files:**

- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add failing session registry tests**

Add tests:

```rust
#[test]
fn agent_session_registry_creates_and_reuses_sessions() {
    let registry = AgentSessionRegistry::default();

    let created = registry.create_session(CreateAgentSessionRequest {
        provider_id: "codex".to_string(),
        model_id: Some("gpt-5.2".to_string()),
        permission_level: "ask".to_string(),
        workspace_root: Some("C:/work/project".to_string()),
    });
    let fetched = registry.get_session(&created.id).expect("session should exist");

    assert_eq!(created.id, fetched.id);
    assert_eq!(fetched.provider_id, "codex");
    assert_eq!(fetched.state, AgentSessionRunState::Idle);
}

#[test]
fn agent_session_registry_sends_message_and_cancels_turn() {
    let registry = AgentSessionRegistry::default();
    let session = registry.create_session(CreateAgentSessionRequest {
        provider_id: "codex".to_string(),
        model_id: None,
        permission_level: "ask".to_string(),
        workspace_root: None,
    });

    let updated = registry
        .send_message(SendAgentSessionMessageRequest {
            session_id: session.id.clone(),
            content: "Explain the diagnostics".to_string(),
            attachments: Vec::new(),
        })
        .expect("message should be accepted");

    assert_eq!(updated.state, AgentSessionRunState::Running);
    assert_eq!(updated.messages.len(), 1);

    let cancelled = registry
        .cancel_session_turn(CancelAgentSessionTurnRequest {
            session_id: session.id,
        })
        .expect("turn should cancel");

    assert_eq!(cancelled.state, AgentSessionRunState::Idle);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml agent_session_registry
```

Expected: FAIL because registry and request types do not exist yet.

- [ ] **Step 3: Add request structs and registry**

Add:

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateAgentSessionRequest {
    provider_id: String,
    model_id: Option<String>,
    permission_level: String,
    workspace_root: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendAgentSessionMessageRequest {
    session_id: String,
    content: String,
    attachments: Vec<AgentContextAttachment>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CancelAgentSessionTurnRequest {
    session_id: String,
}

#[derive(Default)]
struct AgentSessionRegistry {
    sessions: Mutex<BTreeMap<String, AgentSession>>,
}
```

Add methods:

```rust
impl AgentSessionRegistry {
    fn create_session(&self, request: CreateAgentSessionRequest) -> AgentSession {
        let mut sessions = self.sessions.lock().expect("agent session lock poisoned");
        let id = format!("agent-session-{}", sessions.len() + 1);
        let session = AgentSession::new(
            id.clone(),
            request.provider_id,
            request.model_id,
            request.permission_level,
            request.workspace_root,
        );
        sessions.insert(id, session.clone());
        session
    }

    fn get_session(&self, session_id: &str) -> Option<AgentSession> {
        self.sessions
            .lock()
            .expect("agent session lock poisoned")
            .get(session_id)
            .cloned()
    }

    fn send_message(
        &self,
        request: SendAgentSessionMessageRequest,
    ) -> Result<AgentSession, String> {
        let mut sessions = self.sessions.lock().map_err(|error| error.to_string())?;
        let session = sessions
            .get_mut(&request.session_id)
            .ok_or_else(|| "Agent session was not found.".to_string())?;
        session.append_user_message(request.content, request.attachments);
        let next_turn = format!("{}-turn-{}", session.id, session.messages.len());
        session.mark_running(next_turn);
        Ok(session.clone())
    }

    fn cancel_session_turn(
        &self,
        request: CancelAgentSessionTurnRequest,
    ) -> Result<AgentSession, String> {
        let mut sessions = self.sessions.lock().map_err(|error| error.to_string())?;
        let session = sessions
            .get_mut(&request.session_id)
            .ok_or_else(|| "Agent session was not found.".to_string())?;
        session.cancel_running_turn();
        Ok(session.clone())
    }
}
```

Add static registry:

```rust
static AGENT_SESSIONS: OnceLock<AgentSessionRegistry> = OnceLock::new();

fn agent_sessions() -> &'static AgentSessionRegistry {
    AGENT_SESSIONS.get_or_init(AgentSessionRegistry::default)
}
```

- [ ] **Step 4: Add Tauri commands**

Add:

```rust
#[tauri::command]
fn create_agent_session(request: CreateAgentSessionRequest) -> Result<AgentSession, String> {
    Ok(agent_sessions().create_session(request))
}

#[tauri::command]
fn send_agent_session_message(
    request: SendAgentSessionMessageRequest,
) -> Result<AgentSession, String> {
    agent_sessions().send_message(request)
}

#[tauri::command]
fn cancel_agent_session_turn(
    request: CancelAgentSessionTurnRequest,
) -> Result<AgentSession, String> {
    agent_sessions().cancel_session_turn(request)
}
```

Register all three commands in `tauri::generate_handler![...]`.

- [ ] **Step 5: Run tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml agent_session_registry
```

Expected: PASS.

## Task 5: Frontend Language Capability Consumption

**Files:**

- Modify: `src/App.tsx`
- Build: `npm run build`

- [ ] **Step 1: Add frontend types and label helpers**

Add near existing editor/language types in `src/App.tsx`:

```ts
type LanguageProviderAvailability = "active" | "degraded" | "missing" | "unsupported";

type LanguageFeature =
  | "diagnostics"
  | "hover"
  | "outline"
  | "workspaceSymbols"
  | "documentSymbols"
  | "semanticTokens"
  | "formatting"
  | "organizeImports"
  | "codeActions"
  | "references"
  | "rename";

type LanguageCapabilityStatus = {
  languageId: string;
  providerName: string;
  availability: LanguageProviderAvailability;
  resolvedPath: string | null;
  supportedFeatures: LanguageFeature[];
  inactiveReason: string | null;
  recommendedAction: string | null;
};

function languageCapabilitySummary(statuses: LanguageCapabilityStatus[], languageId: string) {
  const matching = statuses.filter((status) => status.languageId === languageId);
  if (matching.length === 0) {
    return "No language provider is registered for this file type.";
  }
  const active = matching.filter((status) => status.availability === "active");
  if (active.length > 0) {
    const features = new Set(active.flatMap((status) => status.supportedFeatures));
    return `${active.map((status) => status.providerName).join(" + ")} active · ${features.size} features`;
  }
  const degraded = matching.find((status) => status.availability === "degraded");
  if (degraded) {
    return degraded.inactiveReason ?? `${degraded.providerName} is partially configured.`;
  }
  const missing = matching.find((status) => status.availability === "missing");
  if (missing) {
    return missing.inactiveReason ?? `${missing.providerName} is missing.`;
  }
  return "Language support is not enabled for this file type.";
}
```

- [ ] **Step 2: Add state and loader**

Add inside `App`:

```ts
const [languageCapabilities, setLanguageCapabilities] = createSignal<LanguageCapabilityStatus[]>([]);
const [languageCapabilityStatus, setLanguageCapabilityStatus] = createSignal("");

async function refreshLanguageCapabilities(root = workspaceRoot()) {
  try {
    const capabilities = await invokeCommand<LanguageCapabilityStatus[]>(
      "get_language_capabilities",
      { workspace: root },
    );
    setLanguageCapabilities(capabilities);
    setLanguageCapabilityStatus("Language capabilities refreshed.");
  } catch (error) {
    setLanguageCapabilityStatus(error instanceof Error ? error.message : String(error));
  }
}
```

Call `void refreshLanguageCapabilities(root);` after a workspace opens and `void refreshLanguageCapabilities();` during bootstrap after workspace state is hydrated.

- [ ] **Step 3: Render capability status near existing language/tooling status**

Add a compact status row in the existing project/tools area:

```tsx
<div class="language-capability-strip">
  <span>{activeDocument() ? languageCapabilitySummary(languageCapabilities(), activeDocument()!.language) : "Open a file to inspect language support."}</span>
  <button type="button" class="ghost-button" onClick={() => void refreshLanguageCapabilities()}>
    Refresh
  </button>
</div>
```

- [ ] **Step 4: Add minimal CSS**

Add in `src/App.css`:

```css
.language-capability-strip {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-height: 30px;
  padding: 6px 8px;
  border: 1px solid rgba(132, 146, 166, 0.22);
  border-radius: 6px;
  color: var(--muted-text);
  font-size: 12px;
}
```

- [ ] **Step 5: Build**

Run:

```powershell
npm run build
```

Expected: PASS.

## Task 6: Frontend Interactive Agent Sessions

**Files:**

- Modify: `src/App.tsx`
- Modify: `src/App.css`
- Build: `npm run build`

- [ ] **Step 1: Add frontend session types**

Add near existing chat types:

```ts
type AgentSessionRole = "user" | "assistant" | "system";
type AgentSessionRunState = "idle" | "running" | "awaitingApproval" | "error";

type AgentContextAttachment = {
  kind: string;
  label: string;
  content: string;
  estimatedChars: number;
};

type AgentSessionMessage = {
  id: string;
  role: AgentSessionRole;
  content: string;
  attachments: AgentContextAttachment[];
  createdAtMs: number;
};

type AgentSession = {
  id: string;
  providerId: string;
  modelId: string | null;
  permissionLevel: string;
  workspaceRoot: string | null;
  state: AgentSessionRunState;
  activeTurnId: string | null;
  messages: AgentSessionMessage[];
};
```

- [ ] **Step 2: Add explicit context attachment builder**

Add helper:

```ts
function buildAgentContextAttachments(input: {
  activeDocument: EditorDocument | null;
  includeActiveFile: boolean;
  includeDiagnostics: boolean;
  includeOpenTabs: boolean;
}): AgentContextAttachment[] {
  const attachments: AgentContextAttachment[] = [];
  if (input.includeActiveFile && input.activeDocument) {
    attachments.push({
      kind: "activeFile",
      label: input.activeDocument.path,
      content: input.activeDocument.content,
      estimatedChars: input.activeDocument.content.length,
    });
  }
  if (input.includeDiagnostics && input.activeDocument && input.activeDocument.diagnostics.length > 0) {
    const content = input.activeDocument.diagnostics
      .map((diagnostic) => `${diagnostic.severity}: ${diagnostic.message}`)
      .join("\n");
    attachments.push({
      kind: "diagnostics",
      label: "Active diagnostics",
      content,
      estimatedChars: content.length,
    });
  }
  if (input.includeOpenTabs) {
    const content = Object.values(documents)
      .map((document) => document.path)
      .join("\n");
    attachments.push({
      kind: "openTabs",
      label: "Open tabs",
      content,
      estimatedChars: content.length,
    });
  }
  return attachments;
}
```

- [ ] **Step 3: Add session state and commands**

Add inside `App`:

```ts
const [agentSession, setAgentSession] = createSignal<AgentSession | null>(null);
const [agentSessionInput, setAgentSessionInput] = createSignal("");
const [attachActiveFile, setAttachActiveFile] = createSignal(true);
const [attachDiagnostics, setAttachDiagnostics] = createSignal(true);
const [attachOpenTabs, setAttachOpenTabs] = createSignal(false);

async function ensureAgentSession() {
  const current = agentSession();
  if (current) {
    return current;
  }
  const agent = selectedAgent();
  const session = await invokeCommand<AgentSession>("create_agent_session", {
    request: {
      providerId: agent.id,
      modelId: selectedAgentModelId(agent.id) || null,
      permissionLevel: selectedAgentPermissionLevel(agent.id),
      workspaceRoot: workspaceRoot(),
    },
  });
  setAgentSession(session);
  return session;
}

async function sendAgentSessionMessage() {
  const content = agentSessionInput().trim();
  if (!content) {
    return;
  }
  const session = await ensureAgentSession();
  const updated = await invokeCommand<AgentSession>("send_agent_session_message", {
    request: {
      sessionId: session.id,
      content,
      attachments: buildAgentContextAttachments({
        activeDocument: activeDocument() ?? null,
        includeActiveFile: attachActiveFile(),
        includeDiagnostics: attachDiagnostics(),
        includeOpenTabs: attachOpenTabs(),
      }),
    },
  });
  setAgentSession(updated);
  setAgentSessionInput("");
}

async function cancelAgentSessionTurn() {
  const session = agentSession();
  if (!session || session.state !== "running") {
    return;
  }
  const updated = await invokeCommand<AgentSession>("cancel_agent_session_turn", {
    request: { sessionId: session.id },
  });
  setAgentSession(updated);
}
```

- [ ] **Step 4: Render session chat controls in Agents pane**

Add in the existing Agents/Chat utility pane:

```tsx
<div class="agent-session-panel">
  <div class="agent-session-toolbar">
    <span>{agentSession()?.state ?? "idle"}</span>
    <button
      type="button"
      class="ghost-button"
      disabled={agentSession()?.state !== "running"}
      onClick={() => void cancelAgentSessionTurn()}
    >
      Cancel
    </button>
  </div>
  <div class="agent-attachment-row">
    <label><input type="checkbox" checked={attachActiveFile()} onChange={(event) => setAttachActiveFile(event.currentTarget.checked)} /> Active file</label>
    <label><input type="checkbox" checked={attachDiagnostics()} onChange={(event) => setAttachDiagnostics(event.currentTarget.checked)} /> Diagnostics</label>
    <label><input type="checkbox" checked={attachOpenTabs()} onChange={(event) => setAttachOpenTabs(event.currentTarget.checked)} /> Open tabs</label>
  </div>
  <div class="agent-session-messages">
    <For each={agentSession()?.messages ?? []}>
      {(message) => (
        <article class={`agent-session-message ${message.role}`}>
          <strong>{message.role}</strong>
          <p>{message.content}</p>
          <Show when={message.attachments.length > 0}>
            <small>{message.attachments.length} attachments · {message.attachments.reduce((sum, item) => sum + item.estimatedChars, 0)} chars</small>
          </Show>
        </article>
      )}
    </For>
  </div>
  <textarea
    class="agent-session-input"
    value={agentSessionInput()}
    onInput={(event) => setAgentSessionInput(event.currentTarget.value)}
    placeholder="Ask the selected agent..."
  />
  <button type="button" class="primary-button" onClick={() => void sendAgentSessionMessage()}>
    Send
  </button>
</div>
```

- [ ] **Step 5: Add minimal CSS**

Add:

```css
.agent-session-panel,
.agent-session-messages {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.agent-session-toolbar,
.agent-attachment-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.agent-session-message {
  border: 1px solid rgba(132, 146, 166, 0.22);
  border-radius: 6px;
  padding: 8px;
}

.agent-session-input {
  min-height: 76px;
  resize: vertical;
}
```

- [ ] **Step 6: Build**

Run:

```powershell
npm run build
```

Expected: PASS.

## Task 7: Verification and Integration Check

**Files:**

- Read/verify: `src-tauri/src/lib.rs`
- Read/verify: `src/App.tsx`
- Read/verify: `src/App.css`

- [ ] **Step 1: Run focused backend tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml language_capability_status
cargo test --manifest-path src-tauri/Cargo.toml language_capabilities_include
cargo test --manifest-path src-tauri/Cargo.toml agent_session_
```

Expected: all three commands PASS.

- [ ] **Step 2: Run frontend build**

Run:

```powershell
npm run build
```

Expected: PASS.

- [ ] **Step 3: Run debug Tauri build**

Run:

```powershell
npm run tauri -- build --debug
```

Expected: PASS.

- [ ] **Step 4: Manual smoke validation**

Run the app and verify:

- Opening a workspace refreshes language capability status.
- Opening a Python, Rust, or C-family file shows a truthful active, degraded, or missing-provider summary.
- The Agents pane allows a session message with explicit context attachments.
- Cancelling a running session returns the session to idle.
- No language provider or agent starts whole-workspace work on launch.

## Self-Review

Spec coverage:

- Language capability model: Tasks 1 and 2.
- Lazy, modular provider model: Tasks 1 and 2 keep capability detection separate from analysis execution.
- Interactive agent sessions: Tasks 3, 4, and 6.
- Explicit context attachments: Task 6.
- Cancel control: Tasks 3, 4, and 6.
- Tests: Tasks 1 through 4 and Task 7.
- Diff preview: intentionally deferred by the approved spec as a subsequent phase.
- Provider streaming into real Codex/Gemini/Claude/Kilo processes: intentionally not in Phase 1; Task 4 creates the shared session state needed before provider adapters are wired.

Placeholder scan:

- No placeholder tokens are used.
- Every code-changing task includes concrete code snippets.
- Verification commands and expected outcomes are listed.

Type consistency:

- Backend `AgentSession`, `AgentContextAttachment`, and request names match the frontend camelCase request payloads.
- Backend language feature names use `serde(rename_all = "camelCase")`, matching frontend union values.
- Availability values use camelCase serialization and lowercase frontend strings.
