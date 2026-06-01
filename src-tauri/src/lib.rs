use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Output, Stdio},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{
    AppHandle, Emitter, Manager,
    menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu},
};
use tree_sitter::{Node, Parser};
use walkdir::WalkDir;

const MAX_EDITOR_FILE_BYTES: u64 = 512 * 1024;
const MAX_CONTEXT_CHARS: usize = 4_200;
const CONTEXT_FILE_LIMIT: usize = 8;
const MAX_AUTOMATIC_PYTHON_ANALYSIS_BYTES: usize = 128 * 1024;
const MAX_AUTOMATIC_RUST_ANALYSIS_BYTES: usize = 192 * 1024;
const MAX_AUTOMATIC_C_FAMILY_ANALYSIS_BYTES: usize = 192 * 1024;
const RUST_ANALYZER_HOVER_RETRY_DELAYS_MS: &[u64] = &[80, 160, 320, 640];
const TERMINAL_CWD_MARKER: &str = "__HEMATITE_CWD__=";
const TERMINAL_STATUS_MARKER: &str = "__HEMATITE_STATUS__=";
const PYTHON_INSTALL_FAILURE_COOLDOWN: Duration = Duration::from_secs(45);
const UI_STATE_FILE_NAME: &str = "ui-state.json";
const LEGACY_UI_STATE_IDENTIFIERS: &[&str] = &["com.entity_27th.hematite"];
const CODEX_SAFE_MODEL_FOR_OLD_GPT55_CONFIG: &str = "gpt-5.2";
const MAX_AGENT_SESSIONS: usize = 12;
const MAX_AGENT_SESSION_MESSAGES: usize = 80;
const MAX_AGENT_MESSAGE_CHARS: usize = 120_000;
const MAX_AGENT_ATTACHMENT_CHARS: usize = 120_000;
const MAX_AGENT_RUN_OUTPUT_CHARS: usize = 200_000;
#[cfg(target_os = "windows")]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

static PYTHON_INSTALL_FAILURES: OnceLock<Mutex<BTreeMap<String, Instant>>> = OnceLock::new();
static PYTHON_INSTALL_IN_PROGRESS: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
static CODEX_APP_SERVER: OnceLock<Mutex<CodexAppServerState>> = OnceLock::new();
static CODEX_MODEL_OVERRIDE: OnceLock<Option<String>> = OnceLock::new();
static GEMINI_ACP: OnceLock<Mutex<GeminiAcpState>> = OnceLock::new();
static CODESHARE_STATE: OnceLock<Mutex<CodeShareState>> = OnceLock::new();
static TY_LSP: OnceLock<Mutex<TyLspState>> = OnceLock::new();
static RUST_ANALYZER_LSP: OnceLock<Mutex<RustAnalyzerLspState>> = OnceLock::new();
static AGENT_SESSIONS: OnceLock<AgentSessionRegistry> = OnceLock::new();
static TERMINAL_SESSIONS: OnceLock<TerminalSessionRegistry> = OnceLock::new();

const PYTHON_MISSING_IMPORT_PREFIX: &str = "import:";
const TERMINAL_PTY_COLS: u16 = 120;
const TERMINAL_PTY_ROWS: u16 = 30;
const LSP_SEMANTIC_TOKEN_TYPES: &[&str] = &[
    "namespace",
    "type",
    "class",
    "enum",
    "interface",
    "struct",
    "typeParameter",
    "parameter",
    "variable",
    "property",
    "enumMember",
    "event",
    "function",
    "method",
    "macro",
    "keyword",
    "modifier",
    "comment",
    "string",
    "number",
    "regexp",
    "operator",
];

const RUST_TOOL_STATUS_SPECS: &[ToolStatusSpec] = &[
    ToolStatusSpec {
        id: "rustup",
        label: "rustup",
    },
    ToolStatusSpec {
        id: "rustc",
        label: "Rust compiler",
    },
    ToolStatusSpec {
        id: "cargo",
        label: "Cargo",
    },
    ToolStatusSpec {
        id: "rustfmt",
        label: "rustfmt",
    },
    ToolStatusSpec {
        id: "cargo-clippy",
        label: "Clippy",
    },
    ToolStatusSpec {
        id: "rust-analyzer",
        label: "rust-analyzer",
    },
    ToolStatusSpec {
        id: "lldb",
        label: "LLDB",
    },
    ToolStatusSpec {
        id: "codelldb",
        label: "CodeLLDB",
    },
    ToolStatusSpec {
        id: "wasm-pack",
        label: "wasm-pack",
    },
    ToolStatusSpec {
        id: "cargo-nextest",
        label: "cargo-nextest",
    },
    ToolStatusSpec {
        id: "cargo-watch",
        label: "cargo-watch",
    },
    ToolStatusSpec {
        id: "cargo-audit",
        label: "cargo-audit",
    },
    ToolStatusSpec {
        id: "cargo-deny",
        label: "cargo-deny",
    },
    ToolStatusSpec {
        id: "cargo-expand",
        label: "cargo-expand",
    },
    ToolStatusSpec {
        id: "cargo-llvm-cov",
        label: "cargo-llvm-cov",
    },
];

const C_FAMILY_TOOL_STATUS_SPECS: &[ToolStatusSpec] = &[
    ToolStatusSpec {
        id: "clangd",
        label: "clangd",
    },
    ToolStatusSpec {
        id: "clang",
        label: "Clang",
    },
    ToolStatusSpec {
        id: "clang++",
        label: "Clang++",
    },
    ToolStatusSpec {
        id: "gcc",
        label: "GCC",
    },
    ToolStatusSpec {
        id: "g++",
        label: "G++",
    },
    ToolStatusSpec {
        id: "cl",
        label: "MSVC cl",
    },
    ToolStatusSpec {
        id: "cmake",
        label: "CMake",
    },
    ToolStatusSpec {
        id: "ninja",
        label: "Ninja",
    },
    ToolStatusSpec {
        id: "make",
        label: "Make",
    },
    ToolStatusSpec {
        id: "nvcc",
        label: "NVCC",
    },
    ToolStatusSpec {
        id: "cuda-gdb",
        label: "cuda-gdb",
    },
];

#[derive(Clone, Copy)]
struct ToolStatusSpec {
    id: &'static str,
    label: &'static str,
}

#[derive(Clone, Copy, Debug)]
enum SourceLanguage {
    Python,
    Rust,
    C,
    Cpp,
    Cuda,
    JavaScript,
    TypeScript,
    Tsx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
enum LanguageProviderAvailability {
    Active,
    Degraded,
    Missing,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
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

#[derive(Clone, Debug)]
struct ImportCandidate {
    module: String,
    line: u32,
    column: u32,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentCredentials {
    openai_api_key: Option<String>,
    gemini_api_key: Option<String>,
    google_api_key: Option<String>,
    google_cloud_project: Option<String>,
    google_cloud_location: Option<String>,
    google_application_credentials: Option<String>,
    anthropic_api_key: Option<String>,
    kilo_api_key: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapPayload {
    default_root: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolStatus {
    id: String,
    label: String,
    available: bool,
    resolved_path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DirectoryListing {
    path: String,
    entries: Vec<FileEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileDocument {
    path: String,
    name: String,
    language: String,
    content: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveFileRequest {
    path: String,
    content: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateFileRequest {
    root: String,
    relative_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveUiStateRequest {
    state_json: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SymbolEntry {
    kind: String,
    label: String,
    start_line: u32,
    end_line: u32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SemanticToken {
    kind: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct HoverItem {
    kind: String,
    title: String,
    detail: Option<String>,
    source: Option<String>,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct EditorSemanticsPayload {
    tokens: Vec<SemanticToken>,
    hover_items: Vec<HoverItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompactContextPayload {
    context: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompactContextRequest {
    root: String,
    current_file: Option<String>,
    content: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentRunRequest {
    root: String,
    binary: String,
    args: Vec<String>,
    stdin_prompt: bool,
    prompt: String,
    include_compact_context: bool,
    current_file: Option<String>,
    content: Option<String>,
    model: Option<String>,
    permission_level: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentRunResponse {
    success: bool,
    command: Vec<String>,
    prompt: String,
    stdout: String,
    stderr: String,
    context: Option<String>,
}

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
    next_session_id: AtomicUsize,
}

struct TerminalSession {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
}

#[derive(Default)]
struct TerminalSessionRegistry {
    sessions: Mutex<BTreeMap<String, TerminalSession>>,
    next_session_id: AtomicUsize,
}

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
    let supported_features = features_for_language_provider(
        language,
        provider_name,
        project_ready && resolved_path.is_some(),
    );

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
        LanguageProviderAvailability::Degraded => Some(
            "Open a configured project or add the metadata this language server expects."
                .to_string(),
        ),
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

fn monotonic_message_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let mut end = 0;
    for (count, (index, ch)) in value.char_indices().enumerate() {
        if count == max_chars {
            break;
        }
        end = index + ch.len_utf8();
    }

    let mut truncated = value[..end].to_string();
    truncated.push_str("\n[truncated]");
    truncated
}

fn bounded_agent_attachments(
    attachments: Vec<AgentContextAttachment>,
) -> Vec<AgentContextAttachment> {
    attachments
        .into_iter()
        .map(|attachment| {
            let content = truncate_text(&attachment.content, MAX_AGENT_ATTACHMENT_CHARS);
            AgentContextAttachment {
                estimated_chars: content.chars().count(),
                content,
                ..attachment
            }
        })
        .collect()
}

fn bounded_process_output(bytes: &[u8]) -> String {
    truncate_text(
        String::from_utf8_lossy(bytes).trim(),
        MAX_AGENT_RUN_OUTPUT_CHARS,
    )
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
            content: truncate_text(&content, MAX_AGENT_MESSAGE_CHARS),
            attachments: bounded_agent_attachments(attachments),
            created_at_ms: monotonic_message_timestamp_ms(),
        });
        if self.messages.len() > MAX_AGENT_SESSION_MESSAGES {
            let overflow = self.messages.len() - MAX_AGENT_SESSION_MESSAGES;
            self.messages.drain(0..overflow);
        }
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

impl AgentSessionRegistry {
    fn create_session(&self, request: CreateAgentSessionRequest) -> AgentSession {
        let mut sessions = self.sessions.lock().expect("agent session lock poisoned");
        while sessions.len() >= MAX_AGENT_SESSIONS {
            let Some(oldest_id) = sessions.keys().next().cloned() else {
                break;
            };
            sessions.remove(&oldest_id);
        }
        let next_id = self.next_session_id.fetch_add(1, Ordering::Relaxed) + 1;
        let id = format!("agent-session-{next_id}");
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

    #[allow(dead_code)]
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

fn agent_sessions() -> &'static AgentSessionRegistry {
    AGENT_SESSIONS.get_or_init(AgentSessionRegistry::default)
}

fn terminal_sessions() -> &'static TerminalSessionRegistry {
    TERMINAL_SESSIONS.get_or_init(TerminalSessionRegistry::default)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PythonImportRequest {
    root: String,
    file_path: String,
    source: String,
    auto_install: bool,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum PythonToolingAction {
    Check,
    FixAll,
    Format,
    OrganizeImports,
    TypeCheck,
}

struct PythonToolingCommand {
    binary: &'static str,
    args: Vec<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum RustToolingAction {
    Check,
    Clippy,
    Format,
    Test,
    Build,
    Doc,
    Metadata,
}

struct RustToolingCommand {
    binary: &'static str,
    args: Vec<String>,
    parses_diagnostics: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PythonToolingRequest {
    root: String,
    file_path: String,
    action: PythonToolingAction,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RustToolingRequest {
    root: String,
    file_path: String,
    action: RustToolingAction,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RustDiagnosticsRequest {
    root: String,
    file_path: String,
    source: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditorHoverRequest {
    root: String,
    file_path: String,
    source: String,
    line: u32,
    column: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EditorDiagnostic {
    module: String,
    from: usize,
    to: usize,
    line: u32,
    column: u32,
    severity: String,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EditorCompletionItem {
    label: String,
    detail: Option<String>,
    kind: String,
    insert_text: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EditorCodeAction {
    title: String,
    kind: Option<String>,
    edit: Option<Value>,
    command: Option<Value>,
    is_preferred: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EditorLocation {
    uri: String,
    path: Option<String>,
    line: u32,
    column: u32,
    end_line: u32,
    end_column: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EditorInlayHint {
    label: String,
    line: u32,
    column: u32,
    kind: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PythonImportEvent {
    module: String,
    package: String,
    success: bool,
    state: String,
    command: String,
    output: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PythonImportResponse {
    environment_ready: bool,
    environment_path: Option<String>,
    diagnostics: Vec<EditorDiagnostic>,
    events: Vec<PythonImportEvent>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CredentialSnapshot {
    has_openai_api_key: bool,
    has_gemini_api_key: bool,
    has_google_api_key: bool,
    has_anthropic_api_key: bool,
    has_kilo_api_key: bool,
    google_cloud_project: Option<String>,
    google_cloud_location: Option<String>,
    google_application_credentials: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentStatus {
    id: String,
    label: String,
    available: bool,
    resolved_path: Option<String>,
    auth_state: String,
    auth_source: Option<String>,
    summary: String,
    supports_oauth: bool,
    supports_api_key: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentHealthPayload {
    agents: Vec<AgentStatus>,
    credentials: CredentialSnapshot,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeAuthStatusPayload {
    logged_in: bool,
    auth_method: Option<String>,
    api_provider: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveAgentCredentialsRequest {
    openai_api_key: Option<String>,
    gemini_api_key: Option<String>,
    google_api_key: Option<String>,
    google_cloud_project: Option<String>,
    google_cloud_location: Option<String>,
    google_application_credentials: Option<String>,
    anthropic_api_key: Option<String>,
    kilo_api_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentLoginRequest {
    agent_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PythonEnvironmentStatus {
    root: String,
    uv_available: bool,
    pyproject_exists: bool,
    venv_exists: bool,
    python_path: Option<String>,
    summary: String,
    recommended_command: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RustEnvironmentStatus {
    root: String,
    cargo_toml_exists: bool,
    rust_toolchain_file: Option<String>,
    active_toolchain: Option<String>,
    installed_toolchains: Vec<String>,
    installed_components: Vec<String>,
    rustc_version: Option<String>,
    cargo_version: Option<String>,
    rust_analyzer_available: bool,
    rustfmt_available: bool,
    clippy_available: bool,
    lldb_available: bool,
    codelldb_available: bool,
    summary: String,
    recommended_command: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CFamilyEnvironmentStatus {
    root: String,
    cmake_lists_exists: bool,
    compile_commands_exists: bool,
    clangd_available: bool,
    clang_available: bool,
    clangxx_available: bool,
    gcc_available: bool,
    gxx_available: bool,
    msvc_cl_available: bool,
    cmake_available: bool,
    ninja_available: bool,
    make_available: bool,
    nvcc_available: bool,
    cuda_gdb_available: bool,
    lldb_available: bool,
    codelldb_available: bool,
    clangd_version: Option<String>,
    clang_version: Option<String>,
    gcc_version: Option<String>,
    nvcc_version: Option<String>,
    summary: String,
    recommended_command: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProcessOutcome {
    success: bool,
    command: String,
    stdout: String,
    stderr: String,
    diagnostics: Vec<EditorDiagnostic>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TerminalCommandRequest {
    command: String,
    cwd: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalCommandResponse {
    success: bool,
    command: String,
    stdout: String,
    stderr: String,
    cwd: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TerminalSessionStartRequest {
    cwd: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalSessionStartResponse {
    session_id: String,
    cwd: String,
    shell: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TerminalSessionInputRequest {
    session_id: String,
    input: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TerminalSessionCommandRequest {
    session_id: String,
    command: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TerminalSessionStopRequest {
    session_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TerminalSessionResizeRequest {
    session_id: String,
    rows: u16,
    cols: u16,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalSessionEvent {
    session_id: String,
    kind: String,
    data: String,
    success: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodeShareSession {
    session_id: String,
    invite_code: String,
    invite_link: String,
    title: String,
    root: String,
    active_file: Option<String>,
    agent_label: String,
    permission_level: String,
    status: String,
    participant_count: u32,
    created_at: u64,
    updated_at: u64,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodeShareSessionRequest {
    title: String,
    root: String,
    active_file: Option<String>,
    agent_label: String,
    permission_level: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodeShareJoinRequest {
    invite_code: String,
}

#[derive(Default)]
struct CodeShareState {
    next_session_id: u64,
    sessions_by_invite: BTreeMap<String, CodeShareSession>,
}

struct PreparedCommand {
    command: Command,
    preview: Vec<String>,
}

#[derive(Default)]
struct CodexAppServerState {
    session: Option<CodexAppServerSession>,
}

#[derive(Default)]
struct GeminiAcpState {
    session: Option<GeminiAcpSession>,
}

#[derive(Default)]
struct TyLspState {
    session: Option<TyLspSession>,
}

#[derive(Default)]
struct RustAnalyzerLspState {
    session: Option<RustAnalyzerLspSession>,
}

struct CodexAppServerSession {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    shared: Arc<Mutex<CodexSharedState>>,
}

struct GeminiAcpSession {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    shared: Arc<Mutex<GeminiSharedState>>,
}

struct TyLspSession {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    shared: Arc<Mutex<TyLspSharedState>>,
}

struct RustAnalyzerLspSession {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    shared: Arc<Mutex<RustAnalyzerLspSharedState>>,
}

struct CodexPendingServerRequest {
    id: Value,
    method: String,
    params: Value,
}

struct CodexSharedState {
    next_request_id: u64,
    initialized: bool,
    current_root: String,
    current_thread_id: Option<String>,
    current_thread_model: Option<String>,
    current_thread_permission_level: Option<AgentPermissionLevel>,
    active_turn_id: Option<String>,
    last_stderr: Option<String>,
    pending_responses: BTreeMap<String, mpsc::Sender<Result<Value, String>>>,
    pending_server_requests: BTreeMap<String, CodexPendingServerRequest>,
}

impl CodexSharedState {
    fn new(root: String) -> Self {
        Self {
            next_request_id: 1,
            initialized: false,
            current_root: root,
            current_thread_id: None,
            current_thread_model: None,
            current_thread_permission_level: None,
            active_turn_id: None,
            last_stderr: None,
            pending_responses: BTreeMap::new(),
            pending_server_requests: BTreeMap::new(),
        }
    }
}

struct GeminiPendingRequest {
    id: Value,
    method: String,
    params: Value,
}

struct GeminiSharedState {
    next_request_id: u64,
    initialized: bool,
    current_root: String,
    current_model: Option<String>,
    current_permission_level: AgentPermissionLevel,
    current_session_id: Option<String>,
    prompt_in_progress: bool,
    pending_responses: BTreeMap<String, mpsc::Sender<Result<Value, String>>>,
    pending_requests: BTreeMap<String, GeminiPendingRequest>,
}

impl GeminiSharedState {
    fn new(root: String, model: Option<String>, permission_level: AgentPermissionLevel) -> Self {
        Self {
            next_request_id: 1,
            initialized: false,
            current_root: root,
            current_model: model,
            current_permission_level: permission_level,
            current_session_id: None,
            prompt_in_progress: false,
            pending_responses: BTreeMap::new(),
            pending_requests: BTreeMap::new(),
        }
    }
}

struct TyLspSharedState {
    next_request_id: u64,
    initialized: bool,
    failed: bool,
    current_root: String,
    python_environment: Option<String>,
    pull_diagnostics: bool,
    token_types: Vec<String>,
    synced_documents: BTreeMap<String, i32>,
    published_diagnostics: BTreeMap<String, TyPublishedDiagnostics>,
    pending_responses: BTreeMap<String, mpsc::Sender<Result<Value, String>>>,
}

#[derive(Clone)]
struct TyPublishedDiagnostics {
    version: Option<i32>,
    diagnostics: Vec<Value>,
}

impl TyLspSharedState {
    fn new(root: String, python_environment: Option<String>) -> Self {
        Self {
            next_request_id: 1,
            initialized: false,
            failed: false,
            current_root: root,
            python_environment,
            pull_diagnostics: false,
            token_types: LSP_SEMANTIC_TOKEN_TYPES
                .iter()
                .map(|value| value.to_string())
                .collect(),
            synced_documents: BTreeMap::new(),
            published_diagnostics: BTreeMap::new(),
            pending_responses: BTreeMap::new(),
        }
    }
}

struct RustAnalyzerLspSharedState {
    next_request_id: u64,
    initialized: bool,
    failed: bool,
    current_root: String,
    token_types: Vec<String>,
    synced_documents: BTreeMap<String, i32>,
    published_diagnostics: BTreeMap<String, RustAnalyzerPublishedDiagnostics>,
    pending_responses: BTreeMap<String, mpsc::Sender<Result<Value, String>>>,
}

#[derive(Clone)]
struct RustAnalyzerPublishedDiagnostics {
    version: Option<i32>,
    diagnostics: Vec<Value>,
}

impl RustAnalyzerLspSharedState {
    fn new(root: String) -> Self {
        Self {
            next_request_id: 1,
            initialized: false,
            failed: false,
            current_root: root,
            token_types: LSP_SEMANTIC_TOKEN_TYPES
                .iter()
                .map(|value| value.to_string())
                .collect(),
            synced_documents: BTreeMap::new(),
            published_diagnostics: BTreeMap::new(),
            pending_responses: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FrontendMenuEvent {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexTurnRequest {
    root: String,
    prompt: String,
    include_compact_context: bool,
    current_file: Option<String>,
    content: Option<String>,
    model: Option<String>,
    permission_level: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexTurnResponse {
    thread_id: String,
    turn_id: String,
    prompt: String,
    context: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexApprovalResponseRequest {
    request_id: String,
    decision: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexResetRequest {
    root: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexResetResponse {
    reset: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexPermissionSummary {
    network_enabled: Option<bool>,
    read_roots: Vec<String>,
    write_roots: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FrontendApprovalChoice {
    id: String,
    label: String,
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum CodexFrontendEvent {
    AgentMessageDelta {
        turn_id: String,
        item_id: String,
        delta: String,
    },
    AgentMessageCompleted {
        turn_id: String,
        item_id: String,
        text: String,
    },
    ApprovalRequested {
        request_id: String,
        approval_type: String,
        turn_id: String,
        item_id: String,
        reason: Option<String>,
        command: Option<String>,
        cwd: Option<String>,
        grant_root: Option<String>,
        permissions: Option<CodexPermissionSummary>,
        choices: Vec<FrontendApprovalChoice>,
    },
    ApprovalResolved {
        request_id: String,
    },
    TurnCompleted {
        turn_id: String,
        success: bool,
        error: Option<String>,
    },
    Error {
        message: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiTurnRequest {
    root: String,
    prompt: String,
    include_compact_context: bool,
    current_file: Option<String>,
    content: Option<String>,
    model: Option<String>,
    permission_level: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiTurnResponse {
    session_id: String,
    prompt: String,
    context: Option<String>,
    stop_reason: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiApprovalResponseRequest {
    request_id: String,
    option_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiResetRequest {
    root: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiResetResponse {
    reset: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiToolLocation {
    path: String,
    line: Option<u32>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum GeminiFrontendEvent {
    AgentMessageDelta {
        session_id: String,
        delta: String,
    },
    ApprovalRequested {
        request_id: String,
        session_id: String,
        title: String,
        tool_kind: Option<String>,
        command: Option<String>,
        locations: Vec<GeminiToolLocation>,
        choices: Vec<FrontendApprovalChoice>,
    },
    ApprovalResolved {
        request_id: String,
    },
    PromptCompleted {
        session_id: String,
        success: bool,
        stop_reason: String,
        error: Option<String>,
    },
    Error {
        message: String,
    },
}

#[tauri::command]
fn bootstrap() -> Result<BootstrapPayload, String> {
    let default_root = detect_workspace_root()?;

    Ok(BootstrapPayload { default_root })
}

#[tauri::command]
async fn refresh_tool_statuses() -> Result<Vec<ToolStatus>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut statuses = vec![
            make_tool_status("uv", "astral-uv"),
            make_tool_status("python", "Python"),
            make_tool_status("ruff", "Ruff"),
            make_tool_status("ty", "ty"),
            make_tool_status("codex", "OpenAI Codex"),
            make_tool_status("gemini", "Gemini CLI"),
            make_tool_status("claude", "Claude Code"),
            make_tool_status("kilo", "Kilo Code"),
        ];
        statuses.extend(
            rust_tool_status_specs()
                .iter()
                .map(|spec| make_tool_status(spec.id, spec.label)),
        );
        statuses.extend(
            c_family_tool_status_specs()
                .iter()
                .map(|spec| make_tool_status(spec.id, spec.label)),
        );
        Ok(statuses)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
fn list_directory(path: Option<String>) -> Result<DirectoryListing, String> {
    let raw_path = path
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(detect_workspace_root().unwrap_or_else(|_| ".".into())));
    let canonical = fs::canonicalize(&raw_path).map_err(|err| err.to_string())?;
    let mut entries = Vec::new();

    for entry in fs::read_dir(&canonical).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if should_ignore_name(&name) {
            continue;
        }

        let metadata = entry.metadata().map_err(|err| err.to_string())?;
        entries.push(FileEntry {
            name,
            path: path_to_string(&path),
            is_dir: metadata.is_dir(),
            size: metadata.is_file().then_some(metadata.len()),
        });
    }

    entries.sort_by(|left, right| match (left.is_dir, right.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
    });

    Ok(DirectoryListing {
        path: path_to_string(&canonical),
        entries,
    })
}

#[tauri::command]
fn read_file(path: String) -> Result<FileDocument, String> {
    let raw_path = PathBuf::from(path);
    let metadata = fs::metadata(&raw_path).map_err(|err| err.to_string())?;

    if metadata.len() > MAX_EDITOR_FILE_BYTES {
        return Err(format!(
            "Files larger than {} KB are intentionally not opened inline.",
            MAX_EDITOR_FILE_BYTES / 1024
        ));
    }

    let bytes = fs::read(&raw_path).map_err(|err| err.to_string())?;
    let content = String::from_utf8(bytes).map_err(|_| {
        "This file does not look like UTF-8 text, so Hematite skipped it.".to_string()
    })?;

    Ok(FileDocument {
        path: path_to_string(&raw_path),
        name: raw_path
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".into()),
        language: language_id_from_path(&raw_path).into(),
        content,
    })
}

#[tauri::command]
fn save_file(request: SaveFileRequest) -> Result<(), String> {
    fs::write(request.path, request.content).map_err(|err| err.to_string())
}

#[tauri::command]
fn create_file(request: CreateFileRequest) -> Result<FileDocument, String> {
    let root = fs::canonicalize(&request.root).map_err(|err| err.to_string())?;
    let relative = request.relative_path.trim();
    if relative.is_empty() {
        return Err("New file path cannot be empty.".into());
    }

    let candidate = PathBuf::from(relative);
    if candidate.is_absolute() {
        return Err("Use a workspace-relative path when creating a new file.".into());
    }

    if candidate
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("New file paths cannot escape the workspace root.".into());
    }

    let resolved = root.join(candidate);
    if resolved.exists() {
        return Err("A file or directory already exists at that path.".into());
    }

    if let Some(parent) = resolved.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    fs::write(&resolved, "").map_err(|err| err.to_string())?;
    read_file(path_to_string(&resolved))
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|existing| existing == &candidate) {
        paths.push(candidate);
    }
}

fn current_ui_state_paths<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(directory) = app.path().app_local_data_dir() {
        push_unique_path(&mut paths, directory.join(UI_STATE_FILE_NAME));
    }

    if let Ok(directory) = app.path().app_data_dir() {
        push_unique_path(&mut paths, directory.join(UI_STATE_FILE_NAME));
    }

    paths
}

fn legacy_ui_state_paths<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for path in current_ui_state_paths(app) {
        if let Some(root) = path.parent().and_then(Path::parent) {
            push_unique_path(&mut roots, root.to_path_buf());
        }
    }

    let mut paths = Vec::new();
    for root in roots {
        for identifier in LEGACY_UI_STATE_IDENTIFIERS {
            push_unique_path(&mut paths, root.join(identifier).join(UI_STATE_FILE_NAME));
        }
    }

    paths
}

#[tauri::command]
fn load_ui_state(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let mut last_error = None;

    for path in current_ui_state_paths(&app)
        .into_iter()
        .chain(legacy_ui_state_paths(&app))
    {
        match fs::read_to_string(&path) {
            Ok(contents) => return Ok(Some(contents)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => last_error = Some(err.to_string()),
        }
    }

    if let Some(error) = last_error {
        Err(error)
    } else {
        Ok(None)
    }
}

#[tauri::command]
fn save_ui_state(app: tauri::AppHandle, request: SaveUiStateRequest) -> Result<(), String> {
    let mut saved_any = false;
    let mut last_error = None;

    for path in current_ui_state_paths(&app) {
        if let Some(directory) = path.parent() {
            if let Err(err) = fs::create_dir_all(directory) {
                last_error = Some(err.to_string());
                continue;
            }
        }

        match fs::write(&path, &request.state_json) {
            Ok(_) => saved_any = true,
            Err(err) => last_error = Some(err.to_string()),
        }
    }

    if saved_any {
        Ok(())
    } else {
        Err(last_error.unwrap_or_else(|| "Could not save ui-state.json.".into()))
    }
}

#[tauri::command]
async fn extract_symbols(path: String, content: String) -> Result<Vec<SymbolEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path_buf = PathBuf::from(path);
        Ok(parse_symbols_for_path(&path_buf, &content))
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn analyze_editor_semantics(
    path: String,
    content: String,
) -> Result<EditorSemanticsPayload, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path_buf = PathBuf::from(path);
        Ok(analyze_editor_semantics_for_path(&path_buf, &content))
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn request_editor_hover(request: EditorHoverRequest) -> Result<Option<HoverItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        match language_id_from_path(Path::new(&request.file_path)) {
            "python" => request_ty_hover(&request),
            "rust" => request_rust_analyzer_hover(&request),
            "c" => Ok(request_c_family_hover(&request, SourceLanguage::C)),
            "cpp" => Ok(request_c_family_hover(&request, SourceLanguage::Cpp)),
            "cuda-cpp" => Ok(request_c_family_hover(&request, SourceLanguage::Cuda)),
            _ => Ok(None),
        }
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn request_editor_completions(
    request: EditorHoverRequest,
) -> Result<Vec<EditorCompletionItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        match language_id_from_path(Path::new(&request.file_path)) {
            "python" => request_ty_completions(&request),
            _ => Ok(Vec::new()),
        }
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn request_editor_code_actions(
    request: EditorHoverRequest,
) -> Result<Vec<EditorCodeAction>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        match language_id_from_path(Path::new(&request.file_path)) {
            "python" => request_ty_code_actions(&request),
            _ => Ok(Vec::new()),
        }
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn request_editor_definition(
    request: EditorHoverRequest,
) -> Result<Option<EditorLocation>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        match language_id_from_path(Path::new(&request.file_path)) {
            "python" => request_ty_definition(&request),
            _ => Ok(None),
        }
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn request_editor_references(
    request: EditorHoverRequest,
) -> Result<Vec<EditorLocation>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        match language_id_from_path(Path::new(&request.file_path)) {
            "python" => request_ty_references(&request),
            _ => Ok(Vec::new()),
        }
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn request_editor_inlay_hints(
    request: EditorHoverRequest,
) -> Result<Vec<EditorInlayHint>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        match language_id_from_path(Path::new(&request.file_path)) {
            "python" => request_ty_inlay_hints(&request),
            _ => Ok(Vec::new()),
        }
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn build_compact_context(
    request: CompactContextRequest,
) -> Result<CompactContextPayload, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = PathBuf::from(&request.root);
        let current_file = request.current_file.as_ref().map(PathBuf::from);
        let context =
            compose_compact_context(&root, current_file.as_ref(), request.content.as_deref())?;

        Ok(CompactContextPayload { context })
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn refresh_agent_health() -> Result<AgentHealthPayload, String> {
    tauri::async_runtime::spawn_blocking(|| Ok(build_agent_health_payload()))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn get_language_capabilities(
    workspace: Option<String>,
) -> Result<Vec<LanguageCapabilityStatus>, String> {
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
                ruff: probe_command("ruff").map(PathBuf::from),
                ty: probe_command("ty").map(PathBuf::from),
                rust_analyzer: probe_command("rust-analyzer").map(PathBuf::from),
                clangd: probe_command("clangd").map(PathBuf::from),
            },
            metadata,
        ))
    })
    .await
    .map_err(|error| error.to_string())?
}

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

#[tauri::command]
fn save_agent_credentials(
    request: SaveAgentCredentialsRequest,
) -> Result<AgentHealthPayload, String> {
    let mut stored = load_agent_credentials();

    merge_optional_value(&mut stored.openai_api_key, request.openai_api_key);
    merge_optional_value(&mut stored.gemini_api_key, request.gemini_api_key);
    merge_optional_value(&mut stored.google_api_key, request.google_api_key);
    merge_optional_value(
        &mut stored.google_cloud_project,
        request.google_cloud_project,
    );
    merge_optional_value(
        &mut stored.google_cloud_location,
        request.google_cloud_location,
    );
    merge_optional_value(
        &mut stored.google_application_credentials,
        request.google_application_credentials,
    );
    merge_optional_value(&mut stored.anthropic_api_key, request.anthropic_api_key);
    merge_optional_value(&mut stored.kilo_api_key, request.kilo_api_key);

    persist_agent_credentials(&stored)?;
    Ok(build_agent_health_payload())
}

#[tauri::command]
fn launch_agent_login(request: AgentLoginRequest) -> Result<String, String> {
    let stored = load_agent_credentials();

    match request.agent_id.as_str() {
        "codex" => {
            let binary = probe_command("codex").ok_or_else(|| {
                "Codex CLI is not installed. Install it first, then try signing in again."
                    .to_string()
            })?;
            spawn_external_terminal(&binary, &["login"], &stored)?;
            Ok("Opened Codex login in a new terminal window.".into())
        }
        "gemini" => {
            let binary = probe_command("gemini").ok_or_else(|| {
                "Gemini CLI is not installed. Install it first, then try signing in again."
                    .to_string()
            })?;
            spawn_external_terminal(&binary, &[], &stored)?;
            Ok("Opened Gemini CLI. Choose Sign in with Google or Use Gemini API key in the CLI window.".into())
        }
        "claude" => {
            let binary = probe_command("claude")
                .ok_or_else(|| "Claude Code CLI is not installed on PATH yet.".to_string())?;
            spawn_external_terminal(&binary, &["auth", "login"], &stored)?;
            Ok("Opened Claude Code login in a new terminal window.".into())
        }
        "kilo" => {
            let binary = probe_command("kilo")
                .ok_or_else(|| "Kilo Code CLI is not installed on PATH yet.".to_string())?;
            spawn_external_terminal(&binary, &["auth", "login"], &stored)?;
            Ok("Opened Kilo Code provider login in a new terminal window.".into())
        }
        _ => Err("Unknown agent provider.".into()),
    }
}

#[tauri::command]
fn pick_workspace_directory() -> Option<String> {
    FileDialog::new()
        .pick_folder()
        .map(|path| path_to_string(&path))
}

#[tauri::command]
fn pick_service_account_file() -> Option<String> {
    FileDialog::new()
        .add_filter("JSON", &["json"])
        .pick_file()
        .map(|path| path_to_string(&path))
}

#[tauri::command]
fn inspect_python_environment(root: String) -> Result<PythonEnvironmentStatus, String> {
    let root_path = PathBuf::from(root);
    let uv_available = probe_command("uv").is_some();
    let pyproject_exists = root_path.join("pyproject.toml").exists();
    let python_path = venv_python_path(&root_path);
    let venv_exists = python_path.exists();
    let recommended_command = if pyproject_exists {
        "uv sync".to_string()
    } else {
        "uv venv".to_string()
    };

    let summary = if !uv_available {
        "uv is not available on PATH, so automatic Python environment management is paused."
            .to_string()
    } else if pyproject_exists && venv_exists {
        "pyproject.toml and a local .venv are both present. Hematite can sync and auto-install missing imports.".to_string()
    } else if pyproject_exists {
        "pyproject.toml was found. Run uv sync to create or refresh the local environment."
            .to_string()
    } else if venv_exists {
        "A local .venv already exists. Hematite will install unresolved Python imports into that environment.".to_string()
    } else {
        "No .venv detected yet. Hematite will create one with uv when needed.".to_string()
    };

    Ok(PythonEnvironmentStatus {
        root: path_to_string(&root_path),
        uv_available,
        pyproject_exists,
        venv_exists,
        python_path: venv_exists.then(|| path_to_string(&python_path)),
        summary,
        recommended_command,
    })
}

#[tauri::command]
fn prepare_python_environment(root: String) -> Result<ProcessOutcome, String> {
    let root_path = PathBuf::from(root);
    let uv_path = probe_command("uv")
        .ok_or_else(|| "uv is not installed or not available on PATH.".to_string())?;
    let pyproject_exists = root_path.join("pyproject.toml").exists();
    let mut command = Command::new(&uv_path);
    command.current_dir(&root_path);
    hide_background_window(&mut command);

    let preview = if pyproject_exists {
        command.args(["sync"]);
        "uv sync".to_string()
    } else {
        command.args(["venv"]);
        "uv venv".to_string()
    };

    let output = command.output().map_err(|err| err.to_string())?;
    Ok(ProcessOutcome {
        success: output.status.success(),
        command: preview,
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        diagnostics: Vec::new(),
    })
}

#[tauri::command]
async fn inspect_c_family_environment(root: String) -> Result<CFamilyEnvironmentStatus, String> {
    tauri::async_runtime::spawn_blocking(move || inspect_c_family_environment_sync(&root))
        .await
        .map_err(|err| err.to_string())?
}

fn inspect_c_family_environment_sync(root: &str) -> Result<CFamilyEnvironmentStatus, String> {
    let root_path = PathBuf::from(root);
    let c_root = find_c_family_workspace_root(&root_path).unwrap_or_else(|| root_path.clone());
    let cmake_lists_exists = c_root.join("CMakeLists.txt").exists();
    let compile_commands_exists = c_root.join("compile_commands.json").exists()
        || c_root.join("build").join("compile_commands.json").exists();

    let clangd_available = probe_available_command("clangd").is_some();
    let clang_available = probe_available_command("clang").is_some();
    let clangxx_available = probe_available_command("clang++").is_some();
    let gcc_available = probe_available_command("gcc").is_some();
    let gxx_available = probe_available_command("g++").is_some();
    let msvc_cl_available = probe_available_command("cl").is_some();
    let cmake_available = probe_available_command("cmake").is_some();
    let ninja_available = probe_available_command("ninja").is_some();
    let make_available = probe_available_command("make").is_some();
    let nvcc_available = probe_available_command("nvcc").is_some();
    let cuda_gdb_available = probe_available_command("cuda-gdb").is_some();
    let lldb_available = probe_command("lldb").is_some();
    let codelldb_available = probe_command("codelldb").is_some();

    let clangd_version = command_first_line("clangd", &["--version"], &c_root);
    let clang_version = command_first_line("clang", &["--version"], &c_root);
    let gcc_version = command_first_line("gcc", &["--version"], &c_root);
    let nvcc_version = command_first_line("nvcc", &["--version"], &c_root);

    let summary = if compile_commands_exists && clangd_available {
        "compile_commands.json and clangd are available. Hematite can provide C/C++/CUDA parser semantics now, with project-aware clangd integration ready to wire next.".to_string()
    } else if compile_commands_exists {
        "compile_commands.json was found. Install clangd to enable project-aware C/C++/CUDA language service features; Hematite will use the standalone C-family parser meanwhile.".to_string()
    } else if cmake_lists_exists {
        "CMakeLists.txt was found, but no compile_commands.json is available yet. Generate one for clangd; Hematite will use the standalone C-family parser meanwhile.".to_string()
    } else {
        "No C-family compile database was found. Hematite will use the standalone C-family parser for hover, outline, and semantic coloring; add compile_commands.json or CMake metadata for clangd.".to_string()
    };

    let recommended_command = if compile_commands_exists {
        "clangd".to_string()
    } else if cmake_lists_exists {
        "cmake -S . -B build -DCMAKE_EXPORT_COMPILE_COMMANDS=ON".to_string()
    } else {
        "Create compile_commands.json or CMakeLists.txt".to_string()
    };

    Ok(CFamilyEnvironmentStatus {
        root: path_to_string(&c_root),
        cmake_lists_exists,
        compile_commands_exists,
        clangd_available,
        clang_available,
        clangxx_available,
        gcc_available,
        gxx_available,
        msvc_cl_available,
        cmake_available,
        ninja_available,
        make_available,
        nvcc_available,
        cuda_gdb_available,
        lldb_available,
        codelldb_available,
        clangd_version,
        clang_version,
        gcc_version,
        nvcc_version,
        summary,
        recommended_command,
    })
}

#[tauri::command]
async fn inspect_rust_environment(root: String) -> Result<RustEnvironmentStatus, String> {
    tauri::async_runtime::spawn_blocking(move || inspect_rust_environment_sync(&root))
        .await
        .map_err(|err| err.to_string())?
}

fn inspect_rust_environment_sync(root: &str) -> Result<RustEnvironmentStatus, String> {
    let root_path = PathBuf::from(root);
    let rust_root = find_rust_workspace_root(&root_path).unwrap_or_else(|| root_path.clone());
    let cargo_toml_exists = rust_root.join("Cargo.toml").exists();
    let rust_toolchain_file =
        rust_toolchain_file_for_root(&rust_root).map(|path| path_to_string(&path));
    let active_toolchain = command_first_line("rustup", &["show", "active-toolchain"], &rust_root)
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty());
    let installed_toolchains = command_lines("rustup", &["toolchain", "list"], &rust_root)
        .into_iter()
        .map(|line| line.replace(" (default)", "").replace(" (active)", ""))
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let installed_components =
        command_lines("rustup", &["component", "list", "--installed"], &rust_root)
            .into_iter()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
    let rustc_version = command_first_line("rustc", &["--version"], &rust_root);
    let cargo_version = command_first_line("cargo", &["--version"], &rust_root);
    let rust_analyzer_available = probe_available_command("rust-analyzer").is_some();
    let rustfmt_available = probe_available_command("rustfmt").is_some();
    let clippy_available = probe_available_command("cargo-clippy").is_some()
        || probe_available_command("clippy-driver").is_some();
    let lldb_available = probe_command("lldb").is_some();
    let codelldb_available = probe_command("codelldb").is_some();

    let summary = if !cargo_toml_exists {
        "No Cargo.toml was found at this workspace root. Hematite will use the standalone Rust parser for hover and semantic coloring; open a Cargo package or workspace to enable rust-analyzer diagnostics, Cargo checks, Clippy, tests, docs, and metadata.".to_string()
    } else if rust_analyzer_available && rustfmt_available && clippy_available {
        "Cargo, rust-analyzer, rustfmt, and Clippy are available. Hematite can provide Rust hover, semantic tokens, diagnostics, formatting, linting, builds, tests, docs, and metadata.".to_string()
    } else if rust_analyzer_available {
        "Cargo project found with rust-analyzer available. Install rustfmt and Clippy components for the full Rust IDE workflow.".to_string()
    } else {
        "Cargo project found. Install rust-analyzer plus rustfmt and Clippy components for full Rust IDE support.".to_string()
    };
    let recommended_command = if !cargo_toml_exists {
        "cargo init".to_string()
    } else if !rust_analyzer_available {
        "rustup component add rust-analyzer rustfmt clippy".to_string()
    } else if !rustfmt_available || !clippy_available {
        "rustup component add rustfmt clippy".to_string()
    } else {
        "cargo check".to_string()
    };

    Ok(RustEnvironmentStatus {
        root: path_to_string(&rust_root),
        cargo_toml_exists,
        rust_toolchain_file,
        active_toolchain,
        installed_toolchains,
        installed_components,
        rustc_version,
        cargo_version,
        rust_analyzer_available,
        rustfmt_available,
        clippy_available,
        lldb_available,
        codelldb_available,
        summary,
        recommended_command,
    })
}

#[tauri::command]
async fn execute_terminal_command(
    request: TerminalCommandRequest,
) -> Result<TerminalCommandResponse, String> {
    tauri::async_runtime::spawn_blocking(move || execute_terminal_command_blocking(request))
        .await
        .map_err(|err| err.to_string())?
}

fn execute_terminal_command_blocking(
    request: TerminalCommandRequest,
) -> Result<TerminalCommandResponse, String> {
    let command_text = request.command.trim();
    if command_text.is_empty() {
        return Err("Terminal command cannot be empty.".into());
    }

    let requested_cwd = request
        .cwd
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(detect_workspace_root().unwrap_or_else(|_| ".".into())));

    let resolved_cwd = fs::canonicalize(&requested_cwd).unwrap_or(requested_cwd);
    let stored = load_agent_credentials();
    let (success, raw_output) = run_terminal_command_in_pty(command_text, &resolved_cwd, &stored)?;
    let cleaned_output = strip_terminal_control_sequences(&raw_output);
    let (stdout, cwd) = split_terminal_output(&cleaned_output, &resolved_cwd);

    Ok(TerminalCommandResponse {
        success,
        command: command_text.to_string(),
        stdout,
        stderr: String::new(),
        cwd,
    })
}

#[tauri::command]
fn start_terminal_session(
    app: AppHandle,
    request: TerminalSessionStartRequest,
) -> Result<TerminalSessionStartResponse, String> {
    let requested_cwd = request
        .cwd
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(detect_workspace_root().unwrap_or_else(|_| ".".into())));
    let resolved_cwd = fs::canonicalize(&requested_cwd).unwrap_or(requested_cwd);
    let stored = load_agent_credentials();

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: TERMINAL_PTY_ROWS,
            cols: TERMINAL_PTY_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| format!("Could not open terminal PTY. {err}"))?;

    let (mut command, shell) = terminal_session_shell_command(&resolved_cwd);
    apply_agent_env_to_pty(&mut command, &stored);
    apply_workspace_env_to_pty(&mut command, &resolved_cwd);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|err| format!("Could not read from terminal PTY. {err}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|err| format!("Could not write to terminal PTY. {err}"))?;
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|err| format!("Could not start terminal shell. {err}"))?;
    drop(pair.slave);

    let registry = terminal_sessions();
    let session_number = registry.next_session_id.fetch_add(1, Ordering::Relaxed) + 1;
    let session_id = format!("terminal-{session_number}");
    let child = Arc::new(Mutex::new(child));
    let writer = Arc::new(Mutex::new(writer));

    {
        let mut sessions = registry
            .sessions
            .lock()
            .map_err(|_| "Terminal session registry lock was poisoned.".to_string())?;
        sessions.insert(
            session_id.clone(),
            TerminalSession {
                master: pair.master,
                writer: Arc::clone(&writer),
                child: Arc::clone(&child),
            },
        );
    }

    let output_app = app.clone();
    let output_session_id = session_id.clone();
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(len) => {
                    let data = String::from_utf8_lossy(&buffer[..len]).to_string();
                    let _ = output_app.emit(
                        "hematite://terminal",
                        TerminalSessionEvent {
                            session_id: output_session_id.clone(),
                            kind: "output".into(),
                            data,
                            success: None,
                        },
                    );
                }
                Err(_) => break,
            }
        }
    });

    let exit_app = app;
    let exit_session_id = session_id.clone();
    thread::spawn(move || {
        loop {
            let success = match child.lock() {
                Ok(mut child) => match child.try_wait() {
                    Ok(status) => status.map(|status| status.success()),
                    Err(_) => Some(false),
                },
                Err(_) => Some(false),
            };

            if let Some(success) = success {
                let _ = terminal_sessions()
                    .sessions
                    .lock()
                    .map(|mut sessions| sessions.remove(&exit_session_id));
                let _ = exit_app.emit(
                    "hematite://terminal",
                    TerminalSessionEvent {
                        session_id: exit_session_id,
                        kind: "exit".into(),
                        data: String::new(),
                        success: Some(success),
                    },
                );
                break;
            }

            thread::sleep(Duration::from_millis(100));
        }
    });

    Ok(TerminalSessionStartResponse {
        session_id,
        cwd: path_to_string(&resolved_cwd),
        shell: shell.into(),
    })
}

#[tauri::command]
fn write_terminal_session_input(request: TerminalSessionInputRequest) -> Result<(), String> {
    write_to_terminal_session(&request.session_id, &request.input)
}

#[tauri::command]
fn write_terminal_session_command(request: TerminalSessionCommandRequest) -> Result<(), String> {
    let command_text = request.command.trim();
    if command_text.is_empty() {
        return Err("Terminal command cannot be empty.".into());
    }
    write_to_terminal_session(
        &request.session_id,
        &terminal_session_command_input(command_text),
    )
}

#[tauri::command]
fn stop_terminal_session(request: TerminalSessionStopRequest) -> Result<(), String> {
    let session = terminal_sessions()
        .sessions
        .lock()
        .map_err(|_| "Terminal session registry lock was poisoned.".to_string())?
        .remove(&request.session_id);

    if let Some(session) = session {
        let _ = session.child.lock().map(|mut child| child.kill());
    }

    Ok(())
}

#[tauri::command]
fn resize_terminal_session(request: TerminalSessionResizeRequest) -> Result<(), String> {
    let sessions = terminal_sessions()
        .sessions
        .lock()
        .map_err(|_| "Terminal session registry lock was poisoned.".to_string())?;
    let session = sessions
        .get(&request.session_id)
        .ok_or_else(|| "Terminal session is no longer active.".to_string())?;
    session
        .master
        .resize(PtySize {
            rows: request.rows.max(1),
            cols: request.cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| format!("Could not resize terminal PTY. {err}"))
}

#[tauri::command]
fn start_codeshare_session(request: CodeShareSessionRequest) -> Result<CodeShareSession, String> {
    let mut state = codeshare_state()
        .lock()
        .map_err(|_| "CodeShare state lock was poisoned.".to_string())?;
    Ok(start_codeshare_session_in_state(
        &mut state,
        request,
        unix_timestamp_seconds(),
    ))
}

#[tauri::command]
fn join_codeshare_session(request: CodeShareJoinRequest) -> Result<CodeShareSession, String> {
    let mut state = codeshare_state()
        .lock()
        .map_err(|_| "CodeShare state lock was poisoned.".to_string())?;
    join_codeshare_session_in_state(&mut state, request, unix_timestamp_seconds())
}

#[tauri::command]
async fn run_agent(request: AgentRunRequest) -> Result<AgentRunResponse, String> {
    tauri::async_runtime::spawn_blocking(move || run_agent_blocking(request))
        .await
        .map_err(|error| error.to_string())?
}

fn run_agent_blocking(request: AgentRunRequest) -> Result<AgentRunResponse, String> {
    let root = PathBuf::from(&request.root);
    let context = if request.include_compact_context {
        Some(compose_compact_context(
            &root,
            request.current_file.as_ref().map(PathBuf::from).as_ref(),
            request.content.as_deref(),
        )?)
    } else {
        None
    };

    let prompt = if let Some(context) = &context {
        let mut prompt = request.prompt.trim().to_string();
        if !prompt.is_empty() {
            prompt.push_str("\n\n");
        }
        prompt.push_str("Compact workspace context:\n");
        prompt.push_str(context);
        prompt
    } else {
        request.prompt.trim().to_string()
    };

    let model_args = agent_args_with_selected_model(
        &request.binary,
        &request.args,
        request.model.as_deref(),
        request.permission_level.as_deref(),
    );
    let resolved_args = model_args
        .iter()
        .map(|value| value.replace("{prompt}", &prompt))
        .collect::<Vec<_>>();

    let stored = load_agent_credentials();
    let mut prepared = prepare_cli_command(&request.binary, &resolved_args);
    prepared.command.current_dir(&root);
    apply_agent_env(&mut prepared.command, &stored);
    apply_workspace_env(&mut prepared.command, &root);
    hide_background_window(&mut prepared.command);

    let output = if request.stdin_prompt {
        prepared.command.stdin(Stdio::piped());
        let mut child = prepared.command.spawn().map_err(|err| {
            format!(
                "Failed to start `{}`. Make sure the CLI is installed and available on PATH. {}",
                request.binary, err
            )
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt.as_bytes()).map_err(|err| {
                format!(
                    "Failed to write prompt to `{}` stdin. {}",
                    request.binary, err
                )
            })?;
        }

        child.wait_with_output().map_err(|err| {
            format!(
                "Failed while waiting for `{}` to finish. {}",
                request.binary, err
            )
        })?
    } else {
        prepared.command.output().map_err(|err| {
            format!(
                "Failed to start `{}`. Make sure the CLI is installed and available on PATH. {}",
                request.binary, err
            )
        })?
    };

    Ok(AgentRunResponse {
        success: output.status.success(),
        command: prepared.preview,
        prompt,
        stdout: bounded_process_output(&output.stdout),
        stderr: bounded_process_output(&output.stderr),
        context,
    })
}

fn codeshare_state() -> &'static Mutex<CodeShareState> {
    CODESHARE_STATE.get_or_init(|| Mutex::new(CodeShareState::default()))
}

fn unix_timestamp_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn start_codeshare_session_in_state(
    state: &mut CodeShareState,
    request: CodeShareSessionRequest,
    now: u64,
) -> CodeShareSession {
    state.next_session_id = state.next_session_id.saturating_add(1);
    let sequence = state.next_session_id;
    let invite_code = format!("CS-{now:X}-{sequence:04X}");
    let session_id = format!("codeshare-{now:x}-{sequence:x}");
    let session = CodeShareSession {
        session_id,
        invite_link: format!("hematite://codeshare/{invite_code}"),
        invite_code: invite_code.clone(),
        title: normalize_codeshare_title(&request.title),
        root: request.root.trim().to_string(),
        active_file: request
            .active_file
            .and_then(|value| normalize_optional_codeshare_value(&value)),
        agent_label: normalize_codeshare_label(&request.agent_label, "Agent"),
        permission_level: normalize_codeshare_label(&request.permission_level, "ask"),
        status: "hosting".into(),
        participant_count: 1,
        created_at: now,
        updated_at: now,
    };

    state
        .sessions_by_invite
        .insert(invite_code, session.clone());
    session
}

fn join_codeshare_session_in_state(
    state: &mut CodeShareState,
    request: CodeShareJoinRequest,
    now: u64,
) -> Result<CodeShareSession, String> {
    let invite_code = normalize_codeshare_invite_code(&request.invite_code);
    let session = state
        .sessions_by_invite
        .get_mut(&invite_code)
        .ok_or_else(|| "CodeShare invite was not found.".to_string())?;

    session.participant_count = session.participant_count.saturating_add(1);
    session.updated_at = now;

    let mut joined = session.clone();
    joined.status = "joined".into();
    Ok(joined)
}

fn normalize_codeshare_title(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "CodeShare session".into()
    } else {
        trimmed.to_string()
    }
}

fn normalize_codeshare_label(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.into()
    } else {
        trimmed.to_string()
    }
}

fn normalize_optional_codeshare_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn normalize_codeshare_invite_code(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("hematite://codeshare/")
        .trim_end_matches('/')
        .to_ascii_uppercase()
}

#[tauri::command]
fn start_codex_turn(
    app: tauri::AppHandle,
    request: CodexTurnRequest,
) -> Result<CodexTurnResponse, String> {
    let root = PathBuf::from(&request.root);
    let context = if request.include_compact_context {
        Some(compose_compact_context(
            &root,
            request.current_file.as_ref().map(PathBuf::from).as_ref(),
            request.content.as_deref(),
        )?)
    } else {
        None
    };

    let prompt = if let Some(context) = &context {
        let mut prompt = request.prompt.trim().to_string();
        if !prompt.is_empty() {
            prompt.push_str("\n\n");
        }
        prompt.push_str("Compact workspace context:\n");
        prompt.push_str(context);
        prompt
    } else {
        request.prompt.trim().to_string()
    };

    let root_string = path_to_string(&root);
    let state = codex_app_server_state();
    let mut bridge = state
        .lock()
        .map_err(|_| "Codex bridge lock was poisoned.".to_string())?;
    let session = ensure_codex_app_server_session(&mut bridge, &app, &root_string)?;

    ensure_codex_initialized(session)?;
    let fallback_model = codex_model_override();
    let selected_model =
        codex_model_for_request(request.model.as_deref(), fallback_model.as_deref());
    let permission_level = resolved_agent_permission_level(
        request.permission_level.as_deref(),
        AgentPermissionLevel::Ask,
    );
    let thread_id = ensure_codex_thread(
        session,
        &root_string,
        selected_model.as_deref(),
        permission_level,
    )?;
    let codex_settings = codex_execution_settings_for_permission_level(permission_level);
    let mut turn_params = json!({
        "threadId": thread_id,
        "cwd": root_string,
        "approvalPolicy": codex_settings.approval_policy,
        "input": [
            {
                "type": "text",
                "text": prompt,
                "text_elements": [],
            }
        ],
    });
    apply_codex_model_param(&mut turn_params, selected_model.as_deref(), None);

    let turn_response =
        codex_send_request(session, "turn/start", turn_params, Duration::from_secs(20))?;

    let turn_id = turn_response
        .get("turn")
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| "Codex app-server did not return a turn id.".to_string())?
        .to_string();

    if let Ok(mut shared) = session.shared.lock() {
        shared.active_turn_id = Some(turn_id.clone());
    }

    Ok(CodexTurnResponse {
        thread_id,
        turn_id,
        prompt,
        context,
    })
}

#[tauri::command]
fn respond_to_codex_approval(request: CodexApprovalResponseRequest) -> Result<(), String> {
    let state = codex_app_server_state();
    let mut bridge = state
        .lock()
        .map_err(|_| "Codex bridge lock was poisoned.".to_string())?;
    let session = bridge
        .session
        .as_mut()
        .ok_or_else(|| "Codex is not running yet.".to_string())?;

    codex_respond_to_server_request(session, request)
}

#[tauri::command]
fn reset_codex_session(request: CodexResetRequest) -> Result<CodexResetResponse, String> {
    let state = codex_app_server_state();
    let mut bridge = state
        .lock()
        .map_err(|_| "Codex bridge lock was poisoned.".to_string())?;
    let mut should_drop_session = false;

    if let Some(session) = bridge.session.as_mut() {
        let mut restart = false;
        if let Ok(mut shared) = session.shared.lock() {
            if shared.active_turn_id.is_some() {
                return Err(
                    "Wait for the current Codex turn to finish before starting a new chat.".into(),
                );
            }

            shared.current_thread_id = None;
            shared.current_thread_model = None;
            shared.current_thread_permission_level = None;
            shared.pending_server_requests.clear();
            if shared.current_root != request.root {
                restart = true;
            }
        }

        if restart {
            dispose_codex_session(session);
            should_drop_session = true;
        }
    }

    if should_drop_session {
        bridge.session = None;
    }

    Ok(CodexResetResponse { reset: true })
}

#[tauri::command]
fn start_gemini_turn(
    app: tauri::AppHandle,
    request: GeminiTurnRequest,
) -> Result<GeminiTurnResponse, String> {
    let root = PathBuf::from(&request.root);
    let context = if request.include_compact_context {
        Some(compose_compact_context(
            &root,
            request.current_file.as_ref().map(PathBuf::from).as_ref(),
            request.content.as_deref(),
        )?)
    } else {
        None
    };

    let prompt = if let Some(context) = &context {
        let mut prompt = request.prompt.trim().to_string();
        if !prompt.is_empty() {
            prompt.push_str("\n\n");
        }
        prompt.push_str("Compact workspace context:\n");
        prompt.push_str(context);
        prompt
    } else {
        request.prompt.trim().to_string()
    };

    let root_string = path_to_string(&root);
    let selected_model = normalized_agent_model(request.model.as_deref());
    let permission_level = resolved_agent_permission_level(
        request.permission_level.as_deref(),
        AgentPermissionLevel::Ask,
    );
    let (session_id, stdin, shared) = {
        let state = gemini_acp_state();
        let mut bridge = state
            .lock()
            .map_err(|_| "Gemini bridge lock was poisoned.".to_string())?;
        let session = ensure_gemini_acp_session(
            &mut bridge,
            &app,
            &root_string,
            selected_model.as_deref(),
            permission_level,
        )?;

        ensure_gemini_initialized(session)?;
        let session_id = ensure_gemini_chat_session(session, &root_string)?;
        let stdin = Arc::clone(&session.stdin);
        let shared = Arc::clone(&session.shared);
        if let Ok(mut shared_state) = shared.lock() {
            shared_state.prompt_in_progress = true;
        }
        (session_id, stdin, shared)
    };

    let response = gemini_send_request_with_handles(
        &stdin,
        &shared,
        "session/prompt",
        json!({
            "sessionId": session_id,
            "prompt": [
                {
                    "type": "text",
                    "text": prompt,
                }
            ],
        }),
        Duration::from_secs(60 * 20),
    );

    if let Ok(mut shared_state) = shared.lock() {
        shared_state.prompt_in_progress = false;
    }

    let response = response?;
    let stop_reason = response
        .get("stopReason")
        .and_then(Value::as_str)
        .unwrap_or("end_turn")
        .to_string();

    emit_gemini_frontend_event(
        &app,
        GeminiFrontendEvent::PromptCompleted {
            session_id: session_id.clone(),
            success: true,
            stop_reason: stop_reason.clone(),
            error: None,
        },
    );

    Ok(GeminiTurnResponse {
        session_id,
        prompt,
        context,
        stop_reason,
    })
}

#[tauri::command]
fn respond_to_gemini_approval(
    app: tauri::AppHandle,
    request: GeminiApprovalResponseRequest,
) -> Result<(), String> {
    let state = gemini_acp_state();
    let mut bridge = state
        .lock()
        .map_err(|_| "Gemini bridge lock was poisoned.".to_string())?;
    let session = bridge
        .session
        .as_mut()
        .ok_or_else(|| "Gemini is not running yet.".to_string())?;

    let request_id = request.request_id.clone();
    gemini_respond_to_permission_request(session, request)?;
    emit_gemini_frontend_event(&app, GeminiFrontendEvent::ApprovalResolved { request_id });
    Ok(())
}

#[tauri::command]
fn reset_gemini_session(request: GeminiResetRequest) -> Result<GeminiResetResponse, String> {
    let state = gemini_acp_state();
    let mut bridge = state
        .lock()
        .map_err(|_| "Gemini bridge lock was poisoned.".to_string())?;
    let mut should_drop_session = false;

    if let Some(session) = bridge.session.as_mut() {
        let mut restart = false;
        if let Ok(mut shared) = session.shared.lock() {
            if shared.prompt_in_progress {
                return Err(
                    "Wait for the current Gemini turn to finish before starting a new chat.".into(),
                );
            }

            shared.current_session_id = None;
            shared.pending_requests.clear();
            if shared.current_root != request.root {
                restart = true;
            }
        }

        if restart {
            dispose_gemini_session(session);
            should_drop_session = true;
        }
    }

    if should_drop_session {
        bridge.session = None;
    }

    Ok(GeminiResetResponse { reset: true })
}

fn ty_lsp_state() -> &'static Mutex<TyLspState> {
    TY_LSP.get_or_init(|| Mutex::new(TyLspState::default()))
}

fn ensure_ty_lsp_session<'a>(
    bridge: &'a mut TyLspState,
    root: &Path,
) -> Result<&'a mut TyLspSession, String> {
    let root_string = path_to_string(root);
    let python_environment = ty_python_environment_for_root(root);
    let mut needs_restart = bridge.session.is_none();

    if let Some(session) = bridge.session.as_mut() {
        let exited = session
            .child
            .try_wait()
            .map_err(|err| format!("Could not inspect ty language server. {}", err))?
            .is_some();
        let (current_root, current_python_environment, failed) = {
            let shared = session
                .shared
                .lock()
                .map_err(|_| "ty language server state lock was poisoned.".to_string())?;
            (
                shared.current_root.clone(),
                shared.python_environment.clone(),
                shared.failed,
            )
        };

        needs_restart = exited
            || failed
            || current_root != root_string
            || current_python_environment != python_environment;
    }

    if needs_restart {
        if let Some(session) = bridge.session.as_mut() {
            dispose_ty_lsp_session(session);
        }
        bridge.session = Some(spawn_ty_lsp_session(&root_string)?);
    }

    bridge
        .session
        .as_mut()
        .ok_or_else(|| "ty language server did not start.".to_string())
}

fn spawn_ty_lsp_session(root: &str) -> Result<TyLspSession, String> {
    let args = vec!["server".to_string()];
    let mut prepared = prepare_cli_command("ty", &args);
    prepared.command.stdin(Stdio::piped());
    prepared.command.stdout(Stdio::piped());
    prepared.command.stderr(Stdio::piped());
    prepared.command.current_dir(root);
    apply_workspace_env(&mut prepared.command, Path::new(root));
    hide_background_window(&mut prepared.command);

    let mut child = prepared.command.spawn().map_err(|err| {
        format!(
            "Failed to start `ty server`. Make sure ty is bundled or available on PATH. {}",
            err
        )
    })?;

    let stdin = Arc::new(Mutex::new(
        child
            .stdin
            .take()
            .ok_or_else(|| "ty server did not expose stdin.".to_string())?,
    ));
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "ty server did not expose stdout.".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "ty server did not expose stderr.".to_string())?;
    let shared = Arc::new(Mutex::new(TyLspSharedState::new(
        root.to_string(),
        ty_python_environment_for_root(Path::new(root)),
    )));

    spawn_ty_lsp_stdout_reader(shared.clone(), stdin.clone(), stdout);
    spawn_ty_lsp_stderr_reader(stderr);

    Ok(TyLspSession {
        child,
        stdin,
        shared,
    })
}

fn dispose_ty_lsp_session(session: &mut TyLspSession) {
    let _ = session.child.kill();
    let _ = session.child.wait();
}

fn reset_ty_lsp_session() {
    if let Ok(mut bridge) = ty_lsp_state().lock() {
        if let Some(session) = bridge.session.as_mut() {
            dispose_ty_lsp_session(session);
        }
        bridge.session = None;
    }
}

fn spawn_ty_lsp_stdout_reader(
    shared: Arc<Mutex<TyLspSharedState>>,
    stdin: Arc<Mutex<ChildStdin>>,
    stdout: impl Read + Send + 'static,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let Ok(Some(message)) = read_lsp_message(&mut reader) else {
                break;
            };

            if let Some(method) = message.get("method").and_then(Value::as_str) {
                if method == "textDocument/publishDiagnostics" {
                    handle_ty_published_diagnostics(&shared, &message);
                    continue;
                }

                if message.get("id").is_some() {
                    if let Some(response) = ty_server_request_response(&shared, &message) {
                        let _ = send_lsp_json(&stdin, &response);
                    }
                }
                continue;
            }

            if let Some(id) = message.get("id").cloned() {
                handle_ty_lsp_response(&shared, id, &message);
            }
        }
    });
}

fn handle_ty_published_diagnostics(shared: &Arc<Mutex<TyLspSharedState>>, message: &Value) {
    let Some(params) = message.get("params") else {
        return;
    };
    let Some(uri) = params
        .get("uri")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return;
    };
    let diagnostics = params
        .get("diagnostics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let version = params
        .get("version")
        .and_then(Value::as_i64)
        .map(|value| value as i32);

    if let Ok(mut state) = shared.lock() {
        state.published_diagnostics.insert(
            uri,
            TyPublishedDiagnostics {
                version,
                diagnostics,
            },
        );
    }
}

fn spawn_ty_lsp_stderr_reader(stderr: impl Read + Send + 'static) {
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if line.is_err() {
                break;
            }
        }
    });
}

fn handle_ty_lsp_response(shared: &Arc<Mutex<TyLspSharedState>>, id: Value, message: &Value) {
    let Some(id_key) = request_id_key(&id) else {
        return;
    };

    let sender = shared
        .lock()
        .ok()
        .and_then(|mut state| state.pending_responses.remove(&id_key));

    if let Some(sender) = sender {
        if let Some(result) = message.get("result") {
            let _ = sender.send(Ok(result.clone()));
        } else {
            let error = message
                .get("error")
                .map(json_error_message)
                .unwrap_or_else(|| "ty returned an empty response.".into());
            let _ = sender.send(Err(error));
        }
    }
}

fn ty_server_request_response(
    shared: &Arc<Mutex<TyLspSharedState>>,
    message: &Value,
) -> Option<Value> {
    let root = shared
        .lock()
        .ok()
        .map(|state| state.current_root.clone())
        .unwrap_or_default();
    ty_server_request_response_for_root(&root, message)
}

fn ty_server_request_response_for_root(root: &str, message: &Value) -> Option<Value> {
    let id = message.get("id")?.clone();
    let method = message.get("method").and_then(Value::as_str)?;
    let result = match method {
        "workspace/configuration" => {
            let settings = ty_editor_settings_for_root(root);
            let items = message
                .get("params")
                .and_then(|value| value.get("items"))
                .and_then(Value::as_array);
            Value::Array(
                items
                    .map(|items| {
                        items
                            .iter()
                            .map(|item| ty_configuration_item_value(item, &settings))
                            .collect()
                    })
                    .unwrap_or_default(),
            )
        }
        "workspace/workspaceFolders" => Value::Array(Vec::new()),
        "client/registerCapability"
        | "client/unregisterCapability"
        | "window/workDoneProgress/create" => Value::Null,
        _ => Value::Null,
    };

    Some(json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    }))
}

fn ty_editor_settings_for_root(root: &str) -> Value {
    let mut configuration = json!({});

    if let Some(python) = ty_python_environment_for_root(Path::new(root)) {
        configuration = json!({
            "environment": {
                "python": python,
            }
        });
    }

    json!({
        "configuration": configuration,
        "disableLanguageServices": false,
    })
}

fn ty_configuration_item_value(item: &Value, settings: &Value) -> Value {
    match item.get("section").and_then(Value::as_str) {
        None | Some("ty") => settings.clone(),
        Some("ty.configuration") => settings
            .get("configuration")
            .cloned()
            .unwrap_or_else(|| json!({})),
        Some("ty.configuration.environment") => settings
            .get("configuration")
            .and_then(|value| value.get("environment"))
            .cloned()
            .unwrap_or_else(|| json!({})),
        Some("ty.configuration.environment.python") => settings
            .get("configuration")
            .and_then(|value| value.get("environment"))
            .and_then(|value| value.get("python"))
            .cloned()
            .unwrap_or(Value::Null),
        Some("ty.disableLanguageServices") => json!(false),
        _ => json!({}),
    }
}

fn ty_python_environment_for_root(root: &Path) -> Option<String> {
    let venv_python = venv_python_path(root);
    if venv_python.exists() {
        return Some(path_to_string(&venv_python));
    }

    if let Some(value) = env::var_os("VIRTUAL_ENV").filter(|value| !value.is_empty()) {
        return Some(path_to_string(&PathBuf::from(value)));
    }

    if let Some(value) = env::var_os("CONDA_PREFIX").filter(|value| !value.is_empty()) {
        return Some(path_to_string(&PathBuf::from(value)));
    }

    probe_command("python").or_else(|| probe_command("python3"))
}

fn read_lsp_message(reader: &mut BufReader<impl Read>) -> Result<Option<Value>, String> {
    let mut content_length = None;

    loop {
        let mut header = String::new();
        let read = reader
            .read_line(&mut header)
            .map_err(|err| err.to_string())?;
        if read == 0 {
            return Ok(None);
        }

        let trimmed = header.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }

        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = value.trim().parse::<usize>().ok();
        }
    }

    let Some(length) = content_length else {
        return Err("ty sent an LSP message without Content-Length.".into());
    };

    let mut payload = vec![0u8; length];
    reader
        .read_exact(&mut payload)
        .map_err(|err| err.to_string())?;
    serde_json::from_slice::<Value>(&payload)
        .map(Some)
        .map_err(|err| err.to_string())
}

fn ty_send_request(
    session: &TyLspSession,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value, String> {
    let (tx, rx) = mpsc::channel();
    let (request_id, message) = {
        let mut shared = session
            .shared
            .lock()
            .map_err(|_| "ty language server state lock was poisoned.".to_string())?;
        let request_id = shared.next_request_id;
        shared.next_request_id += 1;
        shared.pending_responses.insert(request_id.to_string(), tx);
        (
            request_id,
            json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": params,
            }),
        )
    };

    if let Err(error) = send_lsp_json(&session.stdin, &message) {
        if let Ok(mut shared) = session.shared.lock() {
            shared.pending_responses.remove(&request_id.to_string());
        }
        return Err(error);
    }

    match rx.recv_timeout(timeout) {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) => Err(error),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            if let Ok(mut shared) = session.shared.lock() {
                shared.pending_responses.remove(&request_id.to_string());
                shared.failed = true;
            }
            Err(format!(
                "ty did not answer `{}` within {} seconds.",
                method,
                timeout.as_secs()
            ))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            if let Ok(mut shared) = session.shared.lock() {
                shared.failed = true;
            }
            Err(format!(
                "The ty language server closed while waiting for `{}`.",
                method
            ))
        }
    }
}

fn ty_send_notification(session: &TyLspSession, method: &str, params: Value) -> Result<(), String> {
    send_lsp_json(
        &session.stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }),
    )
}

fn send_lsp_json(stdin: &Arc<Mutex<ChildStdin>>, message: &Value) -> Result<(), String> {
    let serialized = serde_json::to_vec(message).map_err(|err| err.to_string())?;
    let mut handle = stdin
        .lock()
        .map_err(|_| "LSP stdin lock was poisoned.".to_string())?;
    write!(handle, "Content-Length: {}\r\n\r\n", serialized.len())
        .map_err(|err| err.to_string())?;
    handle
        .write_all(&serialized)
        .map_err(|err| err.to_string())?;
    handle.flush().map_err(|err| err.to_string())
}

fn ensure_ty_initialized(session: &TyLspSession, root: &Path) -> Result<(), String> {
    let needs_initialize = !session
        .shared
        .lock()
        .map_err(|_| "ty language server state lock was poisoned.".to_string())?
        .initialized;

    if !needs_initialize {
        return Ok(());
    }

    let root_uri = path_to_file_uri(root);
    let response = ty_send_request(
        session,
        "initialize",
        json!({
            "processId": null,
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "hover": {
                        "dynamicRegistration": false,
                        "contentFormat": ["markdown", "plaintext"]
                    },
                    "signatureHelp": {
                        "dynamicRegistration": false,
                        "signatureInformation": {
                            "documentationFormat": ["markdown", "plaintext"],
                            "parameterInformation": {
                                "labelOffsetSupport": true
                            },
                            "activeParameterSupport": true
                        },
                        "contextSupport": true
                    },
                    "semanticTokens": {
                        "dynamicRegistration": false,
                        "requests": { "full": true, "range": false },
                        "tokenTypes": LSP_SEMANTIC_TOKEN_TYPES,
                        "tokenModifiers": [],
                        "formats": ["relative"],
                        "overlappingTokenSupport": false,
                        "multilineTokenSupport": true
                    },
                    "diagnostic": { "dynamicRegistration": false }
                },
                "workspace": {
                    "workspaceFolders": true,
                    "configuration": true
                }
            },
            "workspaceFolders": [{
                "uri": root_uri,
                "name": root.file_name().and_then(|value| value.to_str()).unwrap_or("workspace")
            }]
        }),
        Duration::from_secs(5),
    )?;

    if let Some(token_types) = response
        .get("capabilities")
        .and_then(|value| value.get("semanticTokensProvider"))
        .and_then(|value| value.get("legend"))
        .and_then(|value| value.get("tokenTypes"))
        .and_then(Value::as_array)
    {
        if let Ok(mut shared) = session.shared.lock() {
            shared.token_types = token_types
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect();
        }
    }

    let pull_diagnostics = response
        .get("capabilities")
        .and_then(|value| value.get("diagnosticProvider"))
        .is_some();
    if let Ok(mut shared) = session.shared.lock() {
        shared.pull_diagnostics = pull_diagnostics;
    }

    ty_send_notification(session, "initialized", json!({}))?;
    if let Ok(mut shared) = session.shared.lock() {
        shared.initialized = true;
    }
    Ok(())
}

fn sync_ty_document(
    session: &TyLspSession,
    path: &Path,
    content: &str,
) -> Result<(String, i32), String> {
    let uri = path_to_file_uri(path);
    let (version, already_synced) = {
        let mut shared = session
            .shared
            .lock()
            .map_err(|_| "ty language server state lock was poisoned.".to_string())?;
        let (version, already_synced) = {
            let entry = shared.synced_documents.entry(uri.clone()).or_insert(0);
            let already_synced = *entry > 0;
            *entry += 1;
            (*entry, already_synced)
        };
        shared.published_diagnostics.remove(&uri);
        (version, already_synced)
    };

    if already_synced {
        ty_send_notification(
            session,
            "textDocument/didChange",
            json!({
                "textDocument": {
                    "uri": uri,
                    "version": version,
                },
                "contentChanges": [{
                    "text": content,
                }]
            }),
        )?;
    } else {
        ty_send_notification(
            session,
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "python",
                    "version": version,
                    "text": content,
                }
            }),
        )?;
    }

    Ok((uri, version))
}

fn ty_semantic_tokens_for_document(
    root: &Path,
    path: &Path,
    content: &str,
) -> Result<EditorSemanticsPayload, String> {
    if language_id_from_path(path) != "python" {
        return Ok(EditorSemanticsPayload::default());
    }

    let state = ty_lsp_state();
    let mut bridge = state
        .lock()
        .map_err(|_| "ty language server bridge lock was poisoned.".to_string())?;
    let session = ensure_ty_lsp_session(&mut bridge, root)?;
    ensure_ty_initialized(session, root)?;
    let (uri, _) = sync_ty_document(session, path, content)?;

    let response = ty_send_request(
        session,
        "textDocument/semanticTokens/full",
        json!({
            "textDocument": {
                "uri": uri,
            }
        }),
        Duration::from_secs(2),
    )?;

    let token_types = session
        .shared
        .lock()
        .map_err(|_| "ty language server state lock was poisoned.".to_string())?
        .token_types
        .clone();
    let tokens = decode_lsp_semantic_tokens(&response, &token_types);
    Ok(EditorSemanticsPayload {
        tokens,
        hover_items: Vec::new(),
    })
}

fn request_ty_hover(request: &EditorHoverRequest) -> Result<Option<HoverItem>, String> {
    let root = PathBuf::from(&request.root);
    let path = PathBuf::from(&request.file_path);
    let state = ty_lsp_state();
    let mut bridge = state
        .lock()
        .map_err(|_| "ty language server bridge lock was poisoned.".to_string())?;
    let session = ensure_ty_lsp_session(&mut bridge, &root)?;
    ensure_ty_initialized(session, &root)?;
    let (uri, _) = sync_ty_document(session, &path, &request.source)?;

    let response = ty_send_request(
        session,
        "textDocument/hover",
        json!({
            "textDocument": { "uri": uri },
            "position": {
                "line": request.line.saturating_sub(1),
                "character": request.column.saturating_sub(1),
            }
        }),
        Duration::from_secs(2),
    )?;

    let hover = hover_item_from_lsp(&response, request.line, request.column);
    let signature = request_ty_signature_help(session, &uri, request).unwrap_or(None);

    Ok(merge_hover_and_signature_help(hover, signature))
}

fn request_ty_signature_help(
    session: &TyLspSession,
    uri: &str,
    request: &EditorHoverRequest,
) -> Result<Option<HoverItem>, String> {
    let Some(call) = call_signature_request_position(&request.source, request.line, request.column)
    else {
        return Ok(None);
    };

    let response = ty_send_request(
        session,
        "textDocument/signatureHelp",
        json!({
            "textDocument": { "uri": uri },
            "position": {
                "line": call.lsp_line,
                "character": call.lsp_character,
            }
        }),
        Duration::from_secs(2),
    )?;

    Ok(signature_help_item_from_lsp(&response, &call))
}

fn request_ty_completions(
    request: &EditorHoverRequest,
) -> Result<Vec<EditorCompletionItem>, String> {
    let response = request_ty_lsp_at_position(request, "textDocument/completion", json!({}))?;
    Ok(parse_lsp_completion_items(&response))
}

fn request_ty_code_actions(request: &EditorHoverRequest) -> Result<Vec<EditorCodeAction>, String> {
    let response = request_ty_lsp_at_position(
        request,
        "textDocument/codeAction",
        json!({
            "range": lsp_point_range(request.line, request.column),
            "context": {
                "diagnostics": [],
                "only": ["quickfix", "refactor", "source"]
            }
        }),
    )?;
    Ok(parse_lsp_code_actions(&response))
}

fn request_ty_definition(request: &EditorHoverRequest) -> Result<Option<EditorLocation>, String> {
    let response = request_ty_lsp_at_position(request, "textDocument/definition", json!({}))?;
    Ok(parse_lsp_locations(&response).into_iter().next())
}

fn request_ty_references(request: &EditorHoverRequest) -> Result<Vec<EditorLocation>, String> {
    let response = request_ty_lsp_at_position(
        request,
        "textDocument/references",
        json!({
            "context": {
                "includeDeclaration": true
            }
        }),
    )?;
    Ok(parse_lsp_locations(&response))
}

fn request_ty_inlay_hints(request: &EditorHoverRequest) -> Result<Vec<EditorInlayHint>, String> {
    let root = PathBuf::from(&request.root);
    let path = PathBuf::from(&request.file_path);
    let state = ty_lsp_state();
    let mut bridge = state
        .lock()
        .map_err(|_| "ty language server bridge lock was poisoned.".to_string())?;
    let session = ensure_ty_lsp_session(&mut bridge, &root)?;
    ensure_ty_initialized(session, &root)?;
    let (uri, _) = sync_ty_document(session, &path, &request.source)?;
    let response = ty_send_request(
        session,
        "textDocument/inlayHint",
        json!({
            "textDocument": { "uri": uri },
            "range": full_lsp_range(&request.source)
        }),
        Duration::from_secs(2),
    )?;
    Ok(parse_lsp_inlay_hints(&response))
}

fn request_ty_lsp_at_position(
    request: &EditorHoverRequest,
    method: &str,
    mut extra_params: Value,
) -> Result<Value, String> {
    let root = PathBuf::from(&request.root);
    let path = PathBuf::from(&request.file_path);
    let state = ty_lsp_state();
    let mut bridge = state
        .lock()
        .map_err(|_| "ty language server bridge lock was poisoned.".to_string())?;
    let session = ensure_ty_lsp_session(&mut bridge, &root)?;
    ensure_ty_initialized(session, &root)?;
    let (uri, _) = sync_ty_document(session, &path, &request.source)?;

    let mut params = json!({
        "textDocument": { "uri": uri },
        "position": lsp_position(request.line, request.column)
    });
    merge_lsp_params(&mut params, extra_params.take());

    ty_send_request(session, method, params, Duration::from_secs(2))
}

fn merge_lsp_params(target: &mut Value, extra: Value) {
    let (Some(target), Some(extra)) = (target.as_object_mut(), extra.as_object()) else {
        return;
    };
    for (key, value) in extra {
        target.insert(key.clone(), value.clone());
    }
}

fn lsp_position(line: u32, column: u32) -> Value {
    json!({
        "line": line.saturating_sub(1),
        "character": column.saturating_sub(1),
    })
}

fn lsp_point_range(line: u32, column: u32) -> Value {
    let position = lsp_position(line, column);
    json!({
        "start": position,
        "end": position,
    })
}

fn full_lsp_range(source: &str) -> Value {
    let mut line = 0_u32;
    let mut character = 0_u32;
    for segment in source.split_inclusive('\n') {
        if segment.ends_with('\n') {
            line += 1;
            character = 0;
        } else {
            character = segment.chars().count() as u32;
        }
    }

    json!({
        "start": { "line": 0, "character": 0 },
        "end": { "line": line, "character": character }
    })
}

fn request_ty_diagnostics(
    root: &Path,
    path: &Path,
    content: &str,
) -> Result<Vec<EditorDiagnostic>, String> {
    let state = ty_lsp_state();
    let mut bridge = state
        .lock()
        .map_err(|_| "ty language server bridge lock was poisoned.".to_string())?;
    let session = ensure_ty_lsp_session(&mut bridge, root)?;
    ensure_ty_initialized(session, root)?;
    let (uri, version) = sync_ty_document(session, path, content)?;
    let pull_diagnostics = session
        .shared
        .lock()
        .map_err(|_| "ty language server state lock was poisoned.".to_string())?
        .pull_diagnostics;

    if pull_diagnostics {
        match ty_send_request(
            session,
            "textDocument/diagnostic",
            json!({
                "textDocument": { "uri": uri }
            }),
            Duration::from_secs(3),
        ) {
            Ok(response) => return Ok(parse_lsp_diagnostics(&response, "ty", content)),
            Err(error) => {
                if let Some(diagnostics) = wait_for_ty_published_diagnostics(
                    session,
                    &uri,
                    version,
                    content,
                    Duration::from_millis(850),
                ) {
                    return Ok(diagnostics);
                }
                return Err(error);
            }
        }
    }

    wait_for_ty_published_diagnostics(
        session,
        &uri,
        version,
        content,
        Duration::from_millis(1_500),
    )
    .ok_or_else(|| "ty did not publish diagnostics for the active document.".to_string())
}

fn wait_for_ty_published_diagnostics(
    session: &TyLspSession,
    uri: &str,
    min_version: i32,
    source: &str,
    timeout: Duration,
) -> Option<Vec<EditorDiagnostic>> {
    let deadline = Instant::now() + timeout;

    loop {
        if let Ok(shared) = session.shared.lock() {
            if let Some(published) = shared.published_diagnostics.get(uri) {
                let version_matches = published
                    .version
                    .map(|version| version >= min_version)
                    .unwrap_or(true);

                if version_matches {
                    return Some(parse_lsp_diagnostic_items(
                        &published.diagnostics,
                        "ty",
                        source,
                    ));
                }
            }
        }

        if Instant::now() >= deadline {
            return None;
        }

        thread::sleep(Duration::from_millis(25));
    }
}

fn rust_analyzer_lsp_state() -> &'static Mutex<RustAnalyzerLspState> {
    RUST_ANALYZER_LSP.get_or_init(|| Mutex::new(RustAnalyzerLspState::default()))
}

fn ensure_rust_analyzer_lsp_session<'a>(
    bridge: &'a mut RustAnalyzerLspState,
    root: &Path,
) -> Result<&'a mut RustAnalyzerLspSession, String> {
    let root_string = path_to_string(root);
    let mut needs_restart = bridge.session.is_none();

    if let Some(session) = bridge.session.as_mut() {
        let exited = session
            .child
            .try_wait()
            .map_err(|err| format!("Could not inspect rust-analyzer. {}", err))?
            .is_some();
        let (current_root, failed) = {
            let shared = session
                .shared
                .lock()
                .map_err(|_| "rust-analyzer state lock was poisoned.".to_string())?;
            (shared.current_root.clone(), shared.failed)
        };
        needs_restart = exited || failed || current_root != root_string;
    }

    if needs_restart {
        if let Some(session) = bridge.session.as_mut() {
            dispose_rust_analyzer_lsp_session(session);
        }
        bridge.session = Some(spawn_rust_analyzer_lsp_session(&root_string)?);
    }

    bridge
        .session
        .as_mut()
        .ok_or_else(|| "rust-analyzer did not start.".to_string())
}

fn spawn_rust_analyzer_lsp_session(root: &str) -> Result<RustAnalyzerLspSession, String> {
    let mut prepared = prepare_cli_command("rust-analyzer", &[]);
    prepared.command.stdin(Stdio::piped());
    prepared.command.stdout(Stdio::piped());
    prepared.command.stderr(Stdio::piped());
    prepared.command.current_dir(root);
    hide_background_window(&mut prepared.command);

    let mut child = prepared.command.spawn().map_err(|err| {
        format!(
            "Failed to start `rust-analyzer`. Install the rust-analyzer component or put it on PATH. {}",
            err
        )
    })?;

    let stdin =
        Arc::new(Mutex::new(child.stdin.take().ok_or_else(|| {
            "rust-analyzer did not expose stdin.".to_string()
        })?));
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "rust-analyzer did not expose stdout.".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "rust-analyzer did not expose stderr.".to_string())?;
    let shared = Arc::new(Mutex::new(RustAnalyzerLspSharedState::new(
        root.to_string(),
    )));

    spawn_rust_analyzer_lsp_stdout_reader(shared.clone(), stdin.clone(), stdout);
    spawn_rust_analyzer_lsp_stderr_reader(stderr);

    Ok(RustAnalyzerLspSession {
        child,
        stdin,
        shared,
    })
}

fn dispose_rust_analyzer_lsp_session(session: &mut RustAnalyzerLspSession) {
    let _ = session.child.kill();
    let _ = session.child.wait();
}

fn spawn_rust_analyzer_lsp_stdout_reader(
    shared: Arc<Mutex<RustAnalyzerLspSharedState>>,
    stdin: Arc<Mutex<ChildStdin>>,
    stdout: impl Read + Send + 'static,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let Ok(Some(message)) = read_lsp_message(&mut reader) else {
                break;
            };

            if message.get("method").is_some() {
                if message.get("method").and_then(Value::as_str)
                    == Some("textDocument/publishDiagnostics")
                {
                    handle_rust_analyzer_published_diagnostics(&shared, &message);
                    continue;
                }
                if let Some(response) = rust_analyzer_server_request_response(&shared, &message) {
                    let _ = send_lsp_json(&stdin, &response);
                }
                continue;
            }

            if let Some(id) = message.get("id").cloned() {
                handle_rust_analyzer_lsp_response(&shared, id, &message);
            }
        }
    });
}

fn handle_rust_analyzer_published_diagnostics(
    shared: &Arc<Mutex<RustAnalyzerLspSharedState>>,
    message: &Value,
) {
    let Some(params) = message.get("params") else {
        return;
    };
    let Some(uri) = params.get("uri").and_then(Value::as_str) else {
        return;
    };
    let diagnostics = params
        .get("diagnostics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let version = params
        .get("version")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok());

    if let Ok(mut state) = shared.lock() {
        state.published_diagnostics.insert(
            uri.to_string(),
            RustAnalyzerPublishedDiagnostics {
                version,
                diagnostics,
            },
        );
    }
}

fn spawn_rust_analyzer_lsp_stderr_reader(stderr: impl Read + Send + 'static) {
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if line.is_err() {
                break;
            }
        }
    });
}

fn handle_rust_analyzer_lsp_response(
    shared: &Arc<Mutex<RustAnalyzerLspSharedState>>,
    id: Value,
    message: &Value,
) {
    let Some(id_key) = request_id_key(&id) else {
        return;
    };

    let sender = shared
        .lock()
        .ok()
        .and_then(|mut state| state.pending_responses.remove(&id_key));

    if let Some(sender) = sender {
        if let Some(result) = message.get("result") {
            let _ = sender.send(Ok(result.clone()));
        } else {
            let error = message
                .get("error")
                .map(json_error_message)
                .unwrap_or_else(|| "rust-analyzer returned an empty response.".into());
            let _ = sender.send(Err(error));
        }
    }
}

fn rust_analyzer_server_request_response(
    shared: &Arc<Mutex<RustAnalyzerLspSharedState>>,
    message: &Value,
) -> Option<Value> {
    let root = shared
        .lock()
        .ok()
        .map(|state| state.current_root.clone())
        .unwrap_or_default();
    let id = message.get("id")?.clone();
    let method = message.get("method").and_then(Value::as_str)?;
    let result = match method {
        "workspace/configuration" => {
            let items = message
                .get("params")
                .and_then(|value| value.get("items"))
                .and_then(Value::as_array);
            Value::Array(
                items
                    .map(|items| {
                        items
                            .iter()
                            .map(|item| rust_analyzer_configuration_item_value(&root, item))
                            .collect()
                    })
                    .unwrap_or_default(),
            )
        }
        "workspace/workspaceFolders" => Value::Array(vec![json!({
            "uri": path_string_to_file_uri(&root),
            "name": Path::new(&root)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("workspace")
        })]),
        "client/registerCapability"
        | "client/unregisterCapability"
        | "window/workDoneProgress/create" => Value::Null,
        _ => Value::Null,
    };

    Some(json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    }))
}

fn rust_analyzer_configuration_item_value(root: &str, item: &Value) -> Value {
    match item.get("section").and_then(Value::as_str) {
        None | Some("rust-analyzer") => json!({
            "cargo": {
                "allTargets": true,
                "features": "all",
                "buildScripts": {
                    "enable": true
                }
            },
            "checkOnSave": false,
            "diagnostics": {
                "enable": true
            },
            "procMacro": {
                "enable": true
            },
            "files": {
                "excludeDirs": ["target", ".git"]
            }
        }),
        Some("rust-analyzer.cargo") => json!({
            "allTargets": true,
            "features": "all",
            "targetDir": Path::new(root).join("target").join("rust-analyzer")
        }),
        Some("rust-analyzer.checkOnSave") => json!(false),
        _ => json!({}),
    }
}

fn rust_analyzer_send_request(
    session: &RustAnalyzerLspSession,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value, String> {
    let (tx, rx) = mpsc::channel();
    let (request_id, message) = {
        let mut shared = session
            .shared
            .lock()
            .map_err(|_| "rust-analyzer state lock was poisoned.".to_string())?;
        let request_id = shared.next_request_id;
        shared.next_request_id += 1;
        shared.pending_responses.insert(request_id.to_string(), tx);
        (
            request_id,
            json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": params,
            }),
        )
    };

    if let Err(error) = send_lsp_json(&session.stdin, &message) {
        if let Ok(mut shared) = session.shared.lock() {
            shared.pending_responses.remove(&request_id.to_string());
        }
        return Err(error);
    }

    match rx.recv_timeout(timeout) {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) => Err(error),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            if let Ok(mut shared) = session.shared.lock() {
                shared.pending_responses.remove(&request_id.to_string());
                shared.failed = true;
            }
            Err(format!(
                "rust-analyzer did not answer `{}` within {} seconds.",
                method,
                timeout.as_secs()
            ))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            if let Ok(mut shared) = session.shared.lock() {
                shared.failed = true;
            }
            Err(format!(
                "rust-analyzer closed while waiting for `{}`.",
                method
            ))
        }
    }
}

fn rust_analyzer_send_notification(
    session: &RustAnalyzerLspSession,
    method: &str,
    params: Value,
) -> Result<(), String> {
    send_lsp_json(
        &session.stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }),
    )
}

fn ensure_rust_analyzer_initialized(
    session: &RustAnalyzerLspSession,
    root: &Path,
) -> Result<(), String> {
    let needs_initialize = !session
        .shared
        .lock()
        .map_err(|_| "rust-analyzer state lock was poisoned.".to_string())?
        .initialized;

    if !needs_initialize {
        return Ok(());
    }

    let root_uri = path_to_file_uri(root);
    let response = rust_analyzer_send_request(
        session,
        "initialize",
        json!({
            "processId": null,
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "hover": {
                        "dynamicRegistration": false,
                        "contentFormat": ["markdown", "plaintext"]
                    },
                    "semanticTokens": {
                        "dynamicRegistration": false,
                        "requests": { "full": true, "range": false },
                        "tokenTypes": LSP_SEMANTIC_TOKEN_TYPES,
                        "tokenModifiers": [],
                        "formats": ["relative"],
                        "overlappingTokenSupport": false,
                        "multilineTokenSupport": true
                    },
                    "diagnostic": { "dynamicRegistration": false }
                },
                "workspace": {
                    "workspaceFolders": true,
                    "configuration": true
                },
                "window": {
                    "workDoneProgress": false
                }
            },
            "workspaceFolders": [{
                "uri": root_uri,
                "name": root.file_name().and_then(|value| value.to_str()).unwrap_or("workspace")
            }]
        }),
        Duration::from_secs(8),
    )?;

    if let Some(token_types) = response
        .get("capabilities")
        .and_then(|value| value.get("semanticTokensProvider"))
        .and_then(|value| value.get("legend"))
        .and_then(|value| value.get("tokenTypes"))
        .and_then(Value::as_array)
    {
        if let Ok(mut shared) = session.shared.lock() {
            shared.token_types = token_types
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect();
        }
    }

    rust_analyzer_send_notification(session, "initialized", json!({}))?;
    if let Ok(mut shared) = session.shared.lock() {
        shared.initialized = true;
    }
    Ok(())
}

fn sync_rust_analyzer_document(
    session: &RustAnalyzerLspSession,
    path: &Path,
    content: &str,
) -> Result<(String, i32), String> {
    let uri = path_to_file_uri(path);
    let (version, already_synced) = {
        let mut shared = session
            .shared
            .lock()
            .map_err(|_| "rust-analyzer state lock was poisoned.".to_string())?;
        let entry = shared.synced_documents.entry(uri.clone()).or_insert(0);
        let already_synced = *entry > 0;
        *entry += 1;
        (*entry, already_synced)
    };

    if already_synced {
        rust_analyzer_send_notification(
            session,
            "textDocument/didChange",
            json!({
                "textDocument": {
                    "uri": uri,
                    "version": version,
                },
                "contentChanges": [{
                    "text": content,
                }]
            }),
        )?;
    } else {
        rust_analyzer_send_notification(
            session,
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "rust",
                    "version": version,
                    "text": content,
                }
            }),
        )?;
    }

    Ok((uri, version))
}

fn request_rust_analyzer_diagnostics(
    root: &Path,
    path: &Path,
    content: &str,
) -> Result<Vec<EditorDiagnostic>, String> {
    if language_id_from_path(path) != "rust" || probe_available_command("rust-analyzer").is_none() {
        return Ok(Vec::new());
    }

    let state = rust_analyzer_lsp_state();
    let mut bridge = state
        .lock()
        .map_err(|_| "rust-analyzer bridge lock was poisoned.".to_string())?;
    let session = ensure_rust_analyzer_lsp_session(&mut bridge, root)?;
    ensure_rust_analyzer_initialized(session, root)?;
    let (uri, version) = sync_rust_analyzer_document(session, path, content)?;

    match rust_analyzer_send_request(
        session,
        "textDocument/diagnostic",
        json!({
            "textDocument": { "uri": uri }
        }),
        Duration::from_secs(3),
    ) {
        Ok(response) => Ok(parse_lsp_diagnostics(&response, "rust-analyzer", content)),
        Err(_) => Ok(wait_for_rust_analyzer_published_diagnostics(
            session,
            &uri,
            version,
            content,
            Duration::from_millis(1_500),
        )
        .unwrap_or_default()),
    }
}

fn wait_for_rust_analyzer_published_diagnostics(
    session: &RustAnalyzerLspSession,
    uri: &str,
    min_version: i32,
    source: &str,
    timeout: Duration,
) -> Option<Vec<EditorDiagnostic>> {
    let deadline = Instant::now() + timeout;

    loop {
        if let Ok(shared) = session.shared.lock() {
            if let Some(published) = shared.published_diagnostics.get(uri) {
                let version_matches = published
                    .version
                    .map(|version| version >= min_version)
                    .unwrap_or(true);

                if version_matches {
                    return Some(parse_lsp_diagnostic_items(
                        &published.diagnostics,
                        "rust-analyzer",
                        source,
                    ));
                }
            }
        }

        if Instant::now() >= deadline {
            return None;
        }

        thread::sleep(Duration::from_millis(25));
    }
}

fn rust_analyzer_semantic_tokens_for_document(
    root: &Path,
    path: &Path,
    content: &str,
) -> Result<EditorSemanticsPayload, String> {
    if language_id_from_path(path) != "rust" || probe_available_command("rust-analyzer").is_none() {
        return Ok(EditorSemanticsPayload::default());
    }

    let state = rust_analyzer_lsp_state();
    let mut bridge = state
        .lock()
        .map_err(|_| "rust-analyzer bridge lock was poisoned.".to_string())?;
    let session = ensure_rust_analyzer_lsp_session(&mut bridge, root)?;
    ensure_rust_analyzer_initialized(session, root)?;
    let (uri, _) = sync_rust_analyzer_document(session, path, content)?;

    let response = rust_analyzer_send_request(
        session,
        "textDocument/semanticTokens/full",
        json!({
            "textDocument": {
                "uri": uri,
            }
        }),
        Duration::from_secs(2),
    )?;

    let token_types = session
        .shared
        .lock()
        .map_err(|_| "rust-analyzer state lock was poisoned.".to_string())?
        .token_types
        .clone();
    let tokens = decode_lsp_semantic_tokens(&response, &token_types);
    Ok(EditorSemanticsPayload {
        tokens,
        hover_items: Vec::new(),
    })
}

fn request_rust_analyzer_hover(request: &EditorHoverRequest) -> Result<Option<HoverItem>, String> {
    if probe_available_command("rust-analyzer").is_none() {
        return Ok(rust_tree_sitter_hover_for_request(request));
    }

    let path = PathBuf::from(&request.file_path);
    let Some(rust_root) = find_rust_workspace_root(&path) else {
        return Ok(rust_tree_sitter_hover_for_request(request));
    };
    let state = rust_analyzer_lsp_state();
    let mut bridge = state
        .lock()
        .map_err(|_| "rust-analyzer bridge lock was poisoned.".to_string())?;
    let session = ensure_rust_analyzer_lsp_session(&mut bridge, &rust_root)?;
    ensure_rust_analyzer_initialized(session, &rust_root)?;
    let (uri, _) = sync_rust_analyzer_document(session, &path, &request.source)?;

    let params = json!({
        "textDocument": { "uri": uri },
        "position": {
            "line": request.line.saturating_sub(1),
            "character": request.column.saturating_sub(1),
        }
    });

    let mut response = match rust_analyzer_send_request(
        session,
        "textDocument/hover",
        params.clone(),
        Duration::from_secs(2),
    ) {
        Ok(response) => response,
        Err(_) => return Ok(rust_tree_sitter_hover_for_request(request)),
    };
    for delay_ms in RUST_ANALYZER_HOVER_RETRY_DELAYS_MS {
        if hover_item_from_lsp_with_provider(
            &response,
            request.line,
            request.column,
            "rust-analyzer",
            "Provided by rust-analyzer",
        )
        .is_some()
        {
            break;
        }

        thread::sleep(Duration::from_millis(*delay_ms));
        response = match rust_analyzer_send_request(
            session,
            "textDocument/hover",
            params.clone(),
            Duration::from_secs(2),
        ) {
            Ok(response) => response,
            Err(_) => return Ok(rust_tree_sitter_hover_for_request(request)),
        };
    }
    Ok(hover_item_from_lsp_with_provider(
        &response,
        request.line,
        request.column,
        "rust-analyzer",
        "Provided by rust-analyzer",
    )
    .or_else(|| rust_tree_sitter_hover_for_request(request)))
}

fn rust_tree_sitter_hover_for_request(request: &EditorHoverRequest) -> Option<HoverItem> {
    rust_tree_sitter_hover(&request.source, request.line, request.column)
}

fn request_c_family_hover(
    request: &EditorHoverRequest,
    language: SourceLanguage,
) -> Option<HoverItem> {
    c_family_tree_sitter_hover(&request.source, language, request.line, request.column)
}

fn rust_tree_sitter_semantics_for_document(content: &str) -> EditorSemanticsPayload {
    let Some(tree) = parse_tree(SourceLanguage::Rust, content) else {
        return EditorSemanticsPayload::default();
    };

    let mut tokens = Vec::new();
    let mut seen = BTreeSet::new();
    collect_rust_semantic_tokens(tree.root_node(), content.as_bytes(), &mut tokens, &mut seen);
    EditorSemanticsPayload {
        tokens,
        hover_items: Vec::new(),
    }
}

fn collect_rust_semantic_tokens(
    node: Node<'_>,
    source: &[u8],
    tokens: &mut Vec<SemanticToken>,
    seen: &mut BTreeSet<String>,
) {
    match node.kind() {
        "attribute_item" | "inner_attribute_item" => {
            push_semantic_token(tokens, seen, &text_span_from_node(node), "attribute");
        }
        "function_item" => {
            push_rust_named_child_token(node, tokens, seen, "name", "functionDefinition");
        }
        "struct_item" => {
            push_rust_named_child_token(node, tokens, seen, "name", "struct");
        }
        "enum_item" => {
            push_rust_named_child_token(node, tokens, seen, "name", "enum");
        }
        "trait_item" => {
            push_rust_named_child_token(node, tokens, seen, "name", "type");
        }
        "type_item" => {
            push_rust_named_child_token(node, tokens, seen, "name", "type");
        }
        "const_item" | "static_item" => {
            push_rust_named_child_token(node, tokens, seen, "name", "variableDefinition");
        }
        "line_comment" | "block_comment" => {
            push_semantic_token(tokens, seen, &text_span_from_node(node), "comment");
        }
        "string_literal" | "raw_string_literal" | "char_literal" => {
            push_semantic_token(tokens, seen, &text_span_from_node(node), "string");
        }
        "integer_literal" | "float_literal" => {
            push_semantic_token(tokens, seen, &text_span_from_node(node), "number");
        }
        "primitive_type" => {
            push_semantic_token(tokens, seen, &text_span_from_node(node), "builtinType");
        }
        "lifetime" => {
            push_semantic_token(tokens, seen, &text_span_from_node(node), "lifetime");
        }
        "identifier" => collect_rust_identifier_token(node, tokens, seen),
        kind if rust_keyword_detail(kind) != "Rust keyword." => {
            push_semantic_token(tokens, seen, &text_span_from_node(node), "keyword");
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_rust_semantic_tokens(child, source, tokens, seen);
    }
}

fn collect_rust_identifier_token(
    node: Node<'_>,
    tokens: &mut Vec<SemanticToken>,
    seen: &mut BTreeSet<String>,
) {
    let Some(parent) = node.parent() else {
        return;
    };

    let kind = match parent.kind() {
        "call_expression" => Some("functionCall"),
        "let_declaration" => Some("variableDefinition"),
        "parameters" | "closure_parameters" => Some("parameter"),
        _ => None,
    };

    if let Some(kind) = kind {
        push_semantic_token(tokens, seen, &text_span_from_node(node), kind);
    }
}

fn push_rust_named_child_token(
    node: Node<'_>,
    tokens: &mut Vec<SemanticToken>,
    seen: &mut BTreeSet<String>,
    field_name: &str,
    kind: &str,
) {
    if let Some(child) = node.child_by_field_name(field_name) {
        push_semantic_token(tokens, seen, &text_span_from_node(child), kind);
    }
}

fn c_family_tree_sitter_semantics_for_document(
    language: SourceLanguage,
    content: &str,
) -> EditorSemanticsPayload {
    let Some(tree) = parse_tree(language, content) else {
        return EditorSemanticsPayload::default();
    };

    let mut tokens = Vec::new();
    let mut seen = BTreeSet::new();
    collect_c_family_semantic_tokens(
        tree.root_node(),
        content.as_bytes(),
        language,
        &mut tokens,
        &mut seen,
    );
    EditorSemanticsPayload {
        tokens,
        hover_items: Vec::new(),
    }
}

fn collect_c_family_semantic_tokens(
    node: Node<'_>,
    source: &[u8],
    language: SourceLanguage,
    tokens: &mut Vec<SemanticToken>,
    seen: &mut BTreeSet<String>,
) {
    match node.kind() {
        "function_definition" => {
            if let Some(name) = find_c_family_declarator_name_node(node) {
                push_semantic_token(
                    tokens,
                    seen,
                    &text_span_from_node(name),
                    "functionDefinition",
                );
            }
        }
        "class_specifier" => {
            push_c_family_named_child_token(node, tokens, seen, "name", "class");
        }
        "struct_specifier" => {
            push_c_family_named_child_token(node, tokens, seen, "name", "struct");
        }
        "enum_specifier" => {
            push_c_family_named_child_token(node, tokens, seen, "name", "enum");
        }
        "union_specifier" => {
            push_c_family_named_child_token(node, tokens, seen, "name", "struct");
        }
        "namespace_definition" => {
            push_c_family_named_child_token(node, tokens, seen, "name", "namespace");
        }
        "call_expression" => {
            if let Some(function) = node.child_by_field_name("function") {
                push_semantic_token(tokens, seen, &text_span_from_node(function), "functionCall");
            }
        }
        "preproc_include"
        | "preproc_def"
        | "preproc_function_def"
        | "preproc_call"
        | "preproc_if"
        | "preproc_ifdef"
        | "preproc_else" => {
            push_semantic_token(tokens, seen, &text_span_from_node(node), "macro");
        }
        "comment" => {
            push_semantic_token(tokens, seen, &text_span_from_node(node), "comment");
        }
        "string_literal" | "raw_string_literal" | "char_literal" | "system_lib_string" => {
            push_semantic_token(tokens, seen, &text_span_from_node(node), "string");
        }
        "number_literal" | "float_literal" => {
            push_semantic_token(tokens, seen, &text_span_from_node(node), "number");
        }
        "primitive_type" => {
            push_semantic_token(tokens, seen, &text_span_from_node(node), "builtinType");
        }
        "type_identifier" => {
            push_semantic_token(tokens, seen, &text_span_from_node(node), "type");
        }
        "namespace_identifier" => {
            push_semantic_token(tokens, seen, &text_span_from_node(node), "namespace");
        }
        "field_identifier" => {
            push_semantic_token(tokens, seen, &text_span_from_node(node), "property");
        }
        "identifier" => collect_c_family_identifier_token(node, tokens, seen),
        _ => {
            if let Some(text) = c_family_leaf_text(node, source) {
                if c_family_keyword_detail(language, text) != "C-family keyword." {
                    let kind = if c_family_builtin_type(text) {
                        "builtinType"
                    } else {
                        "keyword"
                    };
                    push_semantic_token(tokens, seen, &text_span_from_node(node), kind);
                }
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_c_family_semantic_tokens(child, source, language, tokens, seen);
    }
}

fn collect_c_family_identifier_token(
    node: Node<'_>,
    tokens: &mut Vec<SemanticToken>,
    seen: &mut BTreeSet<String>,
) {
    let Some(parent) = node.parent() else {
        return;
    };

    let kind = match parent.kind() {
        "parameter_declaration" => Some("parameter"),
        "init_declarator" | "declaration" => Some("variableDefinition"),
        _ => None,
    };

    if let Some(kind) = kind {
        push_semantic_token(tokens, seen, &text_span_from_node(node), kind);
    }
}

fn push_c_family_named_child_token(
    node: Node<'_>,
    tokens: &mut Vec<SemanticToken>,
    seen: &mut BTreeSet<String>,
    field_name: &str,
    kind: &str,
) {
    if let Some(child) = node.child_by_field_name(field_name) {
        push_semantic_token(tokens, seen, &text_span_from_node(child), kind);
    }
}

fn c_family_tree_sitter_hover(
    source: &str,
    language: SourceLanguage,
    line: u32,
    column: u32,
) -> Option<HoverItem> {
    let tree = parse_tree(language, source)?;
    let bytes = source.as_bytes();
    let offset = offset_from_line_column(source, line, column).min(source.len());
    let end_offset = (offset + 1).min(source.len());
    let mut node = tree
        .root_node()
        .descendant_for_byte_range(offset, end_offset)
        .or_else(|| {
            offset
                .checked_sub(1)
                .and_then(|previous| tree.root_node().descendant_for_byte_range(previous, offset))
        })?;

    if let Some(keyword) = c_family_keyword_at(source, offset, language) {
        let (start_line, start_column, end_line, end_column) = rust_word_span(source, offset)?;
        return Some(HoverItem {
            kind: format!("{} syntax", c_family_language_label(language)),
            title: format!("{} keyword `{keyword}`", c_family_language_label(language)),
            detail: Some(c_family_keyword_detail(language, keyword).into()),
            source: Some("Provided by Hematite C-family parser".into()),
            start_line,
            start_column,
            end_line,
            end_column,
        });
    }

    loop {
        if let Some(item) = c_family_hover_from_node(node, bytes, language) {
            return Some(item);
        }

        node = node.parent()?;
    }
}

fn c_family_hover_from_node(
    node: Node<'_>,
    source: &[u8],
    language: SourceLanguage,
) -> Option<HoverItem> {
    match node.kind() {
        "function_definition" => {
            let signature = c_family_signature_line_for_node(node, source)?;
            let is_cuda_kernel = matches!(language, SourceLanguage::Cuda)
                && signature
                    .split_whitespace()
                    .any(|part| part == "__global__");
            c_family_hover_item_for_node(
                node,
                if is_cuda_kernel {
                    "cuda kernel"
                } else {
                    "c-family function"
                },
                signature,
                Some(if is_cuda_kernel {
                    "CUDA kernel defined in this file.".into()
                } else {
                    "Function defined in this file.".into()
                }),
            )
        }
        "class_specifier" => c_family_type_hover(node, source, "cpp class", "class"),
        "struct_specifier" => c_family_type_hover(node, source, "c-family struct", "struct"),
        "enum_specifier" => c_family_type_hover(node, source, "c-family enum", "enum"),
        "union_specifier" => c_family_type_hover(node, source, "c-family union", "union"),
        "namespace_definition" => c_family_type_hover(node, source, "cpp namespace", "namespace"),
        "preproc_include" | "preproc_def" | "preproc_function_def" | "preproc_call" => {
            c_family_hover_item_for_node(
                node,
                "preprocessor",
                c_family_node_single_line_text(node, source)?,
                Some("C-family preprocessor directive.".into()),
            )
        }
        _ => None,
    }
}

fn c_family_type_hover(
    node: Node<'_>,
    source: &[u8],
    kind: &str,
    label: &str,
) -> Option<HoverItem> {
    let name = read_field_text(node, source, "name")?;
    c_family_hover_item_for_node(
        node,
        kind,
        format!("{label} {name}"),
        Some("Type or namespace defined in this file.".into()),
    )
}

fn c_family_hover_item_for_node(
    node: Node<'_>,
    kind: &str,
    title: String,
    detail: Option<String>,
) -> Option<HoverItem> {
    let span = text_span_from_node(node);
    Some(HoverItem {
        kind: kind.into(),
        title,
        detail,
        source: Some("Provided by Hematite C-family parser".into()),
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
    })
}

fn c_family_signature_line_for_node(node: Node<'_>, source: &[u8]) -> Option<String> {
    let text = c_family_node_text(node, source)?;
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("//"))
        .map(|line| line.trim_end_matches('{').trim().to_string())
}

fn c_family_node_single_line_text(node: Node<'_>, source: &[u8]) -> Option<String> {
    c_family_node_text(node, source)
        .map(|text| text.lines().next().unwrap_or("").trim().to_string())
}

fn c_family_node_text(node: Node<'_>, source: &[u8]) -> Option<String> {
    Some(node.utf8_text(source).ok()?.trim().to_string())
}

fn c_family_leaf_text<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    if node.child_count() != 0 {
        return None;
    }

    node.utf8_text(source).ok().map(str::trim)
}

fn find_c_family_declarator_name_node(node: Node<'_>) -> Option<Node<'_>> {
    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "type_identifier" | "operator_name"
    ) {
        return Some(node);
    }

    for field_name in ["name", "declarator", "type"] {
        if let Some(field) = node.child_by_field_name(field_name) {
            if let Some(name) = find_c_family_declarator_name_node(field) {
                return Some(name);
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(name) = find_c_family_declarator_name_node(child) {
            return Some(name);
        }
    }

    None
}

fn c_family_keyword_at(
    source: &str,
    offset: usize,
    language: SourceLanguage,
) -> Option<&'static str> {
    let (start, end) = identifier_bounds_at_offset(source, offset)?;
    let word = source.get(start..end)?;
    (c_family_keyword_detail(language, word) != "C-family keyword.").then_some(match word {
        "__global__" => "__global__",
        "__device__" => "__device__",
        "__host__" => "__host__",
        "__shared__" => "__shared__",
        "alignas" => "alignas",
        "auto" => "auto",
        "bool" => "bool",
        "break" => "break",
        "case" => "case",
        "char" => "char",
        "class" => "class",
        "const" => "const",
        "constexpr" => "constexpr",
        "continue" => "continue",
        "double" => "double",
        "else" => "else",
        "enum" => "enum",
        "extern" => "extern",
        "float" => "float",
        "for" => "for",
        "if" => "if",
        "inline" => "inline",
        "int" => "int",
        "long" => "long",
        "namespace" => "namespace",
        "private" => "private",
        "protected" => "protected",
        "public" => "public",
        "return" => "return",
        "short" => "short",
        "signed" => "signed",
        "static" => "static",
        "struct" => "struct",
        "switch" => "switch",
        "template" => "template",
        "typename" => "typename",
        "union" => "union",
        "unsigned" => "unsigned",
        "using" => "using",
        "virtual" => "virtual",
        "void" => "void",
        "while" => "while",
        _ => return None,
    })
}

fn c_family_keyword_detail(language: SourceLanguage, keyword: &str) -> &'static str {
    match keyword {
        "__global__" => "Marks a CUDA function as a kernel launched from host code.",
        "__device__" => "Marks a CUDA function or variable as device-side.",
        "__host__" => "Marks a CUDA function as callable from host code.",
        "__shared__" => "Places a CUDA variable in block-shared device memory.",
        "class" => "Defines a C++ class type.",
        "struct" => "Defines a C-family aggregate type.",
        "enum" => "Defines an enumeration type.",
        "union" => "Defines a union type.",
        "namespace" => "Defines a C++ namespace scope.",
        "template" => "Introduces a C++ template declaration.",
        "typename" => "Names a type parameter or dependent type.",
        "public" | "private" | "protected" => "Sets C++ member access.",
        "return" => "Returns from the current function.",
        "if" => "Starts a conditional statement.",
        "else" => "Provides an alternate conditional branch.",
        "for" => "Starts a counted or range-based loop.",
        "while" => "Loops while a condition remains true.",
        "switch" => "Dispatches control based on a value.",
        "case" => "Introduces a switch branch.",
        "break" => "Leaves the current switch or loop.",
        "continue" => "Starts the next loop iteration.",
        "using" => "Introduces a using declaration, alias, or directive.",
        "extern" => "Declares external linkage.",
        "static" => "Gives storage duration or internal linkage depending on context.",
        "const" => "Marks an object or member function as immutable in context.",
        "constexpr" => "Requires compile-time evaluation when possible.",
        "inline" => "Permits multiple definitions and suggests inline expansion.",
        "virtual" => "Enables dynamic dispatch for a C++ member function.",
        keyword if c_family_builtin_type(keyword) => "Built-in C-family scalar type.",
        _ => {
            let _ = language;
            "C-family keyword."
        }
    }
}

fn c_family_builtin_type(value: &str) -> bool {
    matches!(
        value,
        "void"
            | "bool"
            | "char"
            | "short"
            | "int"
            | "long"
            | "float"
            | "double"
            | "signed"
            | "unsigned"
            | "auto"
    )
}

fn c_family_language_label(language: SourceLanguage) -> &'static str {
    match language {
        SourceLanguage::C => "C",
        SourceLanguage::Cpp => "C++",
        SourceLanguage::Cuda => "CUDA C++",
        _ => "C-family",
    }
}

fn rust_tree_sitter_hover(source: &str, line: u32, column: u32) -> Option<HoverItem> {
    let tree = parse_tree(SourceLanguage::Rust, source)?;
    let bytes = source.as_bytes();
    let offset = offset_from_line_column(source, line, column).min(source.len());
    let end_offset = (offset + 1).min(source.len());
    let mut node = tree
        .root_node()
        .descendant_for_byte_range(offset, end_offset)
        .or_else(|| {
            offset
                .checked_sub(1)
                .and_then(|previous| tree.root_node().descendant_for_byte_range(previous, offset))
        })?;

    if let Some(keyword) = rust_keyword_at(source, offset) {
        let (start_line, start_column, end_line, end_column) = rust_word_span(source, offset)?;
        return Some(HoverItem {
            kind: "rust syntax".into(),
            title: format!("Rust keyword `{keyword}`"),
            detail: Some(rust_keyword_detail(keyword).into()),
            source: Some("Provided by Hematite Rust parser".into()),
            start_line,
            start_column,
            end_line,
            end_column,
        });
    }

    loop {
        if let Some(item) = rust_hover_from_node(node, bytes) {
            return Some(item);
        }

        node = node.parent()?;
    }
}

fn rust_hover_from_node(node: Node<'_>, source: &[u8]) -> Option<HoverItem> {
    let kind = node.kind();
    match kind {
        "attribute_item" | "inner_attribute_item" => rust_hover_item_for_node(
            node,
            "rust attribute",
            rust_node_single_line_text(node, source)?,
            Some("Attribute applied to the following Rust item.".into()),
        ),
        "function_item" => rust_hover_item_for_node(
            node,
            "rust function",
            rust_signature_line_for_node(node, source)?,
            Some("Function defined in this file.".into()),
        ),
        "struct_item" => rust_hover_item_for_node(
            node,
            "rust struct",
            rust_signature_line_for_node(node, source)?,
            Some("Struct type defined in this file.".into()),
        ),
        "enum_item" => rust_hover_item_for_node(
            node,
            "rust enum",
            rust_signature_line_for_node(node, source)?,
            Some("Enum type defined in this file.".into()),
        ),
        "trait_item" => rust_hover_item_for_node(
            node,
            "rust trait",
            rust_signature_line_for_node(node, source)?,
            Some("Trait defined in this file.".into()),
        ),
        "type_item" => rust_hover_item_for_node(
            node,
            "rust type",
            rust_signature_line_for_node(node, source)?,
            Some("Type alias defined in this file.".into()),
        ),
        "impl_item" => rust_hover_item_for_node(
            node,
            "rust impl",
            rust_signature_line_for_node(node, source)?,
            Some("Implementation block defined in this file.".into()),
        ),
        _ => None,
    }
}

fn rust_hover_item_for_node(
    node: Node<'_>,
    kind: &str,
    title: String,
    detail: Option<String>,
) -> Option<HoverItem> {
    let span = text_span_from_node(node);
    Some(HoverItem {
        kind: kind.into(),
        title,
        detail,
        source: Some("Provided by Hematite Rust parser".into()),
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
    })
}

fn rust_signature_line_for_node(node: Node<'_>, source: &[u8]) -> Option<String> {
    let text = rust_node_text(node, source)?;
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("#["))
        .map(|line| line.trim_end_matches('{').trim().to_string())
}

fn rust_node_single_line_text(node: Node<'_>, source: &[u8]) -> Option<String> {
    rust_node_text(node, source).map(|text| text.lines().next().unwrap_or("").trim().to_string())
}

fn rust_node_text(node: Node<'_>, source: &[u8]) -> Option<String> {
    Some(node.utf8_text(source).ok()?.trim().to_string())
}

struct TextSpan {
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

fn text_span_from_node(node: Node<'_>) -> TextSpan {
    let start = node.start_position();
    let end = node.end_position();

    TextSpan {
        start_line: start.row as u32 + 1,
        start_column: start.column as u32 + 1,
        end_line: end.row as u32 + 1,
        end_column: end.column as u32 + 1,
    }
}

fn push_semantic_token(
    tokens: &mut Vec<SemanticToken>,
    seen: &mut BTreeSet<String>,
    span: &TextSpan,
    kind: &str,
) {
    let key = format!(
        "{}:{}:{}:{}:{}",
        kind, span.start_line, span.start_column, span.end_line, span.end_column
    );
    if !seen.insert(key) {
        return;
    }

    tokens.push(SemanticToken {
        kind: kind.into(),
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
    });
}

fn rust_keyword_at(source: &str, offset: usize) -> Option<&'static str> {
    let (start, end) = identifier_bounds_at_offset(source, offset)?;
    match source.get(start..end)? {
        "async" => Some("async"),
        "fn" => Some("fn"),
        "pub" => Some("pub"),
        "struct" => Some("struct"),
        "enum" => Some("enum"),
        "trait" => Some("trait"),
        "impl" => Some("impl"),
        "let" => Some("let"),
        "const" => Some("const"),
        "mut" => Some("mut"),
        "use" => Some("use"),
        "mod" => Some("mod"),
        "crate" => Some("crate"),
        "self" => Some("self"),
        "super" => Some("super"),
        "where" => Some("where"),
        "match" => Some("match"),
        "if" => Some("if"),
        "else" => Some("else"),
        "for" => Some("for"),
        "while" => Some("while"),
        "loop" => Some("loop"),
        "return" => Some("return"),
        "await" => Some("await"),
        _ => None,
    }
}

fn rust_keyword_detail(keyword: &str) -> &'static str {
    match keyword {
        "async" => "Marks a function or block as asynchronous.",
        "fn" => "Introduces a Rust function item.",
        "pub" => "Makes an item visible outside its current module.",
        "struct" => "Defines a Rust structure type.",
        "enum" => "Defines a Rust enum type.",
        "trait" => "Defines shared behavior that types can implement.",
        "impl" => "Defines inherent or trait implementations for a type.",
        "let" => "Introduces a local binding.",
        "const" => "Defines a compile-time constant item or binding.",
        "mut" => "Marks a binding or reference as mutable.",
        "use" => "Brings a path into scope.",
        "mod" => "Declares or defines a module.",
        "crate" => "Refers to the current crate.",
        "self" => "Refers to the current value or module.",
        "super" => "Refers to the parent module.",
        "where" => "Introduces additional generic bounds.",
        "match" => "Pattern-matches a value against arms.",
        "if" => "Starts a conditional expression.",
        "else" => "Provides the alternate branch of a conditional expression.",
        "for" => "Iterates over values from an iterator.",
        "while" => "Loops while a condition is true.",
        "loop" => "Starts an unconditional loop expression.",
        "return" => "Returns from the current function.",
        "await" => "Waits for a future to complete inside async code.",
        _ => "Rust keyword.",
    }
}

fn rust_word_span(source: &str, offset: usize) -> Option<(u32, u32, u32, u32)> {
    let (start, end) = identifier_bounds_at_offset(source, offset)?;
    let (start_line, start_column) = one_based_line_column_from_offset(source, start);
    let (end_line, end_column) = one_based_line_column_from_offset(source, end);
    Some((start_line, start_column, end_line, end_column))
}

struct CallSignaturePosition {
    lsp_line: u32,
    lsp_character: u32,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

fn call_signature_request_position(
    source: &str,
    line: u32,
    column: u32,
) -> Option<CallSignaturePosition> {
    let hover_offset = offset_from_line_column(source, line, column);
    let (word_start, word_end) = identifier_bounds_at_offset(source, hover_offset)?;
    let open_paren_offset = next_call_open_paren(source, word_end)?;
    let (lsp_line, lsp_character) =
        zero_based_line_column_from_offset(source, open_paren_offset + 1);
    let (start_line, start_column) = one_based_line_column_from_offset(source, word_start);
    let (end_line, end_column) = one_based_line_column_from_offset(source, word_end);

    Some(CallSignaturePosition {
        lsp_line,
        lsp_character,
        start_line,
        start_column,
        end_line,
        end_column,
    })
}

fn identifier_bounds_at_offset(source: &str, offset: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    let mut index = offset.min(bytes.len().saturating_sub(1));
    if !is_python_identifier_byte(bytes[index])
        && index > 0
        && is_python_identifier_byte(bytes[index - 1])
    {
        index -= 1;
    }
    if !is_python_identifier_byte(bytes[index]) {
        return None;
    }

    let mut start = index;
    while start > 0 && is_python_identifier_byte(bytes[start - 1]) {
        start -= 1;
    }

    let mut end = index + 1;
    while end < bytes.len() && is_python_identifier_byte(bytes[end]) {
        end += 1;
    }

    Some((start, end))
}

fn is_python_identifier_byte(value: u8) -> bool {
    value == b'_' || value.is_ascii_alphanumeric()
}

fn next_call_open_paren(source: &str, offset: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = offset;
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') {
        index += 1;
    }

    (index < bytes.len() && bytes[index] == b'(').then_some(index)
}

fn zero_based_line_column_from_offset(source: &str, offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut column = 0u32;
    for (byte_index, character) in source.char_indices() {
        if byte_index >= offset {
            break;
        }
        if character == '\n' {
            line += 1;
            column = 0;
        } else {
            column += 1;
        }
    }

    (line, column)
}

fn one_based_line_column_from_offset(source: &str, offset: usize) -> (u32, u32) {
    let (line, column) = zero_based_line_column_from_offset(source, offset);
    (line + 1, column + 1)
}

fn merge_hover_and_signature_help(
    hover: Option<HoverItem>,
    signature: Option<HoverItem>,
) -> Option<HoverItem> {
    match (hover, signature) {
        (Some(hover), Some(signature)) => {
            if hover.title.starts_with("<module ") || hover.title == "ty hover" {
                return Some(signature);
            }

            let mut detail_parts = Vec::new();
            if let Some(detail) = signature
                .detail
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                detail_parts.push(detail.to_string());
            }
            if let Some(detail) = hover
                .detail
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                detail_parts.push(detail.to_string());
            }

            Some(HoverItem {
                kind: "ty".into(),
                title: signature.title,
                detail: (!detail_parts.is_empty()).then_some(detail_parts.join("\n\n")),
                source: Some("Provided by ty language server".into()),
                start_line: hover.start_line,
                start_column: hover.start_column,
                end_line: hover.end_line,
                end_column: hover.end_column,
            })
        }
        (Some(hover), None) => Some(hover),
        (None, Some(signature)) => Some(signature),
        (None, None) => None,
    }
}

fn decode_lsp_semantic_tokens(response: &Value, token_types: &[String]) -> Vec<SemanticToken> {
    let Some(data) = response.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut tokens = Vec::new();
    let mut line = 0u32;
    let mut column = 0u32;
    let mut index = 0usize;

    while index + 4 < data.len() {
        let delta_line = data[index].as_u64().unwrap_or_default() as u32;
        let delta_start = data[index + 1].as_u64().unwrap_or_default() as u32;
        let length = data[index + 2].as_u64().unwrap_or_default() as u32;
        let token_type_index = data[index + 3].as_u64().unwrap_or_default() as usize;

        if delta_line == 0 {
            column = column.saturating_add(delta_start);
        } else {
            line = line.saturating_add(delta_line);
            column = delta_start;
        }

        if length > 0 {
            let kind = token_types
                .get(token_type_index)
                .cloned()
                .unwrap_or_else(|| "identifier".into());
            tokens.push(SemanticToken {
                kind,
                start_line: line + 1,
                start_column: column + 1,
                end_line: line + 1,
                end_column: column + length + 1,
            });
        }

        index += 5;
    }

    tokens
}

fn hover_item_from_lsp(
    value: &Value,
    fallback_line: u32,
    fallback_column: u32,
) -> Option<HoverItem> {
    hover_item_from_lsp_with_provider(
        value,
        fallback_line,
        fallback_column,
        "ty",
        "Provided by ty language server",
    )
}

fn hover_item_from_lsp_with_provider(
    value: &Value,
    fallback_line: u32,
    fallback_column: u32,
    kind: &str,
    source: &str,
) -> Option<HoverItem> {
    let contents = value.get("contents")?;
    let text = lsp_markup_to_text(contents)?;
    let blocks = text
        .split("\n\n")
        .map(str::trim)
        .filter(|block| !block.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let (title, detail) = hover_title_and_detail(&blocks, "ty hover");
    let range = value.get("range");
    let start_line = range
        .and_then(|value| value.get("start"))
        .and_then(|value| value.get("line"))
        .and_then(Value::as_u64)
        .map(|value| value as u32 + 1)
        .unwrap_or(fallback_line);
    let start_column = range
        .and_then(|value| value.get("start"))
        .and_then(|value| value.get("character"))
        .and_then(Value::as_u64)
        .map(|value| value as u32 + 1)
        .unwrap_or(fallback_column);
    let end_line = range
        .and_then(|value| value.get("end"))
        .and_then(|value| value.get("line"))
        .and_then(Value::as_u64)
        .map(|value| value as u32 + 1)
        .unwrap_or(start_line);
    let end_column = range
        .and_then(|value| value.get("end"))
        .and_then(|value| value.get("character"))
        .and_then(Value::as_u64)
        .map(|value| value as u32 + 1)
        .unwrap_or(start_column + 1);

    Some(HoverItem {
        kind: kind.into(),
        title,
        detail: (!detail.is_empty()).then_some(detail),
        source: Some(source.into()),
        start_line,
        start_column,
        end_line,
        end_column,
    })
}

fn hover_title_and_detail(blocks: &[String], fallback_title: &str) -> (String, String) {
    if blocks.is_empty() {
        return (fallback_title.to_string(), String::new());
    }

    let title_index = blocks
        .iter()
        .position(|block| looks_like_hover_signature(block))
        .unwrap_or(0);
    let title = blocks[title_index].clone();
    let detail = blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| (index != title_index).then_some(block.as_str()))
        .collect::<Vec<_>>()
        .join("\n\n");

    (title, detail)
}

fn looks_like_hover_signature(block: &str) -> bool {
    let trimmed = block.trim();
    trimmed.contains('(')
        || trimmed.contains(" -> ")
        || trimmed.starts_with("pub ")
        || trimmed.starts_with("fn ")
        || trimmed.starts_with("struct ")
        || trimmed.starts_with("enum ")
        || trimmed.starts_with("trait ")
        || trimmed.starts_with("impl ")
        || trimmed.starts_with("type ")
        || trimmed.starts_with("let ")
        || trimmed.starts_with("const ")
}

fn signature_help_item_from_lsp(value: &Value, call: &CallSignaturePosition) -> Option<HoverItem> {
    let signatures = value.get("signatures").and_then(Value::as_array)?;
    if signatures.is_empty() {
        return None;
    }

    let active_signature = value
        .get("activeSignature")
        .and_then(Value::as_u64)
        .unwrap_or_default() as usize;
    let signature = signatures
        .get(active_signature)
        .or_else(|| signatures.first())?;
    let title = signature.get("label").and_then(Value::as_str)?.to_string();
    let mut detail_parts = Vec::new();

    if let Some(documentation) = signature
        .get("documentation")
        .and_then(lsp_markup_to_text)
        .filter(|value| !value.trim().is_empty())
    {
        detail_parts.push(documentation);
    }

    if let Some(parameters) = signature.get("parameters").and_then(Value::as_array) {
        let parameter_docs = parameters
            .iter()
            .filter_map(|parameter| signature_parameter_detail(parameter, &title))
            .collect::<Vec<_>>();
        if !parameter_docs.is_empty() {
            detail_parts.push(format!("Args\n{}", parameter_docs.join("\n")));
        }
    }

    Some(HoverItem {
        kind: "ty signature".into(),
        title,
        detail: (!detail_parts.is_empty()).then_some(detail_parts.join("\n\n")),
        source: Some("Provided by ty signatureHelp".into()),
        start_line: call.start_line,
        start_column: call.start_column,
        end_line: call.end_line,
        end_column: call.end_column,
    })
}

fn signature_parameter_detail(parameter: &Value, signature_label: &str) -> Option<String> {
    let label = parameter
        .get("label")
        .and_then(|value| signature_parameter_label_text(value, signature_label))?;
    let documentation = parameter
        .get("documentation")
        .and_then(lsp_markup_to_text)
        .filter(|value| !value.trim().is_empty());

    Some(match documentation {
        Some(documentation) => format!("{label}: {documentation}"),
        None => label,
    })
}

fn signature_parameter_label_text(value: &Value, signature_label: &str) -> Option<String> {
    if let Some(label) = value.as_str() {
        return Some(label.to_string());
    }

    let range = value.as_array()?;
    let start = range.first()?.as_u64()? as usize;
    let end = range.get(1)?.as_u64()? as usize;
    if start >= end {
        return None;
    }

    let label = signature_label
        .chars()
        .skip(start)
        .take(end - start)
        .collect::<String>();
    (!label.is_empty()).then_some(label)
}

fn lsp_markup_to_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return normalize_lsp_markup_text(text);
    }

    if let Some(text) = value.get("value").and_then(Value::as_str) {
        return normalize_lsp_markup_text(text);
    }

    if let Some(items) = value.as_array() {
        let combined = items
            .iter()
            .filter_map(lsp_markup_to_text)
            .collect::<Vec<_>>()
            .join("\n");
        return (!combined.trim().is_empty()).then_some(combined);
    }

    None
}

fn normalize_lsp_markup_text(raw: &str) -> Option<String> {
    let mut lines = Vec::new();
    let mut in_code_fence = false;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_fence = !in_code_fence;
            continue;
        }

        if trimmed == "---" && !in_code_fence {
            if !lines.last().is_some_and(|line: &String| line.is_empty()) {
                lines.push(String::new());
            }
            continue;
        }

        lines.push(line.trim_end().to_string());
    }

    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    let text = lines.join("\n");
    (!text.trim().is_empty()).then_some(text)
}

fn parse_lsp_diagnostics(response: &Value, tool: &str, source: &str) -> Vec<EditorDiagnostic> {
    let Some(items) = response.get("items").and_then(Value::as_array) else {
        return Vec::new();
    };

    parse_lsp_diagnostic_items(items, tool, source)
}

#[cfg(test)]
fn parse_lsp_publish_diagnostics(
    params: &Value,
    tool: &str,
    source: &str,
) -> Vec<EditorDiagnostic> {
    let Some(items) = params.get("diagnostics").and_then(Value::as_array) else {
        return Vec::new();
    };

    parse_lsp_diagnostic_items(items, tool, source)
}

fn parse_lsp_diagnostic_items(items: &[Value], tool: &str, source: &str) -> Vec<EditorDiagnostic> {
    items
        .iter()
        .filter_map(|item| editor_diagnostic_from_lsp(item, tool, source))
        .collect()
}

fn editor_diagnostic_from_lsp(item: &Value, tool: &str, source: &str) -> Option<EditorDiagnostic> {
    let range = item.get("range")?;
    let start = range.get("start")?;
    let end = range.get("end").unwrap_or(start);
    let message = item.get("message")?.as_str()?.to_string();
    let line = start
        .get("line")
        .and_then(Value::as_u64)
        .unwrap_or_default() as u32
        + 1;
    let column = start
        .get("character")
        .and_then(Value::as_u64)
        .unwrap_or_default() as u32
        + 1;
    let end_line = end
        .get("line")
        .and_then(Value::as_u64)
        .unwrap_or(line as u64 - 1) as u32
        + 1;
    let end_column = end
        .get("character")
        .and_then(Value::as_u64)
        .unwrap_or(column as u64) as u32
        + 1;
    let severity = match item.get("severity").and_then(Value::as_u64) {
        Some(1) => "error",
        Some(2) => "warning",
        Some(3) => "info",
        Some(4) => "info",
        _ => "warning",
    };
    let diagnostic_source = item.get("source").and_then(Value::as_str);
    let code = item
        .get("code")
        .and_then(|value| value_to_string(Some(value)))
        .filter(|value| !value.trim().is_empty());
    let module = match (diagnostic_source, code.as_deref()) {
        (Some(source), Some(code)) if source != tool => format!("{} {}", source, code),
        (_, Some(code)) => code.to_string(),
        (Some(source), None) => source.to_string(),
        (None, None) => tool.to_string(),
    };

    Some(EditorDiagnostic {
        module,
        from: offset_from_line_column(source, line, column),
        to: offset_from_line_column(source, end_line, end_column),
        line,
        column,
        severity: severity.into(),
        message,
    })
}

fn parse_lsp_completion_items(response: &Value) -> Vec<EditorCompletionItem> {
    lsp_result_array(response)
        .into_iter()
        .filter_map(editor_completion_item_from_lsp)
        .collect()
}

fn editor_completion_item_from_lsp(item: &Value) -> Option<EditorCompletionItem> {
    let label = item.get("label")?.as_str()?.to_string();
    let detail = item
        .get("detail")
        .and_then(Value::as_str)
        .map(str::to_string);
    let insert_text = item
        .get("insertText")
        .and_then(Value::as_str)
        .map(str::to_string);
    let kind = lsp_completion_kind_label(item.get("kind").and_then(Value::as_u64)).to_string();

    Some(EditorCompletionItem {
        label,
        detail,
        kind,
        insert_text,
    })
}

fn parse_lsp_code_actions(response: &Value) -> Vec<EditorCodeAction> {
    lsp_result_array(response)
        .into_iter()
        .filter_map(|item| {
            Some(EditorCodeAction {
                title: item.get("title")?.as_str()?.to_string(),
                kind: item.get("kind").and_then(Value::as_str).map(str::to_string),
                edit: item.get("edit").cloned(),
                command: item.get("command").cloned(),
                is_preferred: item
                    .get("isPreferred")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect()
}

fn parse_lsp_locations(response: &Value) -> Vec<EditorLocation> {
    lsp_result_array(response)
        .into_iter()
        .filter_map(editor_location_from_lsp)
        .collect()
}

fn editor_location_from_lsp(item: &Value) -> Option<EditorLocation> {
    let uri = item
        .get("uri")
        .or_else(|| item.get("targetUri"))?
        .as_str()?
        .to_string();
    let range = item
        .get("targetSelectionRange")
        .or_else(|| item.get("targetRange"))
        .or_else(|| item.get("range"))?;
    let start = range.get("start")?;
    let end = range.get("end").unwrap_or(start);
    let line = lsp_position_line(start);
    let column = lsp_position_column(start);
    let end_line = lsp_position_line(end);
    let end_column = lsp_position_column(end);

    Some(EditorLocation {
        path: file_uri_to_path_string(&uri),
        uri,
        line,
        column,
        end_line,
        end_column,
    })
}

fn parse_lsp_inlay_hints(response: &Value) -> Vec<EditorInlayHint> {
    lsp_result_array(response)
        .into_iter()
        .filter_map(|item| {
            let position = item.get("position")?;
            let label = lsp_hint_label(item.get("label")?)?;
            Some(EditorInlayHint {
                label,
                line: lsp_position_line(position),
                column: lsp_position_column(position),
                kind: lsp_inlay_hint_kind_label(item.get("kind").and_then(Value::as_u64))
                    .to_string(),
            })
        })
        .collect()
}

fn lsp_result_array(response: &Value) -> Vec<&Value> {
    if let Some(items) = response.as_array() {
        return items.iter().collect();
    }
    if let Some(items) = response.get("items").and_then(Value::as_array) {
        return items.iter().collect();
    }
    if let Some(items) = response
        .get("result")
        .and_then(|result| result.get("items"))
        .and_then(Value::as_array)
    {
        return items.iter().collect();
    }
    if let Some(items) = response.get("result").and_then(Value::as_array) {
        return items.iter().collect();
    }
    Vec::new()
}

fn lsp_position_line(position: &Value) -> u32 {
    position
        .get("line")
        .and_then(Value::as_u64)
        .unwrap_or_default() as u32
        + 1
}

fn lsp_position_column(position: &Value) -> u32 {
    position
        .get("character")
        .and_then(Value::as_u64)
        .unwrap_or_default() as u32
        + 1
}

fn lsp_hint_label(label: &Value) -> Option<String> {
    if let Some(value) = label.as_str() {
        return Some(value.to_string());
    }
    label.as_array().map(|parts| {
        parts
            .iter()
            .filter_map(|part| part.get("value").and_then(Value::as_str))
            .collect::<String>()
    })
}

fn lsp_completion_kind_label(kind: Option<u64>) -> &'static str {
    match kind {
        Some(2) => "method",
        Some(3) => "function",
        Some(4) => "constructor",
        Some(5) => "field",
        Some(6) => "variable",
        Some(7) => "class",
        Some(8) => "interface",
        Some(9) => "module",
        Some(10) => "property",
        Some(12) => "value",
        Some(13) => "enum",
        Some(14) => "keyword",
        Some(15) => "snippet",
        Some(16) => "text",
        Some(17) => "color",
        Some(18) => "file",
        Some(21) => "constant",
        Some(22) => "struct",
        Some(23) => "event",
        Some(24) => "operator",
        Some(25) => "type-parameter",
        _ => "symbol",
    }
}

fn lsp_inlay_hint_kind_label(kind: Option<u64>) -> &'static str {
    match kind {
        Some(1) => "type",
        Some(2) => "parameter",
        _ => "hint",
    }
}

fn file_uri_to_path_string(uri: &str) -> Option<String> {
    let raw = uri.strip_prefix("file://")?;
    let raw = if raw.starts_with('/') && raw.get(2..3) == Some(":") {
        &raw[1..]
    } else {
        raw
    };
    Some(percent_decode_uri_path(raw))
}

fn percent_decode_uri_path(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[index + 1..index + 3]) {
                if let Ok(decoded) = u8::from_str_radix(hex, 16) {
                    output.push(decoded);
                    index += 3;
                    continue;
                }
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).to_string()
}

fn path_to_file_uri(path: &Path) -> String {
    let absolute = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    path_string_to_file_uri(&path_to_string(&absolute))
}

fn path_string_to_file_uri(path: &str) -> String {
    let mut raw = path.replace('\\', "/");

    #[cfg(target_os = "windows")]
    {
        if let Some(value) = raw.strip_prefix("//?/UNC/") {
            raw = format!("//{value}");
        } else if let Some(value) = raw.strip_prefix("//?/") {
            raw = value.to_string();
        }

        if let Some(authority_path) = raw.strip_prefix("//") {
            return format!("file://{}", percent_encode_file_uri_path(authority_path));
        }

        if !raw.starts_with('/') {
            raw.insert(0, '/');
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if !raw.starts_with('/') {
            raw.insert(0, '/');
        }
    }

    format!("file://{}", percent_encode_file_uri_path(&raw))
}

fn percent_encode_file_uri_path(path: &str) -> String {
    let mut encoded = String::new();
    for byte in path.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char)
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

fn should_run_automatic_python_analysis(content: &str) -> bool {
    content.len() <= MAX_AUTOMATIC_PYTHON_ANALYSIS_BYTES
}

fn should_run_automatic_rust_analysis(content: &str) -> bool {
    content.len() <= MAX_AUTOMATIC_RUST_ANALYSIS_BYTES
}

fn should_run_automatic_c_family_analysis(content: &str) -> bool {
    content.len() <= MAX_AUTOMATIC_C_FAMILY_ANALYSIS_BYTES
}

#[tauri::command]
async fn analyze_python_imports(
    request: PythonImportRequest,
) -> Result<PythonImportResponse, String> {
    tauri::async_runtime::spawn_blocking(move || resolve_python_imports(request, false))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn install_missing_python_imports(
    request: PythonImportRequest,
) -> Result<PythonImportResponse, String> {
    tauri::async_runtime::spawn_blocking(move || resolve_python_imports(request, true))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn run_python_tooling_action(
    request: PythonToolingRequest,
) -> Result<ProcessOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || run_python_tooling_action_sync(request))
        .await
        .map_err(|err| err.to_string())?
}

fn run_python_tooling_action_sync(request: PythonToolingRequest) -> Result<ProcessOutcome, String> {
    let root = PathBuf::from(&request.root);
    let file_path = PathBuf::from(&request.file_path);
    let spec = python_tooling_command_for_action(request.action, &file_path);

    if probe_command(spec.binary).is_none() {
        return Err(format!(
            "{} is not bundled or available on PATH.",
            spec.binary
        ));
    }

    let mut prepared = prepare_cli_command(spec.binary, &spec.args);
    prepared.command.current_dir(&root);
    apply_workspace_env(&mut prepared.command, &root);
    hide_background_window(&mut prepared.command);

    let output = prepared.command.output().map_err(|err| {
        format!(
            "Failed to run `{}` for Python tooling. {}",
            spec.binary, err
        )
    })?;

    Ok(ProcessOutcome {
        success: output.status.success(),
        command: prepared.preview.join(" "),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        diagnostics: Vec::new(),
    })
}

fn python_tooling_command_for_action(
    action: PythonToolingAction,
    file_path: &Path,
) -> PythonToolingCommand {
    let file_path = path_to_string(file_path);
    let args = match action {
        PythonToolingAction::Check => vec!["check".into(), file_path],
        PythonToolingAction::FixAll => vec![
            "check".into(),
            "--fix".into(),
            "--exit-zero".into(),
            file_path,
        ],
        PythonToolingAction::Format => vec!["format".into(), file_path],
        PythonToolingAction::OrganizeImports => vec![
            "check".into(),
            "--select".into(),
            "I".into(),
            "--fix".into(),
            "--exit-zero".into(),
            file_path,
        ],
        PythonToolingAction::TypeCheck => vec!["check".into(), file_path],
    };

    PythonToolingCommand {
        binary: match action {
            PythonToolingAction::TypeCheck => "ty",
            _ => "ruff",
        },
        args,
    }
}

#[tauri::command]
async fn run_rust_tooling_action(request: RustToolingRequest) -> Result<ProcessOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || run_rust_tooling_action_sync(request))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn analyze_rust_diagnostics(
    request: RustDiagnosticsRequest,
) -> Result<Vec<EditorDiagnostic>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = PathBuf::from(&request.root);
        let file_path = PathBuf::from(&request.file_path);
        let rust_root = find_rust_workspace_root(&file_path)
            .or_else(|| find_rust_workspace_root(&root))
            .unwrap_or(root);

        request_rust_analyzer_diagnostics(&rust_root, &file_path, &request.source)
    })
    .await
    .map_err(|err| err.to_string())?
}

fn run_rust_tooling_action_sync(request: RustToolingRequest) -> Result<ProcessOutcome, String> {
    let root = PathBuf::from(&request.root);
    let file_path = PathBuf::from(&request.file_path);
    let rust_root = find_rust_workspace_root(&file_path)
        .or_else(|| find_rust_workspace_root(&root))
        .unwrap_or(root);
    let spec = rust_tooling_command_for_action(request.action, &file_path);

    if probe_available_command(spec.binary).is_none() {
        return Err(format!(
            "{} is not installed or available on PATH.",
            spec.binary
        ));
    }

    if matches!(request.action, RustToolingAction::Clippy)
        && probe_available_command("cargo-clippy").is_none()
        && probe_available_command("clippy-driver").is_none()
    {
        return Err("Clippy is not installed. Run `rustup component add clippy`.".into());
    }

    let mut prepared = prepare_cli_command(spec.binary, &spec.args);
    prepared.command.current_dir(&rust_root);
    hide_background_window(&mut prepared.command);

    let output = prepared
        .command
        .output()
        .map_err(|err| format!("Failed to run `{}` for Rust tooling. {}", spec.binary, err))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let diagnostics = if spec.parses_diagnostics {
        let source = fs::read_to_string(&file_path).unwrap_or_default();
        parse_rust_tooling_diagnostics(&stdout, &rust_root, &file_path, &source)
    } else {
        Vec::new()
    };

    Ok(ProcessOutcome {
        success: output.status.success(),
        command: prepared.preview.join(" "),
        stdout,
        stderr,
        diagnostics,
    })
}

fn rust_tooling_command_for_action(
    action: RustToolingAction,
    file_path: &Path,
) -> RustToolingCommand {
    match action {
        RustToolingAction::Check => RustToolingCommand {
            binary: "cargo",
            args: vec!["check".into(), "--message-format=json".into()],
            parses_diagnostics: true,
        },
        RustToolingAction::Clippy => RustToolingCommand {
            binary: "cargo",
            args: vec!["clippy".into(), "--message-format=json".into()],
            parses_diagnostics: true,
        },
        RustToolingAction::Format => RustToolingCommand {
            binary: "rustfmt",
            args: vec![path_to_string(file_path)],
            parses_diagnostics: false,
        },
        RustToolingAction::Test => RustToolingCommand {
            binary: "cargo",
            args: vec!["test".into(), "--message-format=json".into()],
            parses_diagnostics: true,
        },
        RustToolingAction::Build => RustToolingCommand {
            binary: "cargo",
            args: vec!["build".into(), "--message-format=json".into()],
            parses_diagnostics: true,
        },
        RustToolingAction::Doc => RustToolingCommand {
            binary: "cargo",
            args: vec!["doc".into(), "--no-deps".into()],
            parses_diagnostics: false,
        },
        RustToolingAction::Metadata => RustToolingCommand {
            binary: "cargo",
            args: vec![
                "metadata".into(),
                "--no-deps".into(),
                "--format-version".into(),
                "1".into(),
            ],
            parses_diagnostics: false,
        },
    }
}

fn resolve_python_imports(
    request: PythonImportRequest,
    force_install: bool,
) -> Result<PythonImportResponse, String> {
    let root = PathBuf::from(&request.root);
    let current_file = PathBuf::from(&request.file_path);

    if !force_install && !should_run_automatic_python_analysis(&request.source) {
        return Ok(PythonImportResponse {
            environment_ready: probe_command("uv").is_some(),
            environment_path: Some(path_to_string(&venv_python_path(&root))),
            diagnostics: Vec::new(),
            events: Vec::new(),
        });
    }

    let candidates = collect_python_imports(&request.source)?;
    let mut diagnostics = analyze_python_with_ruff_and_ty(&root, &current_file, &request.source);
    drop_importable_missing_import_diagnostics(&root, &mut diagnostics, &candidates);
    let mut events = Vec::new();
    let mut missing_candidates = missing_imports_from_ty_diagnostics(&diagnostics, &candidates);

    if force_install || request.auto_install {
        let uv_path = match probe_command("uv") {
            Some(path) => path,
            None => {
                diagnostics.push(EditorDiagnostic {
                    module: "uv".into(),
                    from: 0,
                    to: 0,
                    line: 0,
                    column: 0,
                    severity: "warning".into(),
                    message: "astral-uv is not bundled or available on PATH, so package repair is paused.".into(),
                });

                return Ok(PythonImportResponse {
                    environment_ready: false,
                    environment_path: None,
                    diagnostics,
                    events,
                });
            }
        };

        if !missing_candidates.is_empty() {
            ensure_python_environment(&root, &uv_path)?;
        }

        let mut installed_any = false;
        for candidate in missing_candidates.clone() {
            let package = python_package_name(&candidate.module);
            let install_key = python_install_key(&root, &package);

            if python_install_in_progress(&install_key) {
                events.push(PythonImportEvent {
                    module: candidate.module.clone(),
                    package: package.clone(),
                    success: false,
                    state: "in_progress".into(),
                    command: install_command_preview(&root, &package),
                    output: "Hematite is already installing this package in the current workspace."
                        .into(),
                });
                continue;
            }

            if !force_install {
                if let Some(remaining) = python_install_cooldown_remaining(&install_key) {
                    events.push(PythonImportEvent {
                        module: candidate.module.clone(),
                        package: package.clone(),
                        success: false,
                        state: "cooldown".into(),
                        command: install_command_preview(&root, &package),
                        output: format!(
                            "A recent uv install attempt failed. Hematite will retry automatically in about {}s.",
                            remaining.as_secs()
                        ),
                    });
                    continue;
                }
            }

            mark_python_install_started(&install_key);
            let install_result = install_python_package(&root, &uv_path, &package);
            match install_result {
                Ok(output) => {
                    let resolved = python_module_exists(&root, &candidate.module).unwrap_or(false);
                    if resolved {
                        installed_any = true;
                        clear_python_install_failure(&install_key);
                    } else {
                        mark_python_install_failed(&install_key);
                    }
                    events.push(PythonImportEvent {
                        module: candidate.module.clone(),
                        package: package.clone(),
                        success: resolved,
                        state: if resolved {
                            "installed".into()
                        } else {
                            "failed".into()
                        },
                        command: install_command_preview(&root, &package),
                        output,
                    });
                    clear_python_install_started(&install_key);
                }
                Err(output) => {
                    mark_python_install_failed(&install_key);
                    clear_python_install_started(&install_key);
                    events.push(PythonImportEvent {
                        module: candidate.module.clone(),
                        package: package.clone(),
                        success: false,
                        state: "failed".into(),
                        command: install_command_preview(&root, &package),
                        output,
                    });
                }
            }
        }

        if installed_any {
            reset_ty_lsp_session();
        }

        diagnostics = analyze_python_with_ruff_and_ty(&root, &current_file, &request.source);
        drop_importable_missing_import_diagnostics(&root, &mut diagnostics, &candidates);
        missing_candidates = missing_imports_from_ty_diagnostics(&diagnostics, &candidates);

        for candidate in &missing_candidates {
            let has_existing = diagnostics.iter().any(|diagnostic| {
                diagnostic.module == format!("{PYTHON_MISSING_IMPORT_PREFIX}{}", candidate.module)
            });
            if !has_existing {
                events.push(PythonImportEvent {
                    module: candidate.module.clone(),
                    package: python_package_name(&candidate.module),
                    success: false,
                    state: "unresolved".into(),
                    command: install_command_preview(
                        &root,
                        &python_package_name(&candidate.module),
                    ),
                    output:
                        "ty still reports this import as unresolved after the last repair pass."
                            .into(),
                });
            }
        }
    } else {
        tag_missing_import_diagnostics(&mut diagnostics, &missing_candidates);
    }

    tag_missing_import_diagnostics(&mut diagnostics, &missing_candidates);

    Ok(PythonImportResponse {
        environment_ready: probe_command("uv").is_some(),
        environment_path: Some(path_to_string(&venv_python_path(&root))),
        diagnostics,
        events,
    })
}

fn analyze_python_with_ruff_and_ty(
    root: &Path,
    file_path: &Path,
    source: &str,
) -> Vec<EditorDiagnostic> {
    let mut diagnostics = Vec::new();

    match run_ruff_diagnostics(root, file_path, source) {
        Ok(mut items) => diagnostics.append(&mut items),
        Err(error) => diagnostics.push(EditorDiagnostic {
            module: "ruff".into(),
            from: 0,
            to: 0,
            line: 0,
            column: 0,
            severity: "warning".into(),
            message: error,
        }),
    }

    match run_ty_diagnostics(root, file_path, source) {
        Ok(mut items) => diagnostics.append(&mut items),
        Err(error) => diagnostics.push(EditorDiagnostic {
            module: "ty".into(),
            from: 0,
            to: 0,
            line: 0,
            column: 0,
            severity: "warning".into(),
            message: error,
        }),
    }

    diagnostics
}

fn run_ty_diagnostics(
    root: &Path,
    file_path: &Path,
    source: &str,
) -> Result<Vec<EditorDiagnostic>, String> {
    match request_ty_diagnostics(root, file_path, source) {
        Ok(items) => Ok(items),
        Err(lsp_error) => run_ty_cli_diagnostics(root, file_path, source).map_err(|cli_error| {
            format!("ty diagnostics unavailable. LSP: {lsp_error}; CLI: {cli_error}")
        }),
    }
}

fn run_ruff_diagnostics(
    root: &Path,
    file_path: &Path,
    source: &str,
) -> Result<Vec<EditorDiagnostic>, String> {
    if probe_command("ruff").is_none() {
        return Err("Ruff is not bundled or available on PATH.".into());
    }

    let file_name = path_to_string(file_path);
    let args = vec![
        "check".to_string(),
        "--output-format".to_string(),
        "json".to_string(),
        "--stdin-filename".to_string(),
        file_name,
        "-".to_string(),
    ];
    let mut prepared = prepare_cli_command("ruff", &args);
    prepared
        .command
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_workspace_env(&mut prepared.command, root);
    hide_background_window(&mut prepared.command);

    let mut process = prepared.command.spawn().map_err(|err| err.to_string())?;
    if let Some(mut stdin) = process.stdin.take() {
        stdin
            .write_all(source.as_bytes())
            .map_err(|err| err.to_string())?;
    }
    let output = process.wait_with_output().map_err(|err| err.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    if stdout.trim().is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() || output.status.success() {
            return Ok(Vec::new());
        }
        return Err(stderr);
    }

    parse_ruff_json_diagnostics(&stdout, source)
}

fn parse_ruff_json_diagnostics(raw: &str, source: &str) -> Result<Vec<EditorDiagnostic>, String> {
    let items = serde_json::from_str::<Vec<Value>>(raw).map_err(|err| err.to_string())?;
    Ok(items
        .iter()
        .filter_map(|item| editor_diagnostic_from_ruff(item, source))
        .collect())
}

fn editor_diagnostic_from_ruff(item: &Value, source: &str) -> Option<EditorDiagnostic> {
    let message = item.get("message")?.as_str()?.to_string();
    let code = item
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("ruff")
        .to_string();
    let location = item.get("location")?;
    let end_location = item.get("end_location").unwrap_or(location);
    let line = location.get("row").and_then(Value::as_u64).unwrap_or(1) as u32;
    let column = location.get("column").and_then(Value::as_u64).unwrap_or(1) as u32;
    let end_line = end_location
        .get("row")
        .and_then(Value::as_u64)
        .unwrap_or(line as u64) as u32;
    let end_column = end_location
        .get("column")
        .and_then(Value::as_u64)
        .unwrap_or(column as u64) as u32;

    Some(EditorDiagnostic {
        module: code,
        from: offset_from_line_column(source, line, column),
        to: offset_from_line_column(source, end_line, end_column),
        line,
        column,
        severity: if message.to_ascii_lowercase().contains("syntax") {
            "error".into()
        } else {
            "warning".into()
        },
        message: format!("Ruff: {message}"),
    })
}

fn run_ty_cli_diagnostics(
    root: &Path,
    file_path: &Path,
    source: &str,
) -> Result<Vec<EditorDiagnostic>, String> {
    if probe_command("ty").is_none() {
        return Err("ty is not bundled or available on PATH.".into());
    }

    let mut args = vec![
        "check".into(),
        "--output-format".into(),
        "concise".into(),
        "--color".into(),
        "never".into(),
        "--no-progress".into(),
        "--exit-zero".into(),
    ];
    if let Some(python) = ty_python_environment_for_root(root) {
        args.push("--python".into());
        args.push(python);
    }
    args.push(path_to_string(file_path));

    let mut prepared = prepare_cli_command("ty", &args);
    prepared.command.current_dir(root);
    apply_workspace_env(&mut prepared.command, root);
    hide_background_window(&mut prepared.command);

    let output = command_output_with_timeout(prepared.command, Duration::from_secs(6))
        .map_err(|err| format!("Failed to run `ty check`. {err}"))?
        .ok_or_else(|| "ty check did not finish within 6 seconds.".to_string())?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = if stderr.trim().is_empty() {
        stdout.to_string()
    } else if stdout.trim().is_empty() {
        stderr.to_string()
    } else {
        format!("{stdout}\n{stderr}")
    };

    if !output.status.success() && raw.trim().is_empty() {
        return Err("ty check returned a non-zero exit status without output.".into());
    }

    Ok(parse_ty_concise_diagnostics(&raw, file_path, source))
}

fn parse_ty_concise_diagnostics(
    raw: &str,
    active_file: &Path,
    source: &str,
) -> Vec<EditorDiagnostic> {
    raw.lines()
        .filter_map(|line| editor_diagnostic_from_ty_concise_line(line, active_file, source))
        .collect()
}

fn editor_diagnostic_from_ty_concise_line(
    line: &str,
    active_file: &Path,
    source: &str,
) -> Option<EditorDiagnostic> {
    let (location, rest) = line.rsplit_once(": ")?;
    let (path_and_line, column_text) = location.rsplit_once(':')?;
    let (path_text, line_text) = path_and_line.rsplit_once(':')?;
    let line_number = line_text.parse::<u32>().ok()?;
    let column = column_text.parse::<u32>().ok()?;
    if !ty_diagnostic_path_matches_active_file(path_text, active_file) {
        return None;
    }

    let (severity_code, message) = rest.split_once(' ').unwrap_or((rest, ""));
    let (severity_text, code) = if let Some((severity, code)) = severity_code.split_once('[') {
        (severity, code.trim_end_matches(']').to_string())
    } else {
        (severity_code, "ty".into())
    };
    let severity = match severity_text {
        "error" => "error",
        "warning" | "warn" => "warning",
        "info" | "note" | "hint" => "info",
        _ => "warning",
    };
    let from = offset_from_line_column(source, line_number, column);
    let to = ty_concise_diagnostic_end_offset(source, line_number, column).max(from + 1);

    Some(EditorDiagnostic {
        module: code,
        from,
        to,
        line: line_number,
        column,
        severity: severity.into(),
        message: message.to_string(),
    })
}

fn ty_diagnostic_path_matches_active_file(path_text: &str, active_file: &Path) -> bool {
    let diagnostic_path = PathBuf::from(path_text);
    normalized_path_for_compare(&diagnostic_path) == normalized_path_for_compare(active_file)
}

fn ty_concise_diagnostic_end_offset(source: &str, line: u32, column: u32) -> usize {
    let from = offset_from_line_column(source, line, column);
    let Some(line_text) = source.lines().nth(line.saturating_sub(1) as usize) else {
        return from + 1;
    };
    let start = column.saturating_sub(1) as usize;
    let token_len = line_text
        .chars()
        .skip(start)
        .take_while(|value| value.is_ascii_alphanumeric() || matches!(value, '_' | '.' | '-'))
        .map(char::len_utf8)
        .sum::<usize>();

    from + token_len.max(1)
}

fn parse_rust_tooling_diagnostics(
    raw: &str,
    root: &Path,
    active_file: &Path,
    source: &str,
) -> Vec<EditorDiagnostic> {
    raw.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|item| item.get("reason").and_then(Value::as_str) == Some("compiler-message"))
        .filter_map(|item| editor_diagnostic_from_cargo_message(&item, root, active_file, source))
        .collect()
}

fn editor_diagnostic_from_cargo_message(
    item: &Value,
    root: &Path,
    active_file: &Path,
    source: &str,
) -> Option<EditorDiagnostic> {
    let message = item.get("message")?;
    let text = message.get("message")?.as_str()?.to_string();
    let span = message
        .get("spans")
        .and_then(Value::as_array)?
        .iter()
        .find(|span| {
            span.get("is_primary")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .or_else(|| message.get("spans").and_then(Value::as_array)?.first())?;
    let file_name = span.get("file_name")?.as_str()?;
    if !rust_span_matches_active_file(root, active_file, file_name) {
        return None;
    }

    let line = span.get("line_start").and_then(Value::as_u64).unwrap_or(1) as u32;
    let column = span
        .get("column_start")
        .and_then(Value::as_u64)
        .unwrap_or(1) as u32;
    let end_line = span
        .get("line_end")
        .and_then(Value::as_u64)
        .unwrap_or(line as u64) as u32;
    let end_column = span
        .get("column_end")
        .and_then(Value::as_u64)
        .unwrap_or(column as u64) as u32;
    let severity = match message.get("level").and_then(Value::as_str) {
        Some("error") => "error",
        Some("warning") => "warning",
        Some("note") | Some("help") => "info",
        _ => "warning",
    };
    let code = message
        .get("code")
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("rustc")
        .to_string();

    Some(EditorDiagnostic {
        module: code,
        from: offset_from_line_column(source, line, column),
        to: offset_from_line_column(source, end_line, end_column),
        line,
        column,
        severity: severity.into(),
        message: text,
    })
}

fn rust_span_matches_active_file(root: &Path, active_file: &Path, span_file_name: &str) -> bool {
    let span_path = PathBuf::from(span_file_name);
    let candidate = if span_path.is_absolute() {
        span_path
    } else {
        root.join(span_path)
    };

    normalized_path_for_compare(&candidate) == normalized_path_for_compare(active_file)
}

fn normalized_path_for_compare(path: &Path) -> String {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let normalized = path_to_string(&path).replace('\\', "/");
    if cfg!(target_os = "windows") {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}

fn missing_imports_from_ty_diagnostics(
    diagnostics: &[EditorDiagnostic],
    candidates: &[ImportCandidate],
) -> Vec<ImportCandidate> {
    let mut missing = Vec::new();
    let mut seen = BTreeSet::new();

    for diagnostic in diagnostics {
        if !diagnostic_looks_like_missing_import(diagnostic) {
            continue;
        }

        for candidate in candidates {
            if diagnostic_matches_import_candidate(diagnostic, candidate)
                && seen.insert(candidate.module.clone())
            {
                missing.push(candidate.clone());
            }
        }
    }

    missing
}

fn drop_importable_missing_import_diagnostics(
    root: &Path,
    diagnostics: &mut Vec<EditorDiagnostic>,
    candidates: &[ImportCandidate],
) {
    if candidates.is_empty() || !venv_python_path(root).exists() {
        return;
    }

    let mut import_cache = BTreeMap::<String, bool>::new();
    diagnostics.retain(|diagnostic| {
        if !diagnostic_looks_like_missing_import(diagnostic) {
            return true;
        }

        let Some(candidate) = candidates
            .iter()
            .find(|candidate| diagnostic_matches_import_candidate(diagnostic, candidate))
        else {
            return true;
        };

        let exists = *import_cache
            .entry(candidate.module.clone())
            .or_insert_with(|| python_module_exists(root, &candidate.module).unwrap_or(false));
        !exists
    });
}

fn diagnostic_matches_import_candidate(
    diagnostic: &EditorDiagnostic,
    candidate: &ImportCandidate,
) -> bool {
    let message = diagnostic.message.to_ascii_lowercase();
    let module = candidate.module.to_ascii_lowercase();
    let line_matches = diagnostic.line == 0
        || diagnostic.line == candidate.line
        || diagnostic.line + 1 == candidate.line;
    let text_matches = message.contains(&module)
        || module
            .split('.')
            .next()
            .is_some_and(|value| message.contains(value));

    line_matches && text_matches
}

fn diagnostic_looks_like_missing_import(diagnostic: &EditorDiagnostic) -> bool {
    let message = diagnostic.message.to_ascii_lowercase();
    diagnostic.module.starts_with(PYTHON_MISSING_IMPORT_PREFIX)
        || diagnostic
            .module
            .to_ascii_lowercase()
            .contains("unresolved")
        || ((message.contains("import") || message.contains("module"))
            && (message.contains("resolve")
                || message.contains("found")
                || message.contains("missing")
                || message.contains("unknown")))
}

fn tag_missing_import_diagnostics(
    diagnostics: &mut [EditorDiagnostic],
    missing_candidates: &[ImportCandidate],
) {
    for diagnostic in diagnostics {
        if !diagnostic_looks_like_missing_import(diagnostic) {
            continue;
        }

        if let Some(candidate) = missing_candidates
            .iter()
            .find(|candidate| diagnostic_matches_import_candidate(diagnostic, candidate))
        {
            diagnostic.module = format!("{PYTHON_MISSING_IMPORT_PREFIX}{}", candidate.module);
            diagnostic.line = if diagnostic.line == 0 {
                candidate.line
            } else {
                diagnostic.line
            };
            diagnostic.column = if diagnostic.column == 0 {
                candidate.column
            } else {
                diagnostic.column
            };
        }
    }
}

fn offset_from_line_column(source: &str, line: u32, column: u32) -> usize {
    let target_line = line.max(1) as usize;
    let target_column = column.max(1) as usize;
    let mut offset = 0usize;

    for (index, current_line) in source.split_inclusive('\n').enumerate() {
        if index + 1 == target_line {
            return offset
                + current_line
                    .chars()
                    .take(target_column.saturating_sub(1))
                    .map(char::len_utf8)
                    .sum::<usize>();
        }
        offset += current_line.len();
    }

    source.len()
}

fn build_agent_health_payload() -> AgentHealthPayload {
    let stored = load_agent_credentials();
    AgentHealthPayload {
        agents: vec![
            codex_status(&stored),
            gemini_status(&stored),
            claude_status(&stored),
            kilo_status(&stored),
        ],
        credentials: credential_snapshot(&stored),
    }
}

fn codex_status(stored: &AgentCredentials) -> AgentStatus {
    let resolved_path = probe_command("codex");
    let openai_key = effective_value(&stored.openai_api_key, "OPENAI_API_KEY");
    let auth_file = user_home_dir().map(|path| path.join(".codex").join("auth.json"));

    if resolved_path.is_none() {
        return AgentStatus {
            id: "codex".into(),
            label: "OpenAI Codex".into(),
            available: false,
            resolved_path,
            auth_state: "unavailable".into(),
            auth_source: None,
            summary: "Codex CLI is not installed on PATH.".into(),
            supports_oauth: true,
            supports_api_key: true,
        };
    }

    if openai_key.is_some() {
        return AgentStatus {
            id: "codex".into(),
            label: "OpenAI Codex".into(),
            available: true,
            resolved_path,
            auth_state: "ready".into(),
            auth_source: Some("API key".into()),
            summary: "OpenAI API access is configured for Hematite-run Codex commands.".into(),
            supports_oauth: true,
            supports_api_key: true,
        };
    }

    if auth_file.is_some_and(|path| path.exists()) {
        return AgentStatus {
            id: "codex".into(),
            label: "OpenAI Codex".into(),
            available: true,
            resolved_path,
            auth_state: "ready".into(),
            auth_source: Some("Stored credentials".into()),
            summary: "Stored Codex credentials were detected in your user profile.".into(),
            supports_oauth: true,
            supports_api_key: true,
        };
    }

    let auth_args = vec!["login".to_string(), "status".to_string()];
    let output = command_output_with_timeout(
        prepare_cli_command("codex", &auth_args).command,
        Duration::from_secs(2),
    );
    match output {
        Ok(Some(result)) if result.status.success() => AgentStatus {
            id: "codex".into(),
            label: "OpenAI Codex".into(),
            available: true,
            resolved_path,
            auth_state: "ready".into(),
            auth_source: Some("ChatGPT login".into()),
            summary: non_empty_output(&result.stdout, &result.stderr)
                .unwrap_or_else(|| "Codex reports that you are logged in.".into()),
            supports_oauth: true,
            supports_api_key: true,
        },
        Ok(Some(result)) => AgentStatus {
            id: "codex".into(),
            label: "OpenAI Codex".into(),
            available: true,
            resolved_path,
            auth_state: "missing".into(),
            auth_source: None,
            summary: non_empty_output(&result.stdout, &result.stderr).unwrap_or_else(|| {
                "Codex CLI is installed, but no login or API key was detected.".into()
            }),
            supports_oauth: true,
            supports_api_key: true,
        },
        Ok(None) | Err(_) => AgentStatus {
            id: "codex".into(),
            label: "OpenAI Codex".into(),
            available: true,
            resolved_path,
            auth_state: "missing".into(),
            auth_source: None,
            summary: "Codex CLI is installed, but a quick login status check did not complete. Add an API key or open Codex login to verify credentials.".into(),
            supports_oauth: true,
            supports_api_key: true,
        },
    }
}

fn gemini_status(stored: &AgentCredentials) -> AgentStatus {
    let resolved_path = probe_command("gemini");
    let gemini_api_key = effective_value(&stored.gemini_api_key, "GEMINI_API_KEY");
    let google_api_key = effective_value(&stored.google_api_key, "GOOGLE_API_KEY");
    let project = effective_value(&stored.google_cloud_project, "GOOGLE_CLOUD_PROJECT");
    let location = effective_value(&stored.google_cloud_location, "GOOGLE_CLOUD_LOCATION");
    let app_credentials = effective_value(
        &stored.google_application_credentials,
        "GOOGLE_APPLICATION_CREDENTIALS",
    );
    let oauth_file = user_home_dir().map(|path| path.join(".gemini").join("oauth_creds.json"));

    if resolved_path.is_none() {
        return AgentStatus {
            id: "gemini".into(),
            label: "Gemini CLI".into(),
            available: false,
            resolved_path,
            auth_state: "unavailable".into(),
            auth_source: None,
            summary: "Gemini CLI is not installed on PATH.".into(),
            supports_oauth: true,
            supports_api_key: true,
        };
    }

    if gemini_api_key.is_some() {
        return AgentStatus {
            id: "gemini".into(),
            label: "Gemini CLI".into(),
            available: true,
            resolved_path,
            auth_state: "ready".into(),
            auth_source: Some("Gemini API key".into()),
            summary: "Gemini API access is configured through GEMINI_API_KEY.".into(),
            supports_oauth: true,
            supports_api_key: true,
        };
    }

    if oauth_file.is_some_and(|path| path.exists()) {
        return AgentStatus {
            id: "gemini".into(),
            label: "Gemini CLI".into(),
            available: true,
            resolved_path,
            auth_state: "ready".into(),
            auth_source: Some("Google sign-in".into()),
            summary: "Cached Gemini OAuth credentials were detected in your user profile.".into(),
            supports_oauth: true,
            supports_api_key: true,
        };
    }

    if app_credentials.is_some() && project.is_some() && location.is_some() {
        return AgentStatus {
            id: "gemini".into(),
            label: "Gemini CLI".into(),
            available: true,
            resolved_path,
            auth_state: "ready".into(),
            auth_source: Some("Vertex service account".into()),
            summary: "Vertex AI service account credentials and project settings are configured."
                .into(),
            supports_oauth: true,
            supports_api_key: true,
        };
    }

    if google_api_key.is_some() && project.is_some() && location.is_some() {
        return AgentStatus {
            id: "gemini".into(),
            label: "Gemini CLI".into(),
            available: true,
            resolved_path,
            auth_state: "ready".into(),
            auth_source: Some("Vertex API key".into()),
            summary: "GOOGLE_API_KEY plus Vertex project and location are configured.".into(),
            supports_oauth: true,
            supports_api_key: true,
        };
    }

    if google_api_key.is_some()
        || app_credentials.is_some()
        || project.is_some()
        || location.is_some()
    {
        return AgentStatus {
            id: "gemini".into(),
            label: "Gemini CLI".into(),
            available: true,
            resolved_path,
            auth_state: "partial".into(),
            auth_source: Some("Vertex setup".into()),
            summary: "Some Gemini or Vertex settings were found, but the setup is incomplete. Add the missing project, location, or credential values.".into(),
            supports_oauth: true,
            supports_api_key: true,
        };
    }

    AgentStatus {
        id: "gemini".into(),
        label: "Gemini CLI".into(),
        available: true,
        resolved_path,
        auth_state: "missing".into(),
        auth_source: None,
        summary: "No Google sign-in or Gemini API key was detected.".into(),
        supports_oauth: true,
        supports_api_key: true,
    }
}

fn claude_status(stored: &AgentCredentials) -> AgentStatus {
    let resolved_path = probe_command("claude");
    let anthropic_key = effective_value(&stored.anthropic_api_key, "ANTHROPIC_API_KEY");

    if resolved_path.is_none() {
        return AgentStatus {
            id: "claude".into(),
            label: "Claude Code".into(),
            available: false,
            resolved_path,
            auth_state: "unavailable".into(),
            auth_source: None,
            summary: if anthropic_key.is_some() {
                "ANTHROPIC_API_KEY is configured, but Claude Code CLI is not installed on PATH."
                    .into()
            } else {
                "Claude Code CLI is not installed on PATH.".into()
            },
            supports_oauth: true,
            supports_api_key: true,
        };
    }

    if anthropic_key.is_some() {
        return AgentStatus {
            id: "claude".into(),
            label: "Claude Code".into(),
            available: true,
            resolved_path,
            auth_state: "ready".into(),
            auth_source: Some("API key".into()),
            summary: "Anthropic API access is configured for Hematite-run Claude Code commands."
                .into(),
            supports_oauth: true,
            supports_api_key: true,
        };
    }

    if let Some(status) = read_claude_auth_status(stored) {
        if status.logged_in {
            let auth_source = match status.auth_method.as_deref() {
                Some("oauth") => Some("Claude login".into()),
                Some("apiKey") => Some("API key".into()),
                Some("firstParty") => Some("Claude login".into()),
                Some(other) if !other.trim().is_empty() => Some(other.to_string()),
                _ => Some("Claude login".into()),
            };

            let summary = match status.api_provider.as_deref() {
                Some(provider) if !provider.trim().is_empty() => {
                    format!(
                        "Claude Code reports that you are logged in via {}.",
                        provider
                    )
                }
                _ => "Claude Code reports that you are logged in.".into(),
            };

            return AgentStatus {
                id: "claude".into(),
                label: "Claude Code".into(),
                available: true,
                resolved_path,
                auth_state: "ready".into(),
                auth_source,
                summary,
                supports_oauth: true,
                supports_api_key: true,
            };
        }

        return AgentStatus {
            id: "claude".into(),
            label: "Claude Code".into(),
            available: true,
            resolved_path,
            auth_state: "missing".into(),
            auth_source: None,
            summary:
                "Claude Code is installed, but authentication is not configured yet. Run Claude login or add ANTHROPIC_API_KEY.".into(),
            supports_oauth: true,
            supports_api_key: true,
        };
    }

    AgentStatus {
        id: "claude".into(),
        label: "Claude Code".into(),
        available: true,
        resolved_path,
        auth_state: "missing".into(),
        auth_source: None,
        summary: "No ANTHROPIC_API_KEY was detected. You can also open Claude Code and use /login."
            .into(),
        supports_oauth: true,
        supports_api_key: true,
    }
}

fn kilo_status(stored: &AgentCredentials) -> AgentStatus {
    let resolved_path = probe_command("kilo");
    let kilo_key = effective_value(&stored.kilo_api_key, "KILO_API_KEY");
    let config_names = ["opencode.json", "opencode.jsonc", "kilo.jsonc"];
    let config_file = user_home_dir()
        .and_then(|home| {
            config_names
                .iter()
                .map(|name| home.join(".config").join("kilo").join(name))
                .find(|path| path.exists())
        })
        .or_else(|| {
            env::var_os("APPDATA").and_then(|raw| {
                let app_data = PathBuf::from(raw);
                config_names
                    .iter()
                    .map(|name| app_data.join("kilo").join(name))
                    .find(|path| path.exists())
            })
        });

    if resolved_path.is_none() {
        return AgentStatus {
            id: "kilo".into(),
            label: "Kilo Code".into(),
            available: false,
            resolved_path,
            auth_state: "unavailable".into(),
            auth_source: None,
            summary: if kilo_key.is_some() {
                "KILO_API_KEY is configured, but Kilo Code CLI is not bundled or installed.".into()
            } else {
                "Kilo Code CLI is not bundled or installed on PATH.".into()
            },
            supports_oauth: true,
            supports_api_key: true,
        };
    }

    if kilo_key.is_some() {
        return AgentStatus {
            id: "kilo".into(),
            label: "Kilo Code".into(),
            available: true,
            resolved_path,
            auth_state: "ready".into(),
            auth_source: Some("KILO_API_KEY".into()),
            summary: "Kilo API access is configured through KILO_API_KEY.".into(),
            supports_oauth: true,
            supports_api_key: true,
        };
    }

    if config_file.is_some() {
        return AgentStatus {
            id: "kilo".into(),
            label: "Kilo Code".into(),
            available: true,
            resolved_path,
            auth_state: "ready".into(),
            auth_source: Some("Kilo config".into()),
            summary: "Kilo provider configuration was detected in your user profile.".into(),
            supports_oauth: true,
            supports_api_key: true,
        };
    }

    AgentStatus {
        id: "kilo".into(),
        label: "Kilo Code".into(),
        available: true,
        resolved_path,
        auth_state: "missing".into(),
        auth_source: None,
        summary:
            "Kilo Code is available, but no provider config was detected. Open login or run /connect in Kilo.".into(),
        supports_oauth: true,
        supports_api_key: true,
    }
}

fn read_claude_auth_status(stored: &AgentCredentials) -> Option<ClaudeAuthStatusPayload> {
    let args = vec!["auth".to_string(), "status".to_string()];
    let mut prepared = prepare_cli_command("claude", &args);
    apply_agent_env(&mut prepared.command, stored);

    let output = command_output_with_timeout(prepared.command, Duration::from_secs(2))
        .ok()
        .flatten()?;
    let raw = non_empty_output(&output.stdout, &output.stderr)?;
    serde_json::from_str::<ClaudeAuthStatusPayload>(&raw).ok()
}

fn credential_snapshot(stored: &AgentCredentials) -> CredentialSnapshot {
    CredentialSnapshot {
        has_openai_api_key: effective_value(&stored.openai_api_key, "OPENAI_API_KEY").is_some(),
        has_gemini_api_key: effective_value(&stored.gemini_api_key, "GEMINI_API_KEY").is_some(),
        has_google_api_key: effective_value(&stored.google_api_key, "GOOGLE_API_KEY").is_some(),
        has_anthropic_api_key: effective_value(&stored.anthropic_api_key, "ANTHROPIC_API_KEY")
            .is_some(),
        has_kilo_api_key: effective_value(&stored.kilo_api_key, "KILO_API_KEY").is_some(),
        google_cloud_project: effective_value(&stored.google_cloud_project, "GOOGLE_CLOUD_PROJECT"),
        google_cloud_location: effective_value(
            &stored.google_cloud_location,
            "GOOGLE_CLOUD_LOCATION",
        ),
        google_application_credentials: effective_value(
            &stored.google_application_credentials,
            "GOOGLE_APPLICATION_CREDENTIALS",
        ),
    }
}

fn load_agent_credentials() -> AgentCredentials {
    let path = agent_credentials_path();
    let Ok(raw) = fs::read_to_string(path) else {
        return AgentCredentials::default();
    };

    serde_json::from_str(&raw).unwrap_or_default()
}

fn persist_agent_credentials(stored: &AgentCredentials) -> Result<(), String> {
    let path = agent_credentials_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let serialized = serde_json::to_string_pretty(stored).map_err(|err| err.to_string())?;
    fs::write(path, serialized).map_err(|err| err.to_string())
}

fn agent_credentials_path() -> PathBuf {
    hematite_config_dir().join("agent-credentials.json")
}

fn hematite_config_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir)
            .join("Hematite")
    } else {
        user_home_dir()
            .unwrap_or_else(env::temp_dir)
            .join(".config")
            .join("hematite")
    }
}

fn user_home_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(PathBuf::from))
}

fn merge_optional_value(target: &mut Option<String>, incoming: Option<String>) {
    let Some(value) = incoming else {
        return;
    };

    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }

    *target = Some(trimmed.to_string());
}

fn effective_value(stored: &Option<String>, env_key: &str) -> Option<String> {
    stored
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            env::var(env_key)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
}

fn non_empty_output(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();

    if !stdout.is_empty() {
        Some(stdout)
    } else if !stderr.is_empty() {
        Some(stderr)
    } else {
        None
    }
}

fn command_output_with_timeout(
    mut command: Command,
    timeout: Duration,
) -> Result<Option<Output>, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    hide_background_window(&mut command);

    let mut child = command.spawn().map_err(|err| err.to_string())?;
    let started_at = Instant::now();

    loop {
        if child.try_wait().map_err(|err| err.to_string())?.is_some() {
            return child
                .wait_with_output()
                .map(Some)
                .map_err(|err| err.to_string());
        }

        if started_at.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }

        thread::sleep(Duration::from_millis(25));
    }
}

fn spawn_external_terminal(
    binary: &str,
    args: &[&str],
    stored: &AgentCredentials,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let mut parts = Vec::with_capacity(args.len() + 2);
        parts.push("&".to_string());
        parts.push(powershell_quote(binary));
        for arg in args {
            parts.push(powershell_quote(arg));
        }

        let inline = parts.join(" ");
        let mut command = Command::new("powershell.exe");
        command.args(["-NoExit", "-Command", &inline]);
        command.creation_flags(CREATE_NEW_CONSOLE);
        apply_agent_env(&mut command, stored);
        command.spawn().map_err(|err| err.to_string())?;
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut command = Command::new(binary);
        command.args(args);
        apply_agent_env(&mut command, stored);
        command.spawn().map_err(|err| err.to_string())?;
        Ok(())
    }
}

fn codex_app_server_state() -> &'static Mutex<CodexAppServerState> {
    CODEX_APP_SERVER.get_or_init(|| Mutex::new(CodexAppServerState::default()))
}

fn codex_model_override() -> Option<String> {
    CODEX_MODEL_OVERRIDE
        .get_or_init(detect_codex_model_override)
        .clone()
}

fn codex_model_for_request(
    requested_model: Option<&str>,
    fallback_model: Option<&str>,
) -> Option<String> {
    requested_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            fallback_model
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .map(str::to_string)
}

fn apply_codex_model_param(
    params: &mut Value,
    requested_model: Option<&str>,
    fallback_model: Option<&str>,
) {
    if let Some(model) = codex_model_for_request(requested_model, fallback_model) {
        params["model"] = Value::String(model);
    }
}

fn normalized_agent_model(model: Option<&str>) -> Option<String> {
    model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentPermissionLevel {
    Ask,
    AutoEdits,
    Plan,
    FullAuto,
}

struct CodexExecutionSettings {
    approval_policy: &'static str,
    sandbox: &'static str,
}

fn resolved_agent_permission_level(
    permission_level: Option<&str>,
    default_level: AgentPermissionLevel,
) -> AgentPermissionLevel {
    match permission_level
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some("autoEdits" | "auto_edits" | "auto-edit" | "acceptEdits" | "auto_edit") => {
            AgentPermissionLevel::AutoEdits
        }
        Some("plan" | "readOnly" | "read_only" | "read-only") => AgentPermissionLevel::Plan,
        Some("fullAuto" | "full_auto" | "full-auto" | "yolo" | "dontAsk") => {
            AgentPermissionLevel::FullAuto
        }
        Some("ask" | "default" | "onRequest" | "on-request") => AgentPermissionLevel::Ask,
        _ => default_level,
    }
}

fn default_permission_level_for_binary(binary_key: &str) -> AgentPermissionLevel {
    match binary_key {
        "claude" => AgentPermissionLevel::AutoEdits,
        "kilo" => AgentPermissionLevel::FullAuto,
        _ => AgentPermissionLevel::Ask,
    }
}

fn claude_permission_mode_for_level(permission_level: AgentPermissionLevel) -> &'static str {
    match permission_level {
        AgentPermissionLevel::Ask => "default",
        AgentPermissionLevel::AutoEdits => "acceptEdits",
        AgentPermissionLevel::Plan => "plan",
        AgentPermissionLevel::FullAuto => "dontAsk",
    }
}

fn gemini_approval_mode_for_level(permission_level: AgentPermissionLevel) -> &'static str {
    match permission_level {
        AgentPermissionLevel::Ask => "default",
        AgentPermissionLevel::AutoEdits => "auto_edit",
        AgentPermissionLevel::Plan => "plan",
        AgentPermissionLevel::FullAuto => "yolo",
    }
}

fn codex_execution_settings_for_permission_level(
    permission_level: AgentPermissionLevel,
) -> CodexExecutionSettings {
    match permission_level {
        AgentPermissionLevel::Ask => CodexExecutionSettings {
            approval_policy: "on-request",
            sandbox: "workspace-write",
        },
        AgentPermissionLevel::AutoEdits => CodexExecutionSettings {
            approval_policy: "on-failure",
            sandbox: "workspace-write",
        },
        AgentPermissionLevel::Plan => CodexExecutionSettings {
            approval_policy: "never",
            sandbox: "read-only",
        },
        AgentPermissionLevel::FullAuto => CodexExecutionSettings {
            approval_policy: "never",
            sandbox: "workspace-write",
        },
    }
}

fn agent_args_with_selected_model(
    binary: &str,
    args: &[String],
    model: Option<&str>,
    permission_level: Option<&str>,
) -> Vec<String> {
    let binary_key = Path::new(binary)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(binary)
        .to_ascii_lowercase();
    let model = normalized_agent_model(model);
    let permission_level = resolved_agent_permission_level(
        permission_level,
        default_permission_level_for_binary(&binary_key),
    );

    match binary_key.as_str() {
        "claude" => {
            claude_args_with_noninteractive_permissions(args, model.as_deref(), permission_level)
        }
        "gemini" => {
            let has_approval_mode = args
                .iter()
                .any(|value| value == "--approval-mode" || value.starts_with("--approval-mode="));
            let mut resolved =
                Vec::with_capacity(args.len() + if model.is_some() { 2 } else { 0 } + 2);
            if let Some(model) = model {
                resolved.push("--model".into());
                resolved.push(model);
            }
            if !has_approval_mode {
                resolved.push("--approval-mode".into());
                resolved.push(gemini_approval_mode_for_level(permission_level).into());
            }
            resolved.extend_from_slice(args);
            resolved
        }
        "kilo" => kilo_args_with_permissions(args, model.as_deref(), permission_level),
        _ => args.to_vec(),
    }
}

fn claude_args_with_noninteractive_permissions(
    args: &[String],
    model: Option<&str>,
    permission_level: AgentPermissionLevel,
) -> Vec<String> {
    let has_permission_mode = args
        .iter()
        .any(|value| value == "--permission-mode" || value.starts_with("--permission-mode="));
    let extra_capacity =
        if has_permission_mode { 0 } else { 2 } + if model.is_some() { 2 } else { 0 };
    let mut resolved = Vec::with_capacity(args.len() + extra_capacity);

    if !has_permission_mode {
        resolved.push("--permission-mode".into());
        resolved.push(claude_permission_mode_for_level(permission_level).into());
    }
    if let Some(model) = model {
        resolved.push("--model".into());
        resolved.push(model.into());
    }
    resolved.extend_from_slice(args);
    resolved
}

fn kilo_args_with_permissions(
    args: &[String],
    model: Option<&str>,
    permission_level: AgentPermissionLevel,
) -> Vec<String> {
    let body = args
        .iter()
        .filter(|value| value.as_str() != "--auto")
        .cloned()
        .collect::<Vec<_>>();
    let auto = permission_level == AgentPermissionLevel::FullAuto;

    if body.first().is_some_and(|value| value == "run") {
        let mut resolved = Vec::with_capacity(
            body.len() + if model.is_some() { 2 } else { 0 } + usize::from(auto),
        );
        resolved.push(body[0].clone());
        if let Some(model) = model {
            resolved.push("--model".into());
            resolved.push(model.into());
        }
        if auto {
            resolved.push("--auto".into());
        }
        resolved.extend_from_slice(&body[1..]);
        return resolved;
    }

    let mut resolved =
        Vec::with_capacity(body.len() + if model.is_some() { 2 } else { 0 } + usize::from(auto));
    if let Some(model) = model {
        resolved.push("--model".into());
        resolved.push(model.into());
    }
    if auto {
        resolved.push("--auto".into());
    }
    resolved.extend_from_slice(&body);
    resolved
}

#[cfg(test)]
fn gemini_acp_args(model: Option<&str>) -> Vec<String> {
    gemini_acp_args_with_permission(model, AgentPermissionLevel::Ask)
}

fn gemini_acp_args_with_permission(
    model: Option<&str>,
    permission_level: AgentPermissionLevel,
) -> Vec<String> {
    let model = normalized_agent_model(model);
    let mut args = Vec::with_capacity(if model.is_some() { 5 } else { 3 });
    if let Some(model) = model {
        args.push("--model".into());
        args.push(model);
    }
    args.push("--approval-mode".into());
    args.push(gemini_approval_mode_for_level(permission_level).into());
    args.push("--acp".into());
    args
}

fn detect_codex_model_override() -> Option<String> {
    if let Some(model) = env::var("HEMATITE_CODEX_MODEL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Some(model);
    }

    codex_model_override_from_parts(
        codex_configured_model().as_deref(),
        codex_cli_version().as_deref(),
    )
}

fn codex_model_override_from_parts(
    configured_model: Option<&str>,
    cli_version: Option<&str>,
) -> Option<String> {
    let configured_model = configured_model?.trim();
    if configured_model != "gpt-5.5" {
        return None;
    }

    if cli_version.is_some_and(|version| codex_cli_version_before(version, 0, 119, 0)) {
        return Some(CODEX_SAFE_MODEL_FOR_OLD_GPT55_CONFIG.into());
    }

    None
}

fn codex_configured_model() -> Option<String> {
    let path = user_home_dir()?.join(".codex").join("config.toml");
    let raw = fs::read_to_string(path).ok()?;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            break;
        }
        if let Some(model) = parse_simple_toml_string_value(trimmed, "model") {
            return Some(model);
        }
    }

    None
}

fn parse_simple_toml_string_value(line: &str, key: &str) -> Option<String> {
    let (left, right) = line.split_once('=')?;
    if left.trim() != key {
        return None;
    }

    let raw_value = right.split('#').next()?.trim();
    if raw_value.is_empty() {
        return None;
    }

    Some(
        raw_value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(raw_value)
            .trim()
            .to_string(),
    )
    .filter(|value| !value.is_empty())
}

fn codex_cli_version() -> Option<String> {
    let args = vec!["--version".to_string()];
    let output = command_output_with_timeout(
        prepare_cli_command("codex", &args).command,
        Duration::from_secs(1),
    )
    .ok()??;

    non_empty_output(&output.stdout, &output.stderr)
}

fn codex_cli_version_before(version: &str, major: u64, minor: u64, patch: u64) -> bool {
    let Some((actual_major, actual_minor, actual_patch)) = parse_codex_cli_version(version) else {
        return false;
    };

    (actual_major, actual_minor, actual_patch) < (major, minor, patch)
}

fn parse_codex_cli_version(version: &str) -> Option<(u64, u64, u64)> {
    let version = version.split_whitespace().find(|part| {
        part.chars()
            .next()
            .is_some_and(|value| value.is_ascii_digit())
    })?;
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch_text = parts.next().unwrap_or("0");
    let patch = patch_text
        .chars()
        .take_while(|value| value.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()?;

    Some((major, minor, patch))
}

fn ensure_codex_app_server_session<'a>(
    bridge: &'a mut CodexAppServerState,
    app: &tauri::AppHandle,
    root: &str,
) -> Result<&'a mut CodexAppServerSession, String> {
    let mut needs_restart = bridge.session.is_none();

    if let Some(session) = bridge.session.as_mut() {
        let exited = session
            .child
            .try_wait()
            .map_err(|err| format!("Could not inspect Codex background process. {}", err))?
            .is_some();
        let current_root = session
            .shared
            .lock()
            .map_err(|_| "Codex shared state lock was poisoned.".to_string())?
            .current_root
            .clone();

        needs_restart = exited || current_root != root;
    }

    if needs_restart {
        if let Some(session) = bridge.session.as_mut() {
            dispose_codex_session(session);
        }
        bridge.session = Some(spawn_codex_app_server_session(app, root)?);
    }

    bridge
        .session
        .as_mut()
        .ok_or_else(|| "Codex session did not start.".to_string())
}

fn spawn_codex_app_server_session(
    app: &tauri::AppHandle,
    root: &str,
) -> Result<CodexAppServerSession, String> {
    let stored = load_agent_credentials();
    let args = vec![
        "app-server".to_string(),
        "--listen".to_string(),
        "stdio://".to_string(),
    ];
    let mut prepared = prepare_cli_command("codex", &args);
    prepared.command.stdin(Stdio::piped());
    prepared.command.stdout(Stdio::piped());
    prepared.command.stderr(Stdio::piped());
    prepared.command.current_dir(root);
    apply_agent_env(&mut prepared.command, &stored);
    apply_workspace_env(&mut prepared.command, Path::new(root));
    hide_background_window(&mut prepared.command);

    let mut child = prepared.command.spawn().map_err(|err| {
        format!(
            "Failed to start `codex app-server`. Make sure Codex CLI is installed and available on PATH. {}",
            err
        )
    })?;

    let stdin =
        Arc::new(Mutex::new(child.stdin.take().ok_or_else(|| {
            "Codex app-server did not expose stdin.".to_string()
        })?));
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Codex app-server did not expose stdout.".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Codex app-server did not expose stderr.".to_string())?;
    let shared = Arc::new(Mutex::new(CodexSharedState::new(root.to_string())));

    spawn_codex_stdout_reader(app.clone(), shared.clone(), stdin.clone(), stdout);
    spawn_codex_stderr_reader(shared.clone(), stderr);

    Ok(CodexAppServerSession {
        child,
        stdin,
        shared,
    })
}

fn dispose_codex_session(session: &mut CodexAppServerSession) {
    let _ = session.child.kill();
    let _ = session.child.wait();
}

fn spawn_codex_stdout_reader(
    app: tauri::AppHandle,
    shared: Arc<Mutex<CodexSharedState>>,
    stdin: Arc<Mutex<ChildStdin>>,
    stdout: impl std::io::Read + Send + 'static,
) {
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut ended_unexpectedly = true;
        for line in reader.lines() {
            let Ok(line) = line else {
                ended_unexpectedly = true;
                break;
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
                emit_codex_frontend_event(
                    &app,
                    CodexFrontendEvent::Error {
                        message: format!(
                            "Codex sent an unreadable background event: {}",
                            trimmed.chars().take(220).collect::<String>()
                        ),
                    },
                );
                continue;
            };

            handle_codex_message(&app, &shared, &stdin, message);
        }

        if ended_unexpectedly {
            let message = codex_error_with_recent_stderr(
                &shared,
                "The Codex background session closed.".into(),
            );
            if fail_codex_pending_responses(&shared, message.clone()) > 0 {
                emit_codex_frontend_event(&app, CodexFrontendEvent::Error { message });
            }
        }
    });
}

fn spawn_codex_stderr_reader(
    shared: Arc<Mutex<CodexSharedState>>,
    stderr: impl std::io::Read + Send + 'static,
) {
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(mut state) = shared.lock() {
                state.last_stderr = Some(truncate_chars(trimmed, 800));
            }
        }
    });
}

fn codex_recent_stderr(shared: &Arc<Mutex<CodexSharedState>>) -> Option<String> {
    shared
        .lock()
        .ok()
        .and_then(|state| state.last_stderr.clone())
        .filter(|value| !value.trim().is_empty())
}

fn codex_error_with_recent_stderr(
    shared: &Arc<Mutex<CodexSharedState>>,
    message: String,
) -> String {
    let Some(stderr) = codex_recent_stderr(shared) else {
        return message;
    };

    format!("{message}\n\nRecent Codex stderr: {stderr}")
}

fn fail_codex_pending_responses(shared: &Arc<Mutex<CodexSharedState>>, message: String) -> usize {
    let senders = if let Ok(mut state) = shared.lock() {
        let pending = std::mem::take(&mut state.pending_responses);
        state.pending_server_requests.clear();
        pending.into_values().collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let count = senders.len();
    for sender in senders {
        let _ = sender.send(Err(message.clone()));
    }
    count
}

fn handle_codex_message(
    app: &tauri::AppHandle,
    shared: &Arc<Mutex<CodexSharedState>>,
    stdin: &Arc<Mutex<ChildStdin>>,
    message: Value,
) {
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_string);
    let id = message.get("id").cloned();

    match (method, id) {
        (Some(method), Some(id)) => {
            handle_codex_server_request(app, shared, stdin, id, &method, message)
        }
        (Some(method), None) => handle_codex_notification(app, shared, &method, &message),
        (None, Some(id)) => handle_codex_response(shared, id, &message),
        (None, None) => {}
    }
}

fn handle_codex_response(shared: &Arc<Mutex<CodexSharedState>>, id: Value, message: &Value) {
    let Some(id_key) = request_id_key(&id) else {
        return;
    };

    let sender = shared
        .lock()
        .ok()
        .and_then(|mut state| state.pending_responses.remove(&id_key));

    if let Some(sender) = sender {
        if let Some(result) = message.get("result") {
            let _ = sender.send(Ok(result.clone()));
        } else {
            let error = message
                .get("error")
                .map(json_error_message)
                .unwrap_or_else(|| "Codex returned an empty response.".into());
            let _ = sender.send(Err(codex_error_with_recent_stderr(shared, error)));
        }
    }
}

fn handle_codex_server_request(
    app: &tauri::AppHandle,
    shared: &Arc<Mutex<CodexSharedState>>,
    stdin: &Arc<Mutex<ChildStdin>>,
    id: Value,
    method: &str,
    message: Value,
) {
    let Some(request_id) = request_id_key(&id) else {
        return;
    };
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    if method == "item/tool/requestUserInput" {
        let _ = send_codex_json(
            stdin,
            &json!({
                "id": id,
                "result": {
                    "answers": {}
                }
            }),
        );
        emit_codex_frontend_event(
            app,
            CodexFrontendEvent::Error {
                message:
                    "Codex asked for extra structured user input, but Hematite does not support that prompt type yet."
                        .into(),
            },
        );
        return;
    }

    if let Ok(mut state) = shared.lock() {
        state.pending_server_requests.insert(
            request_id.clone(),
            CodexPendingServerRequest {
                id: id.clone(),
                method: method.to_string(),
                params: params.clone(),
            },
        );
    }

    match method {
        "item/commandExecution/requestApproval" => {
            emit_codex_frontend_event(
                app,
                CodexFrontendEvent::ApprovalRequested {
                    request_id,
                    approval_type: "command".into(),
                    turn_id: value_string(&params, &["turnId"]).unwrap_or_default(),
                    item_id: value_string(&params, &["itemId"]).unwrap_or_default(),
                    reason: value_string(&params, &["reason"]),
                    command: value_string(&params, &["command"]),
                    cwd: value_string(&params, &["cwd"]),
                    grant_root: None,
                    permissions: permission_summary_from_profile(
                        params.get("additionalPermissions"),
                    ),
                    choices: command_approval_choices(&params),
                },
            );
        }
        "item/fileChange/requestApproval" => {
            emit_codex_frontend_event(
                app,
                CodexFrontendEvent::ApprovalRequested {
                    request_id,
                    approval_type: "fileChange".into(),
                    turn_id: value_string(&params, &["turnId"]).unwrap_or_default(),
                    item_id: value_string(&params, &["itemId"]).unwrap_or_default(),
                    reason: value_string(&params, &["reason"]),
                    command: None,
                    cwd: None,
                    grant_root: value_string(&params, &["grantRoot"]),
                    permissions: None,
                    choices: file_change_approval_choices(),
                },
            );
        }
        "item/permissions/requestApproval" => {
            emit_codex_frontend_event(
                app,
                CodexFrontendEvent::ApprovalRequested {
                    request_id,
                    approval_type: "permissions".into(),
                    turn_id: value_string(&params, &["turnId"]).unwrap_or_default(),
                    item_id: value_string(&params, &["itemId"]).unwrap_or_default(),
                    reason: value_string(&params, &["reason"]),
                    command: None,
                    cwd: None,
                    grant_root: None,
                    permissions: permission_summary_from_profile(params.get("permissions")),
                    choices: permission_approval_choices(),
                },
            );
        }
        other => {
            if let Ok(mut state) = shared.lock() {
                state.pending_server_requests.remove(&request_id);
            }
            let _ = send_codex_json(
                stdin,
                &json!({
                    "id": id,
                    "error": {
                        "message": format!("Hematite does not support the Codex request `{}` yet.", other),
                    }
                }),
            );
            emit_codex_frontend_event(
                app,
                CodexFrontendEvent::Error {
                    message: format!(
                        "Codex requested `{}` which Hematite does not handle yet.",
                        other
                    ),
                },
            );
        }
    }
}

fn handle_codex_notification(
    app: &tauri::AppHandle,
    shared: &Arc<Mutex<CodexSharedState>>,
    method: &str,
    message: &Value,
) {
    let params = message.get("params").unwrap_or(&Value::Null);

    match method {
        "thread/started" => {
            if let Some(thread_id) = value_string(params, &["thread", "id"]) {
                if let Ok(mut state) = shared.lock() {
                    state.current_thread_id = Some(thread_id);
                }
            }
        }
        "turn/started" => {
            if let Some(turn_id) = value_string(params, &["turn", "id"]) {
                if let Ok(mut state) = shared.lock() {
                    state.active_turn_id = Some(turn_id);
                }
            }
        }
        "item/agentMessage/delta" => {
            emit_codex_frontend_event(
                app,
                CodexFrontendEvent::AgentMessageDelta {
                    turn_id: value_string(params, &["turnId"]).unwrap_or_default(),
                    item_id: value_string(params, &["itemId"]).unwrap_or_default(),
                    delta: value_string(params, &["delta"]).unwrap_or_default(),
                },
            );
        }
        "item/completed" => {
            if params
                .get("item")
                .and_then(|value| value.get("type"))
                .and_then(Value::as_str)
                == Some("agentMessage")
            {
                emit_codex_frontend_event(
                    app,
                    CodexFrontendEvent::AgentMessageCompleted {
                        turn_id: value_string(params, &["turnId"]).unwrap_or_default(),
                        item_id: value_string(params, &["item", "id"]).unwrap_or_default(),
                        text: value_string(params, &["item", "text"]).unwrap_or_default(),
                    },
                );
            }
        }
        "serverRequest/resolved" => {
            if let Some(request_id) = value_to_string(params.get("requestId")) {
                if let Ok(mut state) = shared.lock() {
                    state.pending_server_requests.remove(&request_id);
                }
                emit_codex_frontend_event(app, CodexFrontendEvent::ApprovalResolved { request_id });
            }
        }
        "turn/completed" => {
            if let Ok(mut state) = shared.lock() {
                state.active_turn_id = None;
            }

            let turn_id = value_string(params, &["turn", "id"]).unwrap_or_default();
            let status = value_string(params, &["turn", "status"]).unwrap_or_default();
            let error = value_string(params, &["turn", "error", "message"])
                .or_else(|| value_string(params, &["turn", "error", "additionalDetails"]));

            emit_codex_frontend_event(
                app,
                CodexFrontendEvent::TurnCompleted {
                    turn_id,
                    success: status == "completed",
                    error,
                },
            );
        }
        "error" => {
            let message =
                codex_error_with_recent_stderr(shared, codex_error_notification_message(params));
            emit_codex_frontend_event(app, CodexFrontendEvent::Error { message });
        }
        _ => {}
    }
}

fn codex_error_notification_message(params: &Value) -> String {
    let mut parts = Vec::new();

    if let Some(message) =
        value_string(params, &["message"]).or_else(|| value_string(params, &["error", "message"]))
    {
        parts.push(message);
    }

    if let Some(details) = value_string(params, &["additionalDetails"])
        .or_else(|| value_string(params, &["error", "additionalDetails"]))
        .filter(|details| !parts.iter().any(|part| part == details))
    {
        parts.push(format!("Details: {details}"));
    }

    if let Some(info) = params
        .get("error")
        .and_then(|error| error.get("codexErrorInfo"))
        .filter(|info| !info.is_null())
    {
        parts.push(format!("Codex error info: {}", compact_json(info, 500)));
    }

    if params
        .get("willRetry")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        parts.push("Codex will retry this turn automatically.".into());
    }

    if parts.is_empty() {
        format!(
            "Codex reported an unknown background error: {}",
            compact_json(params, 700)
        )
    } else {
        parts.join("\n")
    }
}

fn emit_codex_frontend_event(app: &tauri::AppHandle, event: CodexFrontendEvent) {
    let _ = app.emit("hematite://codex", event);
}

fn ensure_codex_initialized(session: &CodexAppServerSession) -> Result<(), String> {
    let needs_initialize = session
        .shared
        .lock()
        .map_err(|_| "Codex shared state lock was poisoned.".to_string())?
        .initialized
        == false;

    if !needs_initialize {
        return Ok(());
    }

    codex_send_request(
        session,
        "initialize",
        json!({
            "clientInfo": {
                "name": "hematite",
                "title": "Hematite",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": {
                "experimentalApi": true,
            },
        }),
        Duration::from_secs(15),
    )?;

    if let Ok(mut shared) = session.shared.lock() {
        shared.initialized = true;
    }

    Ok(())
}

fn ensure_codex_thread(
    session: &CodexAppServerSession,
    root: &str,
    selected_model: Option<&str>,
    permission_level: AgentPermissionLevel,
) -> Result<String, String> {
    {
        let shared = session
            .shared
            .lock()
            .map_err(|_| "Codex shared state lock was poisoned.".to_string())?;
        if let Some(thread_id) = &shared.current_thread_id {
            if shared.current_thread_model.as_deref() == selected_model
                && shared.current_thread_permission_level == Some(permission_level)
            {
                return Ok(thread_id.clone());
            }
        }
    }

    let settings = codex_execution_settings_for_permission_level(permission_level);
    let mut params = json!({
        "cwd": root,
        "approvalPolicy": settings.approval_policy,
        "approvalsReviewer": "user",
        "sandbox": settings.sandbox,
        "ephemeral": false,
        "experimentalRawEvents": false,
        "persistExtendedHistory": true,
        "serviceName": "Hematite",
    });
    apply_codex_model_param(&mut params, selected_model, None);

    let response = codex_send_request(session, "thread/start", params, Duration::from_secs(15))?;

    let thread_id = response
        .get("thread")
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| "Codex app-server did not return a thread id.".to_string())?
        .to_string();

    if let Ok(mut shared) = session.shared.lock() {
        shared.current_thread_id = Some(thread_id.clone());
        shared.current_thread_model = selected_model.map(str::to_string);
        shared.current_thread_permission_level = Some(permission_level);
    }

    Ok(thread_id)
}

fn codex_send_request(
    session: &CodexAppServerSession,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value, String> {
    let (tx, rx) = mpsc::channel();
    let (request_id, message) = {
        let mut shared = session
            .shared
            .lock()
            .map_err(|_| "Codex shared state lock was poisoned.".to_string())?;
        let request_id = shared.next_request_id;
        shared.next_request_id += 1;
        shared.pending_responses.insert(request_id.to_string(), tx);
        (
            request_id,
            json!({
                "id": request_id,
                "method": method,
                "params": params,
            }),
        )
    };

    if let Err(error) = send_codex_json(&session.stdin, &message) {
        if let Ok(mut shared) = session.shared.lock() {
            shared.pending_responses.remove(&request_id.to_string());
        }
        return Err(error);
    }

    match rx.recv_timeout(timeout) {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) => Err(error),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            if let Ok(mut shared) = session.shared.lock() {
                shared.pending_responses.remove(&request_id.to_string());
            }
            Err(format!(
                "Codex did not answer `{}` within {} seconds.",
                method,
                timeout.as_secs()
            ))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(format!(
            "The Codex background session closed while waiting for `{}`.",
            method
        )),
    }
}

fn codex_respond_to_server_request(
    session: &CodexAppServerSession,
    request: CodexApprovalResponseRequest,
) -> Result<(), String> {
    let pending = session
        .shared
        .lock()
        .map_err(|_| "Codex shared state lock was poisoned.".to_string())?
        .pending_server_requests
        .get(&request.request_id)
        .map(|entry| CodexPendingServerRequest {
            id: entry.id.clone(),
            method: entry.method.clone(),
            params: entry.params.clone(),
        })
        .ok_or_else(|| "That approval request is no longer pending.".to_string())?;

    let result = match pending.method.as_str() {
        "item/commandExecution/requestApproval" => {
            json!({
                "decision": resolve_command_approval_decision(&request.decision, &pending.params),
            })
        }
        "item/fileChange/requestApproval" => {
            json!({
                "decision": resolve_file_change_approval_decision(&request.decision),
            })
        }
        "item/permissions/requestApproval" => {
            resolve_permission_approval_result(&request.decision, pending.params.get("permissions"))
        }
        other => {
            return Err(format!(
                "Hematite cannot answer the Codex request type `{}` yet.",
                other
            ));
        }
    };

    send_codex_json(
        &session.stdin,
        &json!({
            "id": pending.id,
            "result": result,
        }),
    )
}

fn send_codex_json(stdin: &Arc<Mutex<ChildStdin>>, message: &Value) -> Result<(), String> {
    let serialized = serde_json::to_string(message).map_err(|err| err.to_string())?;
    let mut handle = stdin
        .lock()
        .map_err(|_| "Codex stdin lock was poisoned.".to_string())?;
    handle
        .write_all(serialized.as_bytes())
        .map_err(|err| format!("Could not send a message to Codex. {}", err))?;
    handle
        .write_all(b"\n")
        .map_err(|err| format!("Could not terminate a Codex message. {}", err))?;
    handle.flush().map_err(|err| err.to_string())
}

fn request_id_key(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn value_to_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn value_string(root: &Value, path: &[&str]) -> Option<String> {
    let mut current = root;
    for key in path {
        current = current.get(*key)?;
    }
    value_to_string(Some(current))
}

fn json_error_message(value: &Value) -> String {
    value
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn compact_json(value: &Value, max_chars: usize) -> String {
    serde_json::to_string(value)
        .map(|serialized| truncate_chars(&serialized, max_chars))
        .unwrap_or_else(|_| "<unserializable json>".into())
}

fn permission_summary_from_profile(value: Option<&Value>) -> Option<CodexPermissionSummary> {
    let Some(profile) = value else {
        return None;
    };

    let network_enabled = profile
        .get("network")
        .and_then(|network| network.get("enabled"))
        .and_then(Value::as_bool);
    let read_roots = profile
        .get("fileSystem")
        .and_then(|fs| fs.get("read"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let write_roots = profile
        .get("fileSystem")
        .and_then(|fs| fs.get("write"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(CodexPermissionSummary {
        network_enabled,
        read_roots,
        write_roots,
    })
}

fn command_approval_choices(params: &Value) -> Vec<FrontendApprovalChoice> {
    let mut choices = Vec::new();
    let decisions = params
        .get("availableDecisions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if decisions.iter().any(command_decision_matches_allow_once) {
        choices.push(approval_choice("allowOnce", "Allow once"));
    }
    if decisions.iter().any(command_decision_matches_allow_session) {
        choices.push(approval_choice("allowForSession", "Allow for chat"));
    }
    if decisions.iter().any(command_decision_matches_decline) {
        choices.push(approval_choice("deny", "Deny"));
    }
    if decisions.iter().any(command_decision_matches_cancel) {
        choices.push(approval_choice("cancel", "Cancel"));
    }

    if choices.is_empty() {
        choices.push(approval_choice("allowOnce", "Allow once"));
        choices.push(approval_choice("deny", "Deny"));
    }

    choices
}

fn approval_choice(id: &str, label: &str) -> FrontendApprovalChoice {
    FrontendApprovalChoice {
        id: id.into(),
        label: label.into(),
    }
}

fn file_change_approval_choices() -> Vec<FrontendApprovalChoice> {
    vec![
        approval_choice("allowOnce", "Allow once"),
        approval_choice("allowForSession", "Allow for chat"),
        approval_choice("deny", "Deny"),
        approval_choice("cancel", "Cancel"),
    ]
}

fn permission_approval_choices() -> Vec<FrontendApprovalChoice> {
    vec![
        approval_choice("allowOnce", "Allow once"),
        approval_choice("allowForSession", "Allow for chat"),
        approval_choice("deny", "Deny"),
    ]
}

fn command_decision_matches_allow_once(value: &Value) -> bool {
    value.as_str() == Some("accept")
        || value.get("acceptWithExecpolicyAmendment").is_some()
        || value.get("applyNetworkPolicyAmendment").is_some()
}

fn command_decision_matches_allow_session(value: &Value) -> bool {
    value.as_str() == Some("acceptForSession")
}

fn command_decision_matches_decline(value: &Value) -> bool {
    value.as_str() == Some("decline")
}

fn command_decision_matches_cancel(value: &Value) -> bool {
    value.as_str() == Some("cancel")
}

fn resolve_command_approval_decision(choice: &str, params: &Value) -> Value {
    let decisions = params
        .get("availableDecisions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    match choice {
        "allowForSession" => decisions
            .iter()
            .find(|value| command_decision_matches_allow_session(value))
            .cloned()
            .or_else(|| {
                decisions
                    .iter()
                    .find(|value| command_decision_matches_allow_once(value))
                    .cloned()
            })
            .unwrap_or_else(|| Value::String("acceptForSession".into())),
        "deny" => decisions
            .iter()
            .find(|value| command_decision_matches_decline(value))
            .cloned()
            .or_else(|| {
                decisions
                    .iter()
                    .find(|value| command_decision_matches_cancel(value))
                    .cloned()
            })
            .unwrap_or_else(|| Value::String("decline".into())),
        "cancel" => decisions
            .iter()
            .find(|value| command_decision_matches_cancel(value))
            .cloned()
            .unwrap_or_else(|| Value::String("cancel".into())),
        _ => decisions
            .iter()
            .find(|value| command_decision_matches_allow_once(value))
            .cloned()
            .or_else(|| {
                decisions
                    .iter()
                    .find(|value| command_decision_matches_allow_session(value))
                    .cloned()
            })
            .unwrap_or_else(|| Value::String("accept".into())),
    }
}

fn resolve_file_change_approval_decision(choice: &str) -> &'static str {
    match choice {
        "allowForSession" => "acceptForSession",
        "deny" => "decline",
        "cancel" => "cancel",
        _ => "accept",
    }
}

fn resolve_permission_approval_result(choice: &str, requested: Option<&Value>) -> Value {
    match choice {
        "allowForSession" => json!({
            "permissions": requested.cloned().unwrap_or_else(|| json!({})),
            "scope": "session",
        }),
        "deny" => json!({
            "permissions": {},
            "scope": "turn",
        }),
        _ => json!({
            "permissions": requested.cloned().unwrap_or_else(|| json!({})),
            "scope": "turn",
        }),
    }
}

fn gemini_acp_state() -> &'static Mutex<GeminiAcpState> {
    GEMINI_ACP.get_or_init(|| Mutex::new(GeminiAcpState::default()))
}

fn ensure_gemini_acp_session<'a>(
    bridge: &'a mut GeminiAcpState,
    app: &tauri::AppHandle,
    root: &str,
    selected_model: Option<&str>,
    permission_level: AgentPermissionLevel,
) -> Result<&'a mut GeminiAcpSession, String> {
    let mut needs_restart = bridge.session.is_none();
    let selected_model = normalized_agent_model(selected_model);

    if let Some(session) = bridge.session.as_mut() {
        let exited = session
            .child
            .try_wait()
            .map_err(|err| format!("Could not inspect Gemini background process. {}", err))?
            .is_some();
        let (current_root, current_model, current_permission_level) = {
            let shared = session
                .shared
                .lock()
                .map_err(|_| "Gemini shared state lock was poisoned.".to_string())?;
            (
                shared.current_root.clone(),
                shared.current_model.clone(),
                shared.current_permission_level,
            )
        };

        needs_restart = exited
            || current_root != root
            || current_model != selected_model
            || current_permission_level != permission_level;
    }

    if needs_restart {
        if let Some(session) = bridge.session.as_mut() {
            dispose_gemini_session(session);
        }
        bridge.session = Some(spawn_gemini_acp_session(
            app,
            root,
            selected_model.as_deref(),
            permission_level,
        )?);
    }

    bridge
        .session
        .as_mut()
        .ok_or_else(|| "Gemini ACP session did not start.".to_string())
}

fn spawn_gemini_acp_session(
    app: &tauri::AppHandle,
    root: &str,
    selected_model: Option<&str>,
    permission_level: AgentPermissionLevel,
) -> Result<GeminiAcpSession, String> {
    let stored = load_agent_credentials();
    let args = gemini_acp_args_with_permission(selected_model, permission_level);
    let mut prepared = prepare_cli_command("gemini", &args);
    prepared.command.stdin(Stdio::piped());
    prepared.command.stdout(Stdio::piped());
    prepared.command.stderr(Stdio::piped());
    prepared.command.current_dir(root);
    apply_agent_env(&mut prepared.command, &stored);
    apply_workspace_env(&mut prepared.command, Path::new(root));
    hide_background_window(&mut prepared.command);

    let mut child = prepared.command.spawn().map_err(|err| {
        format!(
            "Failed to start `gemini --acp`. Make sure Gemini CLI is installed and available on PATH. {}",
            err
        )
    })?;

    let stdin = Arc::new(Mutex::new(
        child
            .stdin
            .take()
            .ok_or_else(|| "Gemini ACP did not expose stdin.".to_string())?,
    ));
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Gemini ACP did not expose stdout.".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Gemini ACP did not expose stderr.".to_string())?;
    let shared = Arc::new(Mutex::new(GeminiSharedState::new(
        root.to_string(),
        normalized_agent_model(selected_model),
        permission_level,
    )));

    spawn_gemini_stdout_reader(app.clone(), shared.clone(), stdin.clone(), stdout);
    spawn_gemini_stderr_reader(stderr);

    Ok(GeminiAcpSession {
        child,
        stdin,
        shared,
    })
}

fn dispose_gemini_session(session: &mut GeminiAcpSession) {
    let _ = session.child.kill();
    let _ = session.child.wait();
}

fn spawn_gemini_stdout_reader(
    app: tauri::AppHandle,
    shared: Arc<Mutex<GeminiSharedState>>,
    stdin: Arc<Mutex<ChildStdin>>,
    stdout: impl std::io::Read + Send + 'static,
) {
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else {
                emit_gemini_frontend_event(
                    &app,
                    GeminiFrontendEvent::Error {
                        message: "Lost the Gemini event stream.".into(),
                    },
                );
                break;
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
                emit_gemini_frontend_event(
                    &app,
                    GeminiFrontendEvent::Error {
                        message: format!(
                            "Gemini sent an unreadable background event: {}",
                            trimmed.chars().take(220).collect::<String>()
                        ),
                    },
                );
                continue;
            };

            handle_gemini_message(&app, &shared, &stdin, message);
        }
    });
}

fn spawn_gemini_stderr_reader(stderr: impl std::io::Read + Send + 'static) {
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if line.is_err() {
                break;
            }
        }
    });
}

fn handle_gemini_message(
    app: &tauri::AppHandle,
    shared: &Arc<Mutex<GeminiSharedState>>,
    stdin: &Arc<Mutex<ChildStdin>>,
    message: Value,
) {
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_string);
    let id = message.get("id").cloned();

    match (method, id) {
        (Some(method), Some(id)) => handle_gemini_request(app, shared, stdin, id, &method, message),
        (Some(method), None) => handle_gemini_notification(app, shared, &method, &message),
        (None, Some(id)) => handle_gemini_response(shared, id, &message),
        (None, None) => {}
    }
}

fn handle_gemini_response(shared: &Arc<Mutex<GeminiSharedState>>, id: Value, message: &Value) {
    let Some(id_key) = request_id_key(&id) else {
        return;
    };

    let sender = shared
        .lock()
        .ok()
        .and_then(|mut state| state.pending_responses.remove(&id_key));

    if let Some(sender) = sender {
        if let Some(result) = message.get("result") {
            let _ = sender.send(Ok(result.clone()));
        } else {
            let error = message
                .get("error")
                .map(json_error_message)
                .unwrap_or_else(|| "Gemini returned an empty response.".into());
            let _ = sender.send(Err(error));
        }
    }
}

fn handle_gemini_request(
    app: &tauri::AppHandle,
    shared: &Arc<Mutex<GeminiSharedState>>,
    stdin: &Arc<Mutex<ChildStdin>>,
    id: Value,
    method: &str,
    message: Value,
) {
    let Some(request_id) = request_id_key(&id) else {
        return;
    };
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "session/request_permission" => {
            if let Ok(mut state) = shared.lock() {
                state.pending_requests.insert(
                    request_id.clone(),
                    GeminiPendingRequest {
                        id,
                        method: method.to_string(),
                        params: params.clone(),
                    },
                );
            }

            let session_id = value_string(&params, &["sessionId"]).unwrap_or_default();
            let title = value_string(&params, &["toolCall", "title"])
                .unwrap_or_else(|| "Gemini requested approval".into());
            let tool_kind = value_string(&params, &["toolCall", "kind"]);
            let command = gemini_terminal_command(&params);
            let locations = gemini_tool_locations(&params);
            let choices = params
                .get("options")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| {
                            let id = value_to_string(item.get("optionId"))?;
                            let label = value_string(item, &["name"]).unwrap_or_else(|| id.clone());
                            Some(FrontendApprovalChoice { id, label })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            emit_gemini_frontend_event(
                app,
                GeminiFrontendEvent::ApprovalRequested {
                    request_id,
                    session_id,
                    title,
                    tool_kind,
                    command,
                    locations,
                    choices,
                },
            );
        }
        other => {
            let _ = send_gemini_json(
                stdin,
                &json!({
                    "id": id,
                    "error": {
                        "message": format!("Hematite does not support the Gemini request `{}` yet.", other),
                    }
                }),
            );
            emit_gemini_frontend_event(
                app,
                GeminiFrontendEvent::Error {
                    message: format!(
                        "Gemini requested `{}` which Hematite does not handle yet.",
                        other
                    ),
                },
            );
        }
    }
}

fn handle_gemini_notification(
    app: &tauri::AppHandle,
    _shared: &Arc<Mutex<GeminiSharedState>>,
    method: &str,
    message: &Value,
) {
    if method != "session/update" {
        return;
    }

    let params = message.get("params").unwrap_or(&Value::Null);
    let session_id = value_string(params, &["sessionId"]).unwrap_or_default();
    let update = params.get("update").unwrap_or(&Value::Null);
    let update_kind = value_string(update, &["sessionUpdate"]).unwrap_or_default();

    match update_kind.as_str() {
        "agent_message_chunk" => {
            if let Some(text) = update
                .get("content")
                .and_then(|value| value.get("text"))
                .and_then(Value::as_str)
            {
                emit_gemini_frontend_event(
                    app,
                    GeminiFrontendEvent::AgentMessageDelta {
                        session_id,
                        delta: text.to_string(),
                    },
                );
            }
        }
        _ => {}
    }
}

fn emit_gemini_frontend_event(app: &tauri::AppHandle, event: GeminiFrontendEvent) {
    let _ = app.emit("hematite://gemini", event);
}

fn ensure_gemini_initialized(session: &GeminiAcpSession) -> Result<(), String> {
    let needs_initialize = !session
        .shared
        .lock()
        .map_err(|_| "Gemini shared state lock was poisoned.".to_string())?
        .initialized;

    if !needs_initialize {
        return Ok(());
    }

    gemini_send_request(
        session,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientInfo": {
                "name": "hematite",
                "title": "Hematite",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "clientCapabilities": {
                "auth": { "terminal": false },
                "fs": { "readTextFile": false, "writeTextFile": false },
                "terminal": false,
            }
        }),
        Duration::from_secs(20),
    )?;

    if let Ok(mut shared) = session.shared.lock() {
        shared.initialized = true;
    }

    Ok(())
}

fn ensure_gemini_chat_session(session: &GeminiAcpSession, root: &str) -> Result<String, String> {
    if let Some(session_id) = session
        .shared
        .lock()
        .map_err(|_| "Gemini shared state lock was poisoned.".to_string())?
        .current_session_id
        .clone()
    {
        return Ok(session_id);
    }

    let response = gemini_send_request(
        session,
        "session/new",
        json!({
            "cwd": root,
            "mcpServers": [],
        }),
        Duration::from_secs(30),
    )?;

    let session_id = response
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| "Gemini ACP did not return a session id.".to_string())?
        .to_string();

    if let Ok(mut shared) = session.shared.lock() {
        shared.current_session_id = Some(session_id.clone());
    }

    Ok(session_id)
}

fn gemini_send_request(
    session: &GeminiAcpSession,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value, String> {
    gemini_send_request_with_handles(&session.stdin, &session.shared, method, params, timeout)
}

fn gemini_send_request_with_handles(
    stdin: &Arc<Mutex<ChildStdin>>,
    shared: &Arc<Mutex<GeminiSharedState>>,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value, String> {
    let (tx, rx) = mpsc::channel();
    let (request_id, message) = {
        let mut shared = shared
            .lock()
            .map_err(|_| "Gemini shared state lock was poisoned.".to_string())?;
        let request_id = shared.next_request_id;
        shared.next_request_id += 1;
        shared.pending_responses.insert(request_id.to_string(), tx);
        (
            request_id,
            json!({
                "id": request_id,
                "method": method,
                "params": params,
            }),
        )
    };

    if let Err(error) = send_gemini_json(stdin, &message) {
        if let Ok(mut shared) = shared.lock() {
            shared.pending_responses.remove(&request_id.to_string());
        }
        return Err(error);
    }

    match rx.recv_timeout(timeout) {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) => Err(error),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            if let Ok(mut shared) = shared.lock() {
                shared.pending_responses.remove(&request_id.to_string());
            }
            Err(format!(
                "Gemini did not answer `{}` within {} seconds.",
                method,
                timeout.as_secs()
            ))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(format!(
            "The Gemini background session closed while waiting for `{}`.",
            method
        )),
    }
}

fn gemini_respond_to_permission_request(
    session: &GeminiAcpSession,
    request: GeminiApprovalResponseRequest,
) -> Result<(), String> {
    let pending = session
        .shared
        .lock()
        .map_err(|_| "Gemini shared state lock was poisoned.".to_string())?
        .pending_requests
        .get(&request.request_id)
        .map(|entry| GeminiPendingRequest {
            id: entry.id.clone(),
            method: entry.method.clone(),
            params: entry.params.clone(),
        })
        .ok_or_else(|| "That Gemini approval request is no longer pending.".to_string())?;

    if pending.method != "session/request_permission" {
        return Err(format!(
            "Hematite cannot answer the Gemini request type `{}` yet.",
            pending.method
        ));
    }

    send_gemini_json(
        &session.stdin,
        &json!({
            "id": pending.id,
            "result": {
                "outcome": {
                    "outcome": "selected",
                    "optionId": request.option_id,
                }
            }
        }),
    )
}

fn send_gemini_json(stdin: &Arc<Mutex<ChildStdin>>, message: &Value) -> Result<(), String> {
    let serialized = serde_json::to_string(message).map_err(|err| err.to_string())?;
    let mut handle = stdin
        .lock()
        .map_err(|_| "Gemini stdin lock was poisoned.".to_string())?;
    handle
        .write_all(serialized.as_bytes())
        .map_err(|err| format!("Could not send a message to Gemini. {}", err))?;
    handle
        .write_all(b"\n")
        .map_err(|err| format!("Could not terminate a Gemini message. {}", err))?;
    handle.flush().map_err(|err| err.to_string())
}

fn gemini_terminal_command(params: &Value) -> Option<String> {
    let contents = params
        .get("toolCall")
        .and_then(|value| value.get("content"))
        .and_then(Value::as_array)?;

    for item in contents {
        if item.get("type").and_then(Value::as_str) == Some("terminal") {
            if let Some(command) = value_string(item, &["command"]) {
                return Some(command);
            }
        }
        if item.get("type").and_then(Value::as_str) == Some("content") {
            if let Some(text) = item
                .get("content")
                .and_then(|value| value.get("text"))
                .and_then(Value::as_str)
            {
                return Some(text.to_string());
            }
        }
    }

    None
}

fn gemini_tool_locations(params: &Value) -> Vec<GeminiToolLocation> {
    params
        .get("toolCall")
        .and_then(|value| value.get("locations"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let path = value_string(item, &["path"])?;
                    let line = item
                        .get("line")
                        .and_then(Value::as_u64)
                        .map(|value| value as u32);
                    Some(GeminiToolLocation { path, line })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn prepare_cli_command(binary: &str, args: &[String]) -> PreparedCommand {
    let resolved = probe_command(binary).unwrap_or_else(|| binary.to_string());

    #[cfg(target_os = "windows")]
    {
        let extension = Path::new(&resolved)
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase());

        if matches!(extension.as_deref(), Some("ps1")) {
            let mut command = Command::new("powershell.exe");
            command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
            command.arg(&resolved);
            command.args(args);
            hide_background_window(&mut command);

            let mut preview = vec![
                "powershell.exe".into(),
                "-NoProfile".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                resolved,
            ];
            preview.extend(args.iter().cloned());

            return PreparedCommand { command, preview };
        }

        if matches!(extension.as_deref(), Some("cmd") | Some("bat")) {
            let mut command = Command::new("cmd.exe");
            command.arg("/C").arg(&resolved);
            command.args(args);
            hide_background_window(&mut command);

            let mut preview = vec!["cmd.exe".into(), "/C".into(), resolved];
            preview.extend(args.iter().cloned());

            return PreparedCommand { command, preview };
        }
    }

    let mut command = Command::new(&resolved);
    command.args(args);
    hide_background_window(&mut command);

    let mut preview = vec![resolved];
    preview.extend(args.iter().cloned());

    PreparedCommand { command, preview }
}

#[cfg(target_os = "windows")]
fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(target_os = "windows")]
fn hide_background_window(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn hide_background_window(_command: &mut Command) {}

#[cfg(not(target_os = "windows"))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn apply_agent_env(command: &mut Command, stored: &AgentCredentials) {
    if let Some(value) = effective_value(&stored.openai_api_key, "OPENAI_API_KEY") {
        command.env("OPENAI_API_KEY", value);
    }
    if let Some(value) = effective_value(&stored.gemini_api_key, "GEMINI_API_KEY") {
        command.env("GEMINI_API_KEY", value);
    }
    if let Some(value) = effective_value(&stored.google_api_key, "GOOGLE_API_KEY") {
        command.env("GOOGLE_API_KEY", value);
    }
    if let Some(value) = effective_value(&stored.google_cloud_project, "GOOGLE_CLOUD_PROJECT") {
        command.env("GOOGLE_CLOUD_PROJECT", value);
    }
    if let Some(value) = effective_value(&stored.google_cloud_location, "GOOGLE_CLOUD_LOCATION") {
        command.env("GOOGLE_CLOUD_LOCATION", value);
    }
    if let Some(value) = effective_value(
        &stored.google_application_credentials,
        "GOOGLE_APPLICATION_CREDENTIALS",
    ) {
        command.env("GOOGLE_APPLICATION_CREDENTIALS", value);
    }
    if let Some(value) = effective_value(&stored.anthropic_api_key, "ANTHROPIC_API_KEY") {
        command.env("ANTHROPIC_API_KEY", value);
    }
    if let Some(value) = effective_value(&stored.kilo_api_key, "KILO_API_KEY") {
        command.env("KILO_API_KEY", value);
    }
}

fn apply_workspace_env(command: &mut Command, cwd: &Path) {
    let Some(project_root) = find_python_workspace_root(cwd) else {
        return;
    };

    let venv_dir = project_root.join(".venv");
    let bin_dir = venv_bin_dir(&project_root);
    if !bin_dir.exists() {
        return;
    }

    command.env("VIRTUAL_ENV", &venv_dir);
    command.env("UV_PROJECT_ENVIRONMENT", &venv_dir);

    let path_separator = if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    };
    let existing_path = env::var_os("PATH").unwrap_or_default();
    let combined = if existing_path.is_empty() {
        venv_bin_dir(&project_root).into_os_string()
    } else {
        let mut value = bin_dir.into_os_string();
        value.push(path_separator);
        value.push(existing_path);
        value
    };

    command.env("PATH", combined);
}

fn run_terminal_command_in_pty(
    command_text: &str,
    resolved_cwd: &Path,
    stored: &AgentCredentials,
) -> Result<(bool, String), String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: TERMINAL_PTY_ROWS,
            cols: TERMINAL_PTY_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| format!("Could not open terminal PTY. {err}"))?;

    let script = terminal_pty_shell_script(command_text, resolved_cwd);
    let mut command = terminal_pty_command(&script, resolved_cwd);
    apply_agent_env_to_pty(&mut command, stored);
    apply_workspace_env_to_pty(&mut command, resolved_cwd);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|err| format!("Could not read from terminal PTY. {err}"))?;
    let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(len) => {
                    if output_tx.send(buffer[..len].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|err| format!("Could not start terminal shell. {err}"))?;
    drop(pair.slave);

    let started_at = Instant::now();
    let timeout = Duration::from_secs(60);
    let mut output = Vec::new();
    let status = loop {
        while let Ok(chunk) = output_rx.try_recv() {
            output.extend(chunk);
        }

        if let Some(status) = child
            .try_wait()
            .map_err(|err| format!("Terminal shell poll failed. {err}"))?
        {
            break status;
        }

        if started_at.elapsed() >= timeout {
            let _ = child.kill();
            let status = child
                .wait()
                .map_err(|err| format!("Terminal shell kill wait failed. {err}"))?;
            output.extend_from_slice(b"\n[hematite] command timed out after 60 seconds\n");
            break status;
        }

        thread::sleep(Duration::from_millis(20));
    };

    let drain_until = Instant::now() + Duration::from_millis(250);
    while Instant::now() < drain_until {
        match output_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(chunk) => output.extend(chunk),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    Ok((
        status.success(),
        String::from_utf8_lossy(&output).to_string(),
    ))
}

fn write_to_terminal_session(session_id: &str, input: &str) -> Result<(), String> {
    let writer = {
        let sessions = terminal_sessions()
            .sessions
            .lock()
            .map_err(|_| "Terminal session registry lock was poisoned.".to_string())?;
        sessions
            .get(session_id)
            .map(|session| Arc::clone(&session.writer))
            .ok_or_else(|| "Terminal session is no longer active.".to_string())?
    };

    let mut writer = writer
        .lock()
        .map_err(|_| "Terminal session writer lock was poisoned.".to_string())?;
    writer
        .write_all(input.as_bytes())
        .and_then(|_| writer.flush())
        .map_err(|err| format!("Could not write to terminal session. {err}"))
}

#[cfg(target_os = "windows")]
fn terminal_session_shell_command(cwd: &Path) -> (CommandBuilder, &'static str) {
    let mut command = CommandBuilder::new("powershell.exe");
    command.args(["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass"]);
    command.cwd(cwd);
    command.env("TERM", "xterm-256color");
    (command, "PowerShell")
}

#[cfg(not(target_os = "windows"))]
fn terminal_session_shell_command(cwd: &Path) -> (CommandBuilder, &'static str) {
    let shell = env::var("SHELL").unwrap_or_else(|_| "sh".into());
    let mut command = CommandBuilder::new(shell);
    command.cwd(cwd);
    (command, "shell")
}

#[cfg(target_os = "windows")]
fn terminal_session_command_input(command_text: &str) -> String {
    let command = normalize_powershell_terminal_command(command_text);
    format!(
        "$global:LASTEXITCODE = $null; {command}; \
         $__hematiteOk = $?; \
         $__hematiteLastExit = $LASTEXITCODE; \
         $__hematiteExit = if (($__hematiteLastExit -as [int]) -ne $null) {{ [int]$__hematiteLastExit }} elseif ($__hematiteOk) {{ 0 }} else {{ 1 }}; \
         [Console]::Out.WriteLine('{status}' + $__hematiteExit); \
         [Console]::Out.WriteLine('{cwd}' + (Get-Location).Path)\r\n",
        command = command,
        status = TERMINAL_STATUS_MARKER,
        cwd = TERMINAL_CWD_MARKER,
    )
}

#[cfg(target_os = "windows")]
fn normalize_powershell_terminal_command(command_text: &str) -> String {
    let trimmed = command_text.trim();
    let unquoted = trimmed.trim_matches('"').trim_matches('\'');
    let normalized = unquoted.replace('/', "\\").to_ascii_lowercase();
    let normalized = normalized.strip_prefix(".\\").unwrap_or(&normalized);

    if matches!(
        normalized,
        ".venv\\scripts\\activate"
            | ".venv\\scripts\\activate.ps1"
            | "venv\\scripts\\activate"
            | "venv\\scripts\\activate.ps1"
    ) {
        return format!(". {}", powershell_quote(".\\.venv\\Scripts\\Activate.ps1"));
    }

    trimmed.to_string()
}

#[cfg(not(target_os = "windows"))]
fn terminal_session_command_input(command_text: &str) -> String {
    format!(
        "{command}\n__hematite_exit=$?; printf '%s%s\n' '{status}' \"$__hematite_exit\"; printf '%s%s\n' '{cwd}' \"$PWD\"\n",
        command = command_text,
        status = TERMINAL_STATUS_MARKER,
        cwd = TERMINAL_CWD_MARKER,
    )
}

#[cfg(target_os = "windows")]
fn terminal_pty_command(script: &str, cwd: &Path) -> CommandBuilder {
    let mut command = CommandBuilder::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    command.cwd(cwd);
    command
}

#[cfg(not(target_os = "windows"))]
fn terminal_pty_command(script: &str, cwd: &Path) -> CommandBuilder {
    let mut command = CommandBuilder::new("sh");
    command.args(["-lc", script]);
    command.cwd(cwd);
    command
}

#[cfg(target_os = "windows")]
fn terminal_pty_shell_script(command_text: &str, cwd: &Path) -> String {
    format!(
        "$ErrorActionPreference = 'Continue'; \
         try {{ Set-Location -LiteralPath {cwd}; }} catch {{ Write-Error $_; Write-Output ('{marker}' + {cwd}); exit 1 }}; \
         $global:LASTEXITCODE = $null; \
         try {{ Invoke-Expression {input}; }} catch {{ Write-Error $_; }}; \
         $exitCode = if (($LASTEXITCODE -as [int]) -ne $null) {{ [int]$LASTEXITCODE }} elseif ($?) {{ 0 }} else {{ 1 }}; \
         Write-Output ('{marker}' + (Get-Location).Path); \
         exit $exitCode",
        cwd = powershell_quote(&path_to_string(cwd)),
        input = powershell_quote(command_text),
        marker = TERMINAL_CWD_MARKER,
    )
}

#[cfg(not(target_os = "windows"))]
fn terminal_pty_shell_script(command_text: &str, cwd: &Path) -> String {
    format!(
        "cd {cwd} && {{ {input}; }}; status=$?; printf '%s%s\n' '{marker}' \"$PWD\"; exit $status",
        cwd = shell_quote(&path_to_string(cwd)),
        input = command_text,
        marker = TERMINAL_CWD_MARKER,
    )
}

fn apply_agent_env_to_pty(command: &mut CommandBuilder, stored: &AgentCredentials) {
    if let Some(value) = effective_value(&stored.openai_api_key, "OPENAI_API_KEY") {
        command.env("OPENAI_API_KEY", value);
    }
    if let Some(value) = effective_value(&stored.gemini_api_key, "GEMINI_API_KEY") {
        command.env("GEMINI_API_KEY", value);
    }
    if let Some(value) = effective_value(&stored.google_api_key, "GOOGLE_API_KEY") {
        command.env("GOOGLE_API_KEY", value);
    }
    if let Some(value) = effective_value(&stored.google_cloud_project, "GOOGLE_CLOUD_PROJECT") {
        command.env("GOOGLE_CLOUD_PROJECT", value);
    }
    if let Some(value) = effective_value(&stored.google_cloud_location, "GOOGLE_CLOUD_LOCATION") {
        command.env("GOOGLE_CLOUD_LOCATION", value);
    }
    if let Some(value) = effective_value(
        &stored.google_application_credentials,
        "GOOGLE_APPLICATION_CREDENTIALS",
    ) {
        command.env("GOOGLE_APPLICATION_CREDENTIALS", value);
    }
    if let Some(value) = effective_value(&stored.anthropic_api_key, "ANTHROPIC_API_KEY") {
        command.env("ANTHROPIC_API_KEY", value);
    }
    if let Some(value) = effective_value(&stored.kilo_api_key, "KILO_API_KEY") {
        command.env("KILO_API_KEY", value);
    }
}

fn apply_workspace_env_to_pty(command: &mut CommandBuilder, cwd: &Path) {
    let Some(project_root) = find_python_workspace_root(cwd) else {
        return;
    };

    let venv_dir = project_root.join(".venv");
    let bin_dir = venv_bin_dir(&project_root);
    if !bin_dir.exists() {
        return;
    }

    command.env("VIRTUAL_ENV", path_to_string(&venv_dir));
    command.env("UV_PROJECT_ENVIRONMENT", path_to_string(&venv_dir));

    let path_separator = if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    };
    let existing_path = env::var_os("PATH").unwrap_or_default();
    let combined = if existing_path.is_empty() {
        path_to_string(&bin_dir)
    } else {
        format!(
            "{}{}{}",
            path_to_string(&bin_dir),
            path_separator,
            existing_path.to_string_lossy()
        )
    };
    command.env("PATH", combined);
}

fn split_terminal_output(stdout: &str, fallback_cwd: &Path) -> (String, String) {
    let mut cwd = path_to_string(fallback_cwd);
    let mut lines = Vec::new();

    for line in stdout.lines() {
        if let Some(value) = line.trim().strip_prefix(TERMINAL_CWD_MARKER) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                cwd = trimmed.to_string();
            }
            continue;
        }

        lines.push(line);
    }

    (lines.join("\n").trim().to_string(), cwd)
}

fn strip_terminal_control_sequences(output: &str) -> String {
    let mut cleaned = String::with_capacity(output.len());
    let mut chars = output.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if next == '\u{7}' {
                            break;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }

        if ch == '\r' {
            continue;
        }

        cleaned.push(ch);
    }

    cleaned
}

fn detect_workspace_root() -> Result<String, String> {
    let current_dir = std::env::current_dir().map_err(|err| err.to_string())?;
    let root = if let Some(project_root) = current_dir.ancestors().find(|candidate| {
        candidate.join("package.json").exists()
            && candidate.join("src-tauri").join("tauri.conf.json").exists()
    }) {
        project_root.to_path_buf()
    } else if current_dir
        .file_name()
        .map(|name| name == "src-tauri")
        .unwrap_or(false)
    {
        current_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(current_dir)
    } else {
        current_dir
    };

    Ok(path_to_string(&root))
}

fn find_python_workspace_root(start: &Path) -> Option<PathBuf> {
    let origin = if start.is_file() {
        start.parent()?
    } else {
        start
    };

    origin
        .ancestors()
        .find(|candidate| {
            candidate.join(".venv").exists() || candidate.join("pyproject.toml").exists()
        })
        .map(Path::to_path_buf)
}

fn find_rust_workspace_root(start: &Path) -> Option<PathBuf> {
    let origin = if start.extension().is_some() {
        start.parent()?
    } else {
        start
    };

    origin
        .ancestors()
        .find(|candidate| candidate.join("Cargo.toml").exists())
        .map(Path::to_path_buf)
}

fn find_c_family_workspace_root(start: &Path) -> Option<PathBuf> {
    let origin = if start.extension().is_some() {
        start.parent()?
    } else {
        start
    };

    origin
        .ancestors()
        .find(|candidate| {
            candidate.join("compile_commands.json").exists()
                || candidate
                    .join("build")
                    .join("compile_commands.json")
                    .exists()
                || candidate.join("CMakeLists.txt").exists()
                || candidate.join("Makefile").exists()
                || candidate.join("meson.build").exists()
                || candidate.join(".clangd").exists()
        })
        .map(Path::to_path_buf)
}

fn rust_toolchain_file_for_root(root: &Path) -> Option<PathBuf> {
    for candidate in [
        root.join("rust-toolchain.toml"),
        root.join("rust-toolchain"),
    ] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn venv_bin_dir(root: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        root.join(".venv").join("Scripts")
    } else {
        root.join(".venv").join("bin")
    }
}

fn make_tool_status(id: &str, label: &str) -> ToolStatus {
    let resolved_path = probe_available_command(id);
    ToolStatus {
        id: id.into(),
        label: label.into(),
        available: resolved_path.is_some(),
        resolved_path,
    }
}

fn rust_tool_status_specs() -> &'static [ToolStatusSpec] {
    RUST_TOOL_STATUS_SPECS
}

fn c_family_tool_status_specs() -> &'static [ToolStatusSpec] {
    C_FAMILY_TOOL_STATUS_SPECS
}

fn probe_available_command(binary: &str) -> Option<String> {
    let path = probe_command(binary)?;
    if requires_version_probe(binary) && !command_version_probe_succeeds(&path) {
        return None;
    }

    Some(path)
}

fn requires_version_probe(binary: &str) -> bool {
    matches!(
        binary,
        "rust-analyzer" | "rustfmt" | "cargo-clippy" | "clippy-driver"
    )
}

fn command_version_probe_succeeds(path: &str) -> bool {
    let mut command = Command::new(path);
    command.arg("--version");
    hide_background_window(&mut command);

    command
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn probe_command(binary: &str) -> Option<String> {
    if let Some(path) = probe_bundled_command(binary) {
        return Some(path);
    }

    if cfg!(target_os = "windows") {
        if let Some(path) = probe_windows_command(binary) {
            return Some(path);
        }
    } else {
        let output = Command::new("which").arg(binary).output().ok()?;
        if !output.status.success() {
            return None;
        }

        let first_line = String::from_utf8_lossy(&output.stdout)
            .lines()
            .find(|line| !line.trim().is_empty())?
            .trim()
            .to_string();

        return Some(first_line);
    }

    None
}

fn probe_bundled_command(binary: &str) -> Option<String> {
    let names = bundled_binary_names(binary);
    let mut roots = Vec::new();

    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.to_path_buf());
            roots.push(parent.join("resources"));
            roots.push(parent.join("resources").join("binaries"));
        }
    }

    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries"));

    for root in roots {
        for name in &names {
            let candidate = root.join(name);
            if candidate.is_file() {
                return Some(path_to_string(&candidate));
            }
        }
    }

    None
}

fn bundled_binary_names(binary: &str) -> Vec<String> {
    let mut names = Vec::new();
    let exe_suffix = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };
    let target_suffix = if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-apple-darwin"
        } else {
            "x86_64-apple-darwin"
        }
    } else if cfg!(target_arch = "aarch64") {
        "aarch64-unknown-linux-gnu"
    } else {
        "x86_64-unknown-linux-gnu"
    };

    names.push(format!("{binary}-{target_suffix}{exe_suffix}"));
    names.push(format!("{binary}{exe_suffix}"));
    names
}

fn command_first_line(binary: &str, args: &[&str], cwd: &Path) -> Option<String> {
    command_lines(binary, args, cwd).into_iter().next()
}

fn command_lines(binary: &str, args: &[&str], cwd: &Path) -> Vec<String> {
    let binary_path = probe_command(binary).unwrap_or_else(|| binary.to_string());
    let mut command = Command::new(binary_path);
    command.args(args).current_dir(cwd);
    hide_background_window(&mut command);

    let Ok(output) = command.output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(target_os = "windows")]
fn probe_windows_command(binary: &str) -> Option<String> {
    let where_output = {
        let mut command = Command::new("where.exe");
        command.arg(binary);
        hide_background_window(&mut command);
        command.output().ok()
    };
    if let Some(output) = where_output {
        if output.status.success() {
            let mut candidates = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();

            candidates.sort_by_key(|candidate| windows_command_rank(candidate));
            if let Some(path) = candidates.into_iter().next() {
                return Some(path);
            }
        }
    }

    let script = format!(
        "$cmd = Get-Command -Name '{}' -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Path; if ($cmd) {{ $cmd }}",
        binary.replace('\'', "''")
    );

    let get_command_output = {
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-Command", &script]);
        hide_background_window(&mut command);
        command.output().ok()
    };

    if let Some(output) = get_command_output {
        if output.status.success() {
            if let Some(path) = String::from_utf8_lossy(&output.stdout)
                .lines()
                .find(|line| !line.trim().is_empty())
                .map(|line| line.trim().to_string())
            {
                return Some(path);
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn windows_command_rank(candidate: &str) -> usize {
    match Path::new(candidate)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("exe") => 0,
        Some("cmd") => 1,
        Some("bat") => 2,
        Some("ps1") => 3,
        Some(_) => 5,
        None => 4,
    }
}

fn should_ignore_name(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".idea" | ".next" | ".venv" | "__pycache__" | "dist" | "node_modules" | "target"
    )
}

fn path_to_string(path: &Path) -> String {
    let raw = path.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        if let Some(value) = raw.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{}", value);
        }
        if let Some(value) = raw.strip_prefix(r"\\?\") {
            return value.to_string();
        }
    }

    raw
}

fn language_id_from_path(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "py" => "python",
        "rs" => "rust",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => "cpp",
        "cu" | "cuh" => "cuda-cpp",
        "ts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "jsx",
        "json" => "json",
        "css" => "css",
        "html" | "htm" => "html",
        "md" => "markdown",
        "toml" => "toml",
        "yml" | "yaml" => "yaml",
        _ => "plaintext",
    }
}

fn parser_language_for_path(path: &Path) -> Option<SourceLanguage> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "py" => Some(SourceLanguage::Python),
        "rs" => Some(SourceLanguage::Rust),
        "c" | "h" => Some(SourceLanguage::C),
        "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => Some(SourceLanguage::Cpp),
        "cu" | "cuh" => Some(SourceLanguage::Cuda),
        "js" | "mjs" | "cjs" | "jsx" => Some(SourceLanguage::JavaScript),
        "ts" => Some(SourceLanguage::TypeScript),
        "tsx" => Some(SourceLanguage::Tsx),
        _ => None,
    }
}

fn parse_symbols_for_path(path: &Path, content: &str) -> Vec<SymbolEntry> {
    let Some(language) = parser_language_for_path(path) else {
        return Vec::new();
    };

    let Some(tree) = parse_tree(language, content) else {
        return Vec::new();
    };

    let mut symbols = Vec::new();
    collect_symbols_recursive(tree.root_node(), content.as_bytes(), language, &mut symbols);
    symbols
}

fn parse_tree(language: SourceLanguage, content: &str) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();

    let configured = match language {
        SourceLanguage::Python => parser.set_language(&tree_sitter_python::LANGUAGE.into()),
        SourceLanguage::Rust => parser.set_language(&tree_sitter_rust::LANGUAGE.into()),
        SourceLanguage::C => parser.set_language(&tree_sitter_c::LANGUAGE.into()),
        SourceLanguage::Cpp | SourceLanguage::Cuda => {
            parser.set_language(&tree_sitter_cpp::LANGUAGE.into())
        }
        SourceLanguage::JavaScript => parser.set_language(&tree_sitter_javascript::LANGUAGE.into()),
        SourceLanguage::TypeScript => {
            parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        }
        SourceLanguage::Tsx => parser.set_language(&tree_sitter_typescript::LANGUAGE_TSX.into()),
    };

    if configured.is_err() {
        return None;
    }

    parser.parse(content, None)
}

#[cfg(any())]
mod legacy_python_semantics_types {
    #[derive(Clone, Debug)]
    struct TextSpan {
        start_line: u32,
        start_column: u32,
        end_line: u32,
        end_column: u32,
    }

    #[derive(Clone)]
    struct HoverTemplate {
        kind: String,
        title: String,
        detail: Option<String>,
        source: Option<String>,
    }

    #[derive(Clone, Debug)]
    struct PythonImportAlias {
        alias: String,
        statement: String,
        span: TextSpan,
        token_kind: String,
        hover_kind: String,
    }
}

fn analyze_editor_semantics_for_path(path: &Path, content: &str) -> EditorSemanticsPayload {
    match parser_language_for_path(path) {
        Some(SourceLanguage::Python) => {
            if !should_run_automatic_python_analysis(content) {
                return EditorSemanticsPayload::default();
            }

            let root = find_python_workspace_root(path)
                .or_else(|| path.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| PathBuf::from("."));
            ty_semantic_tokens_for_document(&root, path, content).unwrap_or_default()
        }
        Some(SourceLanguage::Rust) => {
            if !should_run_automatic_rust_analysis(content) {
                return EditorSemanticsPayload::default();
            }

            let parser_payload = rust_tree_sitter_semantics_for_document(content);
            let Some(root) = find_rust_workspace_root(path) else {
                return parser_payload;
            };

            match rust_analyzer_semantic_tokens_for_document(&root, path, content) {
                Ok(payload) if !payload.tokens.is_empty() => payload,
                _ => parser_payload,
            }
        }
        Some(language @ (SourceLanguage::C | SourceLanguage::Cpp | SourceLanguage::Cuda)) => {
            if !should_run_automatic_c_family_analysis(content) {
                return EditorSemanticsPayload::default();
            }

            c_family_tree_sitter_semantics_for_document(language, content)
        }
        _ => EditorSemanticsPayload::default(),
    }
}

#[cfg(any())]
mod legacy_python_semantics {
    fn analyze_python_editor_semantics(content: &str) -> EditorSemanticsPayload {
        let Some(tree) = parse_tree(SourceLanguage::Python, content) else {
            return EditorSemanticsPayload::default();
        };

        let source = content.as_bytes();
        let (imports, import_entries) = collect_python_import_entries(content);
        let mut definitions = BTreeMap::<String, HoverTemplate>::new();
        let mut bindings = BTreeMap::<String, HoverTemplate>::new();
        let mut tokens = Vec::new();
        let mut token_seen = BTreeSet::new();
        let mut hover_items = Vec::new();
        let mut hover_seen = BTreeSet::new();

        for import in &import_entries {
            push_semantic_token(
                &mut tokens,
                &mut token_seen,
                &import.span,
                &import.token_kind,
            );
            push_hover_item(
                &mut hover_items,
                &mut hover_seen,
                &import.span,
                &HoverTemplate {
                    kind: import.hover_kind.clone(),
                    title: import.alias.clone(),
                    detail: None,
                    source: Some(import.statement.clone()),
                },
            );
        }

        collect_python_definition_semantics(
            tree.root_node(),
            source,
            content,
            &mut definitions,
            &mut bindings,
            &mut tokens,
            &mut token_seen,
            &mut hover_items,
            &mut hover_seen,
        );

        collect_python_reference_semantics(
            tree.root_node(),
            source,
            &imports,
            &definitions,
            &bindings,
            &mut tokens,
            &mut token_seen,
            &mut hover_items,
            &mut hover_seen,
        );

        EditorSemanticsPayload {
            tokens,
            hover_items,
        }
    }

    fn collect_python_definition_semantics(
        node: Node<'_>,
        source: &[u8],
        content: &str,
        definitions: &mut BTreeMap<String, HoverTemplate>,
        bindings: &mut BTreeMap<String, HoverTemplate>,
        tokens: &mut Vec<SemanticToken>,
        token_seen: &mut BTreeSet<String>,
        hover_items: &mut Vec<HoverItem>,
        hover_seen: &mut BTreeSet<String>,
    ) {
        match node.kind() {
            "function_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Some(name) = node_text(name_node, source) {
                        let span = span_from_node(name_node);
                        let is_method = is_python_method_definition(node);
                        let hover = HoverTemplate {
                            kind: if is_method { "Method" } else { "Function" }.into(),
                            title: signature_line_for_node(node, content),
                            detail: extract_python_docstring(node, source),
                            source: Some(format!(
                                "Defined in this file · line {}",
                                span.start_line
                            )),
                        };
                        definitions.entry(name).or_insert_with(|| hover.clone());
                        push_semantic_token(
                            tokens,
                            token_seen,
                            &span,
                            if is_method {
                                "methodDefinition"
                            } else {
                                "functionDefinition"
                            },
                        );
                        push_hover_item(hover_items, hover_seen, &span, &hover);
                    }
                }

                if let Some(parameters) = node.child_by_field_name("parameters") {
                    collect_python_parameter_semantics(
                        parameters,
                        source,
                        bindings,
                        tokens,
                        token_seen,
                        hover_items,
                        hover_seen,
                    );
                }
            }
            "class_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Some(name) = node_text(name_node, source) {
                        let span = span_from_node(name_node);
                        let hover = HoverTemplate {
                            kind: "Class".into(),
                            title: signature_line_for_node(node, content),
                            detail: extract_python_docstring(node, source),
                            source: Some(format!(
                                "Defined in this file · line {}",
                                span.start_line
                            )),
                        };
                        definitions.entry(name).or_insert_with(|| hover.clone());
                        push_semantic_token(tokens, token_seen, &span, "classDefinition");
                        push_hover_item(hover_items, hover_seen, &span, &hover);
                    }
                }
            }
            "assignment" | "annotated_assignment" => {
                if let Some(target) = node
                    .child_by_field_name("left")
                    .or_else(|| node.named_child(0))
                {
                    collect_python_binding_semantics(
                        target,
                        source,
                        bindings,
                        tokens,
                        token_seen,
                        hover_items,
                        hover_seen,
                    );
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named() {
                collect_python_definition_semantics(
                    child,
                    source,
                    content,
                    definitions,
                    bindings,
                    tokens,
                    token_seen,
                    hover_items,
                    hover_seen,
                );
            }
        }
    }

    fn collect_python_reference_semantics(
        node: Node<'_>,
        source: &[u8],
        imports: &BTreeMap<String, PythonImportAlias>,
        definitions: &BTreeMap<String, HoverTemplate>,
        bindings: &BTreeMap<String, HoverTemplate>,
        tokens: &mut Vec<SemanticToken>,
        token_seen: &mut BTreeSet<String>,
        hover_items: &mut Vec<HoverItem>,
        hover_seen: &mut BTreeSet<String>,
    ) {
        match node.kind() {
            "call" => {
                if let Some(function_node) = node.child_by_field_name("function") {
                    match function_node.kind() {
                        "identifier" => {
                            if let Some(name) = node_text(function_node, source) {
                                let span = span_from_node(function_node);
                                if let Some(definition) = definitions.get(&name) {
                                    let token_kind = if definition.kind == "Class" {
                                        "classReference"
                                    } else {
                                        "functionCall"
                                    };
                                    push_semantic_token(tokens, token_seen, &span, token_kind);
                                    push_hover_item(hover_items, hover_seen, &span, definition);
                                } else if let Some(binding) = bindings.get(&name) {
                                    push_semantic_token(tokens, token_seen, &span, "functionCall");
                                    push_hover_item(hover_items, hover_seen, &span, binding);
                                } else if let Some(import_alias) = imports.get(&name) {
                                    let token_kind = python_callable_token_kind(&name);
                                    push_semantic_token(tokens, token_seen, &span, token_kind);
                                    push_hover_item(
                                        hover_items,
                                        hover_seen,
                                        &span,
                                        &HoverTemplate {
                                            kind: import_alias.hover_kind.clone(),
                                            title: name,
                                            detail: None,
                                            source: Some(import_alias.statement.clone()),
                                        },
                                    );
                                }
                            }
                        }
                        "attribute" => {
                            let object_name = function_node.child_by_field_name("object").and_then(
                                |object_node| {
                                    if object_node.kind() == "identifier" {
                                        node_text(object_node, source)
                                    } else {
                                        None
                                    }
                                },
                            );

                            if let Some(object_node) = function_node.child_by_field_name("object") {
                                if object_node.kind() == "identifier" {
                                    if let Some(object_name) = node_text(object_node, source) {
                                        if let Some(import_alias) = imports.get(&object_name) {
                                            let span = span_from_node(object_node);
                                            push_semantic_token(
                                                tokens,
                                                token_seen,
                                                &span,
                                                &import_alias.token_kind,
                                            );
                                            push_hover_item(
                                                hover_items,
                                                hover_seen,
                                                &span,
                                                &HoverTemplate {
                                                    kind: import_alias.hover_kind.clone(),
                                                    title: object_name,
                                                    detail: None,
                                                    source: Some(import_alias.statement.clone()),
                                                },
                                            );
                                        } else if let Some(binding) = bindings.get(&object_name) {
                                            let span = span_from_node(object_node);
                                            push_semantic_token(
                                                tokens, token_seen, &span, "variable",
                                            );
                                            push_hover_item(
                                                hover_items,
                                                hover_seen,
                                                &span,
                                                binding,
                                            );
                                        } else if let Some(definition) =
                                            definitions.get(&object_name)
                                        {
                                            let span = span_from_node(object_node);
                                            push_semantic_token(
                                                tokens,
                                                token_seen,
                                                &span,
                                                reference_token_kind_for_hover(definition),
                                            );
                                            push_hover_item(
                                                hover_items,
                                                hover_seen,
                                                &span,
                                                definition,
                                            );
                                        }
                                    }
                                }
                            }

                            if let Some(attribute_node) =
                                function_node.child_by_field_name("attribute")
                            {
                                let span = span_from_node(attribute_node);
                                let attribute_name =
                                    node_text(attribute_node, source).unwrap_or_default();
                                let token_kind =
                                    if let Some(definition) = definitions.get(&attribute_name) {
                                        if definition.kind == "Class" {
                                            "classReference"
                                        } else {
                                            "functionCall"
                                        }
                                    } else {
                                        python_callable_token_kind(&attribute_name)
                                    };
                                push_semantic_token(tokens, token_seen, &span, token_kind);

                                if let Some(definition) = definitions.get(&attribute_name) {
                                    push_hover_item(hover_items, hover_seen, &span, definition);
                                } else if let Some(object_name) = object_name.as_deref() {
                                    if let Some(import_alias) = imports.get(object_name) {
                                        let hover = imported_member_hover_template(
                                            object_name,
                                            &attribute_name,
                                            import_alias,
                                            true,
                                        );
                                        push_hover_item(hover_items, hover_seen, &span, &hover);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            "attribute" => {
                let is_call_target = node.parent().is_some_and(|parent| {
                    parent.kind() == "call"
                        && parent
                            .child_by_field_name("function")
                            .is_some_and(|function| function == node)
                });

                if !is_call_target {
                    let object_name = node.child_by_field_name("object").and_then(|object_node| {
                        if object_node.kind() == "identifier" {
                            node_text(object_node, source)
                        } else {
                            None
                        }
                    });

                    if let Some(object_node) = node.child_by_field_name("object") {
                        if object_node.kind() == "identifier" {
                            if let Some(object_name) = node_text(object_node, source) {
                                if let Some(import_alias) = imports.get(&object_name) {
                                    let span = span_from_node(object_node);
                                    push_semantic_token(
                                        tokens,
                                        token_seen,
                                        &span,
                                        &import_alias.token_kind,
                                    );
                                    push_hover_item(
                                        hover_items,
                                        hover_seen,
                                        &span,
                                        &HoverTemplate {
                                            kind: import_alias.hover_kind.clone(),
                                            title: object_name,
                                            detail: None,
                                            source: Some(import_alias.statement.clone()),
                                        },
                                    );
                                } else if let Some(binding) = bindings.get(&object_name) {
                                    let span = span_from_node(object_node);
                                    push_semantic_token(tokens, token_seen, &span, "variable");
                                    push_hover_item(hover_items, hover_seen, &span, binding);
                                } else if let Some(definition) = definitions.get(&object_name) {
                                    let span = span_from_node(object_node);
                                    push_semantic_token(
                                        tokens,
                                        token_seen,
                                        &span,
                                        reference_token_kind_for_hover(definition),
                                    );
                                    push_hover_item(hover_items, hover_seen, &span, definition);
                                }
                            }
                        }
                    }

                    if let Some(attribute_node) = node.child_by_field_name("attribute") {
                        let span = span_from_node(attribute_node);
                        let attribute_name = node_text(attribute_node, source).unwrap_or_default();
                        let token_kind = if let Some(definition) = definitions.get(&attribute_name)
                        {
                            reference_token_kind_for_hover(definition)
                        } else {
                            python_attribute_token_kind(&attribute_name)
                        };
                        push_semantic_token(tokens, token_seen, &span, token_kind);

                        if let Some(definition) = definitions.get(&attribute_name) {
                            push_hover_item(hover_items, hover_seen, &span, definition);
                        } else if let Some(object_name) = object_name.as_deref() {
                            if let Some(import_alias) = imports.get(object_name) {
                                let hover = imported_member_hover_template(
                                    object_name,
                                    &attribute_name,
                                    import_alias,
                                    false,
                                );
                                push_hover_item(hover_items, hover_seen, &span, &hover);
                            }
                        }
                    }
                }
            }
            "identifier" => {
                if is_python_definition_name(node)
                    || is_python_parameter_node(node)
                    || is_python_import_context(node)
                {
                    // Definition and import ranges are already handled earlier.
                } else if let Some(name) = node_text(node, source) {
                    let span = span_from_node(node);
                    if let Some(import_alias) = imports.get(&name) {
                        push_semantic_token(tokens, token_seen, &span, &import_alias.token_kind);
                        push_hover_item(
                            hover_items,
                            hover_seen,
                            &span,
                            &HoverTemplate {
                                kind: import_alias.hover_kind.clone(),
                                title: name,
                                detail: None,
                                source: Some(import_alias.statement.clone()),
                            },
                        );
                    } else if let Some(binding) = bindings.get(&name) {
                        push_semantic_token(tokens, token_seen, &span, "variable");
                        push_hover_item(hover_items, hover_seen, &span, binding);
                    } else if let Some(definition) = definitions.get(&name) {
                        let token_kind = reference_token_kind_for_hover(definition);
                        push_semantic_token(tokens, token_seen, &span, token_kind);
                        push_hover_item(hover_items, hover_seen, &span, definition);
                    }
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named() {
                collect_python_reference_semantics(
                    child,
                    source,
                    imports,
                    definitions,
                    bindings,
                    tokens,
                    token_seen,
                    hover_items,
                    hover_seen,
                );
            }
        }
    }

    fn collect_python_parameter_semantics(
        node: Node<'_>,
        source: &[u8],
        bindings: &mut BTreeMap<String, HoverTemplate>,
        tokens: &mut Vec<SemanticToken>,
        token_seen: &mut BTreeSet<String>,
        hover_items: &mut Vec<HoverItem>,
        hover_seen: &mut BTreeSet<String>,
    ) {
        let mut identifiers = Vec::new();
        collect_python_parameter_identifiers(node, &mut identifiers);

        for identifier in identifiers {
            if let Some(name) = node_text(identifier, source) {
                let span = span_from_node(identifier);
                let hover = HoverTemplate {
                    kind: "Parameter".into(),
                    title: name.clone(),
                    detail: None,
                    source: Some(format!("Parameter · line {}", span.start_line)),
                };
                bindings.entry(name).or_insert_with(|| hover.clone());
                push_semantic_token(tokens, token_seen, &span, "parameter");
                push_hover_item(hover_items, hover_seen, &span, &hover);
            }
        }
    }

    fn collect_python_binding_semantics(
        node: Node<'_>,
        source: &[u8],
        bindings: &mut BTreeMap<String, HoverTemplate>,
        tokens: &mut Vec<SemanticToken>,
        token_seen: &mut BTreeSet<String>,
        hover_items: &mut Vec<HoverItem>,
        hover_seen: &mut BTreeSet<String>,
    ) {
        let mut identifiers = Vec::new();
        collect_python_binding_identifiers(node, &mut identifiers);

        for identifier in identifiers {
            if let Some(name) = node_text(identifier, source) {
                if name == "_" {
                    continue;
                }

                let span = span_from_node(identifier);
                let hover = HoverTemplate {
                    kind: "Variable".into(),
                    title: name.clone(),
                    detail: None,
                    source: Some(format!("Defined in this file · line {}", span.start_line)),
                };
                bindings.entry(name).or_insert_with(|| hover.clone());
                push_semantic_token(tokens, token_seen, &span, "variableDefinition");
                push_hover_item(hover_items, hover_seen, &span, &hover);
            }
        }
    }

    fn collect_python_parameter_identifiers<'tree>(node: Node<'tree>, out: &mut Vec<Node<'tree>>) {
        if !node.is_named() {
            return;
        }

        match node.kind() {
            "identifier" => {
                out.push(node);
                return;
            }
            "default_parameter" | "typed_parameter" | "typed_default_parameter" => {
                if let Some(name) = node.child_by_field_name("name") {
                    collect_python_parameter_identifiers(name, out);
                    return;
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named() {
                collect_python_parameter_identifiers(child, out);
            }
        }
    }

    fn collect_python_binding_identifiers<'tree>(node: Node<'tree>, out: &mut Vec<Node<'tree>>) {
        if !node.is_named() {
            return;
        }

        match node.kind() {
            "identifier" => {
                out.push(node);
                return;
            }
            "attribute" => return,
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named() {
                collect_python_binding_identifiers(child, out);
            }
        }
    }

    fn push_semantic_token(
        tokens: &mut Vec<SemanticToken>,
        seen: &mut BTreeSet<String>,
        span: &TextSpan,
        kind: &str,
    ) {
        let key = format!(
            "{}:{}:{}:{}:{}",
            kind, span.start_line, span.start_column, span.end_line, span.end_column
        );
        if !seen.insert(key) {
            return;
        }

        tokens.push(SemanticToken {
            kind: kind.into(),
            start_line: span.start_line,
            start_column: span.start_column,
            end_line: span.end_line,
            end_column: span.end_column,
        });
    }

    fn push_hover_item(
        hover_items: &mut Vec<HoverItem>,
        seen: &mut BTreeSet<String>,
        span: &TextSpan,
        hover: &HoverTemplate,
    ) {
        let key = format!(
            "{}:{}:{}:{}:{}:{}",
            hover.kind,
            hover.title,
            span.start_line,
            span.start_column,
            span.end_line,
            span.end_column
        );
        if !seen.insert(key) {
            return;
        }

        hover_items.push(HoverItem {
            kind: hover.kind.clone(),
            title: hover.title.clone(),
            detail: hover.detail.clone(),
            source: hover.source.clone(),
            start_line: span.start_line,
            start_column: span.start_column,
            end_line: span.end_line,
            end_column: span.end_column,
        });
    }

    fn span_from_node(node: Node<'_>) -> TextSpan {
        let start = node.start_position();
        let end = node.end_position();

        TextSpan {
            start_line: start.row as u32 + 1,
            start_column: start.column as u32 + 1,
            end_line: end.row as u32 + 1,
            end_column: end.column as u32 + 1,
        }
    }

    fn node_text(node: Node<'_>, source: &[u8]) -> Option<String> {
        Some(node.utf8_text(source).ok()?.trim().to_string())
    }

    fn signature_line_for_node(node: Node<'_>, content: &str) -> String {
        let line = content
            .lines()
            .nth(node.start_position().row)
            .unwrap_or_default()
            .trim();
        truncate_chars(line, 120)
    }

    fn extract_python_docstring(node: Node<'_>, source: &[u8]) -> Option<String> {
        let body = node.child_by_field_name("body")?;
        let mut cursor = body.walk();
        let first_statement = body.named_children(&mut cursor).next()?;
        if first_statement.kind() != "expression_statement" {
            return None;
        }

        let mut statement_cursor = first_statement.walk();
        for child in first_statement.named_children(&mut statement_cursor) {
            if matches!(child.kind(), "string" | "concatenated_string") {
                let raw = child.utf8_text(source).ok()?;
                let cleaned = clean_python_docstring(raw);
                if !cleaned.is_empty() {
                    return Some(truncate_chars(&cleaned, 280));
                }
            }
        }

        None
    }

    fn clean_python_docstring(raw: &str) -> String {
        let without_prefix = raw.trim().trim_start_matches(|char: char| {
            matches!(char, 'r' | 'R' | 'u' | 'U' | 'b' | 'B' | 'f' | 'F')
        });

        without_prefix
            .trim_matches('"')
            .trim_matches('\'')
            .lines()
            .map(str::trim)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string()
    }

    fn is_python_definition_name(node: Node<'_>) -> bool {
        node.parent().is_some_and(|parent| {
            matches!(parent.kind(), "function_definition" | "class_definition")
                && parent
                    .child_by_field_name("name")
                    .is_some_and(|name| name == node)
        })
    }

    fn is_python_method_definition(node: Node<'_>) -> bool {
        node.parent().is_some_and(|parent| {
            parent.kind() == "block"
                && parent
                    .parent()
                    .is_some_and(|grandparent| grandparent.kind() == "class_definition")
        })
    }

    fn is_python_parameter_node(node: Node<'_>) -> bool {
        node.parent().is_some_and(|parent| {
            matches!(
                parent.kind(),
                "parameters" | "default_parameter" | "typed_parameter" | "typed_default_parameter"
            )
        })
    }

    fn is_python_import_context(node: Node<'_>) -> bool {
        node.parent().is_some_and(|parent| {
            matches!(
                parent.kind(),
                "import_statement"
                    | "import_from_statement"
                    | "aliased_import"
                    | "dotted_name"
                    | "wildcard_import"
            )
        })
    }

    fn collect_python_import_entries(
        content: &str,
    ) -> (BTreeMap<String, PythonImportAlias>, Vec<PythonImportAlias>) {
        let mut aliases = BTreeMap::new();
        let mut entries = Vec::new();

        for (line_index, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") {
                let mut search_start = 0usize;
                for segment in trimmed["import ".len()..].split(',') {
                    let entry = segment.trim();
                    if entry.is_empty() {
                        continue;
                    }

                    let (source_name, alias_name) = parse_python_import_alias(entry);
                    let statement = format!("import {}", entry);
                    let mut module_search = search_start;

                    for part in source_name.split('.') {
                        if let Some(span) =
                            identifier_span_on_line(line, line_index, part, module_search)
                        {
                            module_search = span.end_column.saturating_sub(1) as usize;
                            entries.push(PythonImportAlias {
                                alias: part.to_string(),
                                statement: statement.clone(),
                                span,
                                token_kind: "namespace".into(),
                                hover_kind: "Module".into(),
                            });
                        }
                    }

                    let alias = alias_name.unwrap_or_else(|| {
                        source_name
                            .rsplit('.')
                            .next()
                            .unwrap_or(source_name.as_str())
                            .to_string()
                    });

                    if let Some(span) =
                        identifier_span_on_line(line, line_index, &alias, module_search)
                    {
                        let alias_entry = PythonImportAlias {
                            alias: alias.clone(),
                            statement: statement.clone(),
                            span: span.clone(),
                            token_kind: "namespace".into(),
                            hover_kind: "Module".into(),
                        };
                        aliases
                            .entry(alias.clone())
                            .or_insert_with(|| alias_entry.clone());
                        entries.push(alias_entry);
                        search_start = span.end_column.saturating_sub(1) as usize;
                    }
                }
            } else if trimmed.starts_with("from ") && trimmed.contains(" import ") {
                let mut parts = trimmed["from ".len()..].splitn(2, " import ");
                let module = parts.next().unwrap_or_default().trim();
                let imported = parts
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .trim_start_matches('(')
                    .trim_end_matches(')');

                let mut module_search = 0usize;
                for part in module.split('.') {
                    if let Some(span) =
                        identifier_span_on_line(line, line_index, part, module_search)
                    {
                        module_search = span.end_column.saturating_sub(1) as usize;
                        entries.push(PythonImportAlias {
                            alias: part.to_string(),
                            statement: format!("from {} import {}", module, imported),
                            span,
                            token_kind: "namespace".into(),
                            hover_kind: "Module".into(),
                        });
                    }
                }

                let mut search_start = module_search;
                for segment in imported.split(',') {
                    let entry = segment.trim();
                    if entry.is_empty() || entry == "*" {
                        continue;
                    }

                    let (source_name, alias_name) = parse_python_import_alias(entry);
                    let alias = alias_name.unwrap_or_else(|| source_name.clone());
                    if let Some(span) =
                        identifier_span_on_line(line, line_index, &alias, search_start)
                    {
                        let (token_kind, hover_kind) = import_symbol_kind(&alias);
                        let alias_entry = PythonImportAlias {
                            alias: alias.clone(),
                            statement: format!("from {} import {}", module, entry),
                            span: span.clone(),
                            token_kind: token_kind.into(),
                            hover_kind: hover_kind.into(),
                        };
                        aliases
                            .entry(alias.clone())
                            .or_insert_with(|| alias_entry.clone());
                        entries.push(alias_entry);
                        search_start = span.end_column.saturating_sub(1) as usize;
                    }
                }
            }
        }

        (aliases, entries)
    }

    fn parse_python_import_alias(segment: &str) -> (String, Option<String>) {
        if let Some((source_name, alias_name)) = segment.split_once(" as ") {
            (
                source_name.trim().to_string(),
                Some(alias_name.trim().to_string()),
            )
        } else {
            (segment.trim().to_string(), None)
        }
    }

    fn import_symbol_kind(name: &str) -> (&'static str, &'static str) {
        if name
            .chars()
            .next()
            .is_some_and(|value| value.is_uppercase())
        {
            ("classReference", "Imported class")
        } else {
            ("variable", "Imported symbol")
        }
    }

    fn python_callable_token_kind(name: &str) -> &'static str {
        if name
            .chars()
            .next()
            .is_some_and(|value| value.is_uppercase())
        {
            "classReference"
        } else {
            "functionCall"
        }
    }

    fn python_attribute_token_kind(name: &str) -> &'static str {
        if name
            .chars()
            .next()
            .is_some_and(|value| value.is_uppercase())
        {
            "classReference"
        } else {
            "property"
        }
    }

    fn reference_token_kind_for_hover(hover: &HoverTemplate) -> &'static str {
        match hover.kind.as_str() {
            "Class" | "Imported class" => "classReference",
            "Function" | "Method" => "functionDefinition",
            "Parameter" => "parameter",
            _ => "variable",
        }
    }

    fn imported_member_hover_template(
        object_name: &str,
        member_name: &str,
        import_alias: &PythonImportAlias,
        is_call_target: bool,
    ) -> HoverTemplate {
        let kind = if is_call_target {
            if python_callable_token_kind(member_name) == "classReference" {
                "Imported class"
            } else {
                "Imported function"
            }
        } else if python_attribute_token_kind(member_name) == "classReference" {
            "Imported class"
        } else {
            "Imported member"
        };

        HoverTemplate {
            kind: kind.into(),
            title: format!("{object_name}.{member_name}"),
            detail: None,
            source: Some(import_alias.statement.clone()),
        }
    }

    fn identifier_span_on_line(
        line: &str,
        line_index: usize,
        identifier: &str,
        preferred_start: usize,
    ) -> Option<TextSpan> {
        let start = line[preferred_start.min(line.len())..]
            .find(identifier)
            .map(|offset| preferred_start.min(line.len()) + offset)
            .or_else(|| line.find(identifier))?;

        let start_column = start as u32 + 1;
        let end_column = start_column + identifier.chars().count() as u32;

        Some(TextSpan {
            start_line: line_index as u32 + 1,
            start_column,
            end_line: line_index as u32 + 1,
            end_column,
        })
    }
}

fn collect_symbols_recursive(
    node: Node<'_>,
    source: &[u8],
    language: SourceLanguage,
    symbols: &mut Vec<SymbolEntry>,
) {
    if let Some(symbol) = symbol_from_node(node, source, language) {
        symbols.push(symbol);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.is_named() {
            collect_symbols_recursive(child, source, language, symbols);
        }
    }
}

fn symbol_from_node(
    node: Node<'_>,
    source: &[u8],
    language: SourceLanguage,
) -> Option<SymbolEntry> {
    let kind = node.kind();
    let label = match language {
        SourceLanguage::Python => match kind {
            "function_definition" => read_field_text(node, source, "name"),
            "class_definition" => read_field_text(node, source, "name"),
            _ => None,
        },
        SourceLanguage::Rust => match kind {
            "function_item" | "struct_item" | "enum_item" | "trait_item" | "type_item" => {
                read_field_text(node, source, "name")
            }
            "impl_item" => read_field_text(node, source, "type"),
            _ => None,
        },
        SourceLanguage::C | SourceLanguage::Cpp | SourceLanguage::Cuda => match kind {
            "function_definition" => read_declarator_name(node, source),
            "class_specifier"
            | "struct_specifier"
            | "enum_specifier"
            | "union_specifier"
            | "namespace_definition" => read_field_text(node, source, "name"),
            _ => None,
        },
        SourceLanguage::JavaScript | SourceLanguage::TypeScript | SourceLanguage::Tsx => match kind
        {
            "function_declaration"
            | "class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "method_definition" => read_field_text(node, source, "name"),
            _ => None,
        },
    }?;

    let start = node.start_position();
    let end = node.end_position();

    Some(SymbolEntry {
        kind: prettify_symbol_kind(kind).into(),
        label,
        start_line: start.row as u32 + 1,
        end_line: end.row as u32 + 1,
    })
}

fn read_field_text(node: Node<'_>, source: &[u8], field_name: &str) -> Option<String> {
    let field = node.child_by_field_name(field_name)?;
    Some(field.utf8_text(source).ok()?.trim().to_string())
}

fn read_declarator_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "type_identifier" | "operator_name"
    ) {
        return node
            .utf8_text(source)
            .ok()
            .map(|value| value.trim().to_string());
    }

    for field_name in ["name", "declarator", "type"] {
        if let Some(field) = node.child_by_field_name(field_name) {
            if let Some(name) = read_declarator_name(field, source) {
                return Some(name);
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(name) = read_declarator_name(child, source) {
            return Some(name);
        }
    }

    None
}

fn prettify_symbol_kind(kind: &str) -> &'static str {
    match kind {
        "function_definition" | "function_declaration" | "function_item" => "function",
        "class_definition" | "class_declaration" | "class_specifier" => "class",
        "method_definition" => "method",
        "struct_item" | "struct_specifier" => "struct",
        "enum_item" | "enum_declaration" | "enum_specifier" => "enum",
        "union_specifier" => "union",
        "trait_item" => "trait",
        "interface_declaration" => "interface",
        "impl_item" => "impl",
        "namespace_definition" => "namespace",
        "type_alias_declaration" | "type_item" => "type",
        _ => "symbol",
    }
}

fn compose_compact_context(
    root: &Path,
    current_file: Option<&PathBuf>,
    content: Option<&str>,
) -> Result<String, String> {
    let mut sections = Vec::new();
    let workspace_name = root
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| path_to_string(root));

    sections.push(format!("workspace: {}", workspace_name));

    if let Some(path) = current_file {
        let current_source = match content {
            Some(source) => source.to_string(),
            None => fs::read_to_string(path).unwrap_or_default(),
        };

        let relative = relative_path(root, path);
        sections.push(format!(
            "current file: {}\n{}",
            relative,
            summarize_file_for_context(path, &current_source)
        ));
    }

    let mut candidates = collect_context_candidates(root, current_file);
    candidates.truncate(CONTEXT_FILE_LIMIT);

    if !candidates.is_empty() {
        let mut project_summary = String::from("related files:");
        for candidate in candidates {
            if let Ok(source) = fs::read_to_string(&candidate) {
                let summary = summarize_file_for_context(&candidate, &source);
                project_summary.push_str("\n\n");
                project_summary.push_str(&relative_path(root, &candidate));
                project_summary.push('\n');
                project_summary.push_str(&summary);
            }
        }
        sections.push(project_summary);
    }

    let mut context = sections.join("\n\n");
    if context.chars().count() > MAX_CONTEXT_CHARS {
        context = truncate_chars(&context, MAX_CONTEXT_CHARS);
    }

    Ok(context)
}

fn collect_context_candidates(root: &Path, current_file: Option<&PathBuf>) -> Vec<PathBuf> {
    let current_parent = current_file.and_then(|path| path.parent().map(Path::to_path_buf));
    let current_file = current_file.cloned();
    let mut weighted = Vec::new();

    for entry in WalkDir::new(root)
        .max_depth(4)
        .into_iter()
        .filter_entry(|entry| !should_ignore_name(&entry.file_name().to_string_lossy()))
        .filter_map(Result::ok)
    {
        let path = entry.into_path();
        if !path.is_file() || parser_language_for_path(&path).is_none() {
            continue;
        }

        if current_file
            .as_ref()
            .is_some_and(|candidate| candidate == &path)
        {
            continue;
        }

        let mut weight = 50i32;
        if let Some(parent) = &current_parent {
            if path
                .parent()
                .is_some_and(|path_parent| path_parent == parent)
            {
                weight -= 20;
            }
            if path.starts_with(parent) {
                weight -= 10;
            }
        }

        weighted.push((weight, relative_path(root, &path), path));
    }

    weighted.sort_by(|left, right| left.cmp(right));
    weighted.into_iter().map(|(_, _, path)| path).collect()
}

fn summarize_file_for_context(path: &Path, content: &str) -> String {
    let language = language_id_from_path(path);
    let symbols = parse_symbols_for_path(path, content);
    let imports = match parser_language_for_path(path) {
        Some(SourceLanguage::Python) => collect_python_imports(content)
            .unwrap_or_default()
            .into_iter()
            .map(|candidate| candidate.module)
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };

    let mut summary = Vec::new();
    summary.push(format!("language: {}", language));

    if !imports.is_empty() {
        summary.push(format!(
            "imports: {}",
            imports.into_iter().take(8).collect::<Vec<_>>().join(", ")
        ));
    }

    if !symbols.is_empty() {
        let symbols_text = symbols
            .into_iter()
            .take(8)
            .map(|symbol| format!("- {} {} @{}", symbol.kind, symbol.label, symbol.start_line))
            .collect::<Vec<_>>()
            .join("\n");
        summary.push(format!("symbols:\n{}", symbols_text));
    }

    summary.push(format!("excerpt:\n{}", render_excerpt(content, 18, 900)));

    summary.join("\n")
}

fn render_excerpt(content: &str, max_lines: usize, max_chars: usize) -> String {
    let joined = content
        .lines()
        .take(max_lines)
        .collect::<Vec<_>>()
        .join("\n");

    truncate_chars(&joined, max_chars)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        truncated.push_str("\n...[truncated]");
    }
    truncated
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|_| path_to_string(path))
}

fn collect_python_imports(source: &str) -> Result<Vec<ImportCandidate>, String> {
    let Some(tree) = parse_tree(SourceLanguage::Python, source) else {
        return Ok(Vec::new());
    };

    let mut imports = BTreeMap::<String, ImportCandidate>::new();
    collect_python_import_nodes(tree.root_node(), source, &mut imports)?;

    Ok(imports.into_values().collect())
}

fn collect_python_import_nodes(
    node: Node<'_>,
    source: &str,
    imports: &mut BTreeMap<String, ImportCandidate>,
) -> Result<(), String> {
    match node.kind() {
        "import_statement" | "import_from_statement" => {
            let text = node
                .utf8_text(source.as_bytes())
                .map_err(|err| err.to_string())?;
            let start = node.start_position();
            let modules = parse_import_modules(text);
            for module in modules {
                imports.entry(module.clone()).or_insert(ImportCandidate {
                    module,
                    line: start.row as u32 + 1,
                    column: start.column as u32 + 1,
                });
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.is_named() {
            collect_python_import_nodes(child, source, imports)?;
        }
    }

    Ok(())
}

fn parse_import_modules(statement: &str) -> Vec<String> {
    let trimmed = statement.trim();

    if let Some(remainder) = trimmed.strip_prefix("import ") {
        return remainder
            .split(',')
            .filter_map(normalize_import_target)
            .collect();
    }

    if let Some(remainder) = trimmed.strip_prefix("from ") {
        let target = remainder.split(" import ").next().unwrap_or_default();
        return normalize_import_target(target).into_iter().collect();
    }

    Vec::new()
}

fn normalize_import_target(value: &str) -> Option<String> {
    let token = value.trim().split_whitespace().next()?.trim();
    if token.is_empty() || token.starts_with('.') {
        return None;
    }

    Some(token.split('.').next()?.trim().to_string())
}

fn ensure_python_environment(root: &Path, uv_path: &str) -> Result<(), String> {
    let venv_python = venv_python_path(root);
    if venv_python.exists() {
        return Ok(());
    }

    let mut command = Command::new(uv_path);
    command.current_dir(root);
    hide_background_window(&mut command);
    if root.join("pyproject.toml").exists() {
        command.args(["sync", "--quiet"]);
    } else {
        command.arg("venv");
    }

    let output = command.output().map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn venv_python_path(root: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        root.join(".venv").join("Scripts").join("python.exe")
    } else {
        root.join(".venv").join("bin").join("python")
    }
}

fn python_module_exists(root: &Path, module: &str) -> Result<bool, String> {
    let python = venv_python_path(root);
    if !python.exists() {
        return Ok(false);
    }

    let probe = format!(
        "import importlib.util, sys; sys.exit(0 if importlib.util.find_spec({module:?}) else 7)"
    );

    let mut command = Command::new(&python);
    command.current_dir(root);
    command.args(["-c", &probe]);
    hide_background_window(&mut command);
    let output = command.output().map_err(|err| err.to_string())?;

    Ok(output.status.success())
}

fn install_command_preview(root: &Path, package: &str) -> String {
    if root.join("pyproject.toml").exists() {
        format!("uv add {}", package)
    } else {
        format!("uv pip install --python .venv {}", package)
    }
}

fn install_python_package(root: &Path, uv_path: &str, package: &str) -> Result<String, String> {
    let mut command = Command::new(uv_path);
    command.current_dir(root);
    hide_background_window(&mut command);
    if root.join("pyproject.toml").exists() {
        command.args(["add", package]);
    } else {
        command
            .args(["pip", "install", "--python"])
            .arg(".venv")
            .arg(package);
    }

    let output = command.output().map_err(|err| err.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let combined = [stdout, stderr]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if output.status.success() {
        Ok(combined)
    } else {
        Err(combined)
    }
}

fn python_install_key(root: &Path, package: &str) -> String {
    format!(
        "{}::{}",
        path_to_string(root),
        package.trim().to_ascii_lowercase()
    )
}

fn python_install_failures() -> &'static Mutex<BTreeMap<String, Instant>> {
    PYTHON_INSTALL_FAILURES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn python_install_running() -> &'static Mutex<BTreeSet<String>> {
    PYTHON_INSTALL_IN_PROGRESS.get_or_init(|| Mutex::new(BTreeSet::new()))
}

fn python_install_in_progress(key: &str) -> bool {
    python_install_running()
        .lock()
        .map(|state| state.contains(key))
        .unwrap_or(false)
}

fn mark_python_install_started(key: &str) {
    if let Ok(mut state) = python_install_running().lock() {
        state.insert(key.to_string());
    }
}

fn clear_python_install_started(key: &str) {
    if let Ok(mut state) = python_install_running().lock() {
        state.remove(key);
    }
}

fn mark_python_install_failed(key: &str) {
    if let Ok(mut state) = python_install_failures().lock() {
        state.insert(key.to_string(), Instant::now());
    }
}

fn clear_python_install_failure(key: &str) {
    if let Ok(mut state) = python_install_failures().lock() {
        state.remove(key);
    }
}

fn python_install_cooldown_remaining(key: &str) -> Option<Duration> {
    let Ok(mut state) = python_install_failures().lock() else {
        return None;
    };

    let failed_at = state.get(key).copied()?;
    let elapsed = failed_at.elapsed();
    if elapsed >= PYTHON_INSTALL_FAILURE_COOLDOWN {
        state.remove(key);
        return None;
    }

    Some(PYTHON_INSTALL_FAILURE_COOLDOWN.saturating_sub(elapsed))
}

fn python_package_name(module: &str) -> String {
    match module {
        "PIL" => "Pillow",
        "bs4" => "beautifulsoup4",
        "cv2" => "opencv-python",
        "dotenv" => "python-dotenv",
        "sklearn" => "scikit-learn",
        "yaml" => "PyYAML",
        _ => module,
    }
    .to_string()
}

fn build_app_menu<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<Menu<R>> {
    let pkg = app.package_info();
    let about_metadata = AboutMetadata {
        name: Some(pkg.name.clone()),
        version: Some(pkg.version.to_string()),
        comments: Some(
            "A lightweight desktop IDE with agent workflows, tree-sitter context, and uv-backed Python management."
                .into(),
        ),
        authors: Some(vec!["Entity-27th".into()]),
        ..Default::default()
    };

    let open_folder = MenuItem::with_id(
        app,
        "file.open_folder",
        "&Open Folder...",
        true,
        Some("CmdOrCtrl+O"),
    )?;
    let new_file = MenuItem::with_id(
        app,
        "file.new_file",
        "New &File...",
        true,
        Some("CmdOrCtrl+Alt+N"),
    )?;
    let save = MenuItem::with_id(app, "file.save", "&Save", true, Some("CmdOrCtrl+S"))?;
    let new_chat = MenuItem::with_id(
        app,
        "file.new_chat",
        "&New Agent Task",
        true,
        Some("CmdOrCtrl+N"),
    )?;
    let close_tab = MenuItem::with_id(
        app,
        "file.close_tab",
        "&Close Active Tab",
        true,
        Some("CmdOrCtrl+W"),
    )?;
    let focus_chat =
        MenuItem::with_id(app, "view.focus_chat", "Show &Agents", true, Some("Alt+1"))?;
    let focus_access = MenuItem::with_id(
        app,
        "view.focus_access",
        "Show &Access",
        true,
        Some("Alt+2"),
    )?;
    let focus_project = MenuItem::with_id(
        app,
        "view.focus_project",
        "Show &Project",
        true,
        Some("Alt+3"),
    )?;
    let focus_problems = MenuItem::with_id(
        app,
        "view.focus_problems",
        "Show Pro&blems",
        true,
        Some("Alt+4"),
    )?;
    let focus_outline = MenuItem::with_id(
        app,
        "view.focus_outline",
        "Show &Outline",
        true,
        Some("Alt+5"),
    )?;
    let toggle_terminal = MenuItem::with_id(
        app,
        "view.toggle_terminal",
        "Toggle &Terminal",
        true,
        Some("CmdOrCtrl+J"),
    )?;
    let refresh_context = MenuItem::with_id(
        app,
        "view.refresh_context",
        "&Refresh Compact Context",
        true,
        Some("CmdOrCtrl+Shift+R"),
    )?;
    let refresh_agents = MenuItem::with_id(
        app,
        "help.refresh_agents",
        "&Refresh Agent Access",
        true,
        Some("F6"),
    )?;

    Menu::with_items(
        app,
        &[
            &Submenu::with_items(
                app,
                "&File",
                true,
                &[
                    &new_file,
                    &open_folder,
                    &save,
                    &new_chat,
                    &close_tab,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::quit(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "&Edit",
                true,
                &[
                    &PredefinedMenuItem::undo(app, None)?,
                    &PredefinedMenuItem::redo(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::cut(app, None)?,
                    &PredefinedMenuItem::copy(app, None)?,
                    &PredefinedMenuItem::paste(app, None)?,
                    &PredefinedMenuItem::select_all(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "&View",
                true,
                &[
                    &focus_chat,
                    &focus_access,
                    &focus_project,
                    &focus_problems,
                    &focus_outline,
                    &toggle_terminal,
                    &PredefinedMenuItem::separator(app)?,
                    &refresh_context,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::fullscreen(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "&Window",
                true,
                &[
                    &PredefinedMenuItem::minimize(app, None)?,
                    &PredefinedMenuItem::maximize(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::close_window(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "&Help",
                true,
                &[
                    &refresh_agents,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::about(app, Some("&About Hematite"), Some(about_metadata))?,
                ],
            )?,
        ],
    )
}

fn emit_menu_action<R: tauri::Runtime>(app: &tauri::AppHandle<R>, event: tauri::menu::MenuEvent) {
    let _ = app.emit(
        "hematite://menu",
        FrontendMenuEvent {
            id: event.id().as_ref().to_string(),
        },
    );
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .menu(build_app_menu)
        .on_menu_event(emit_menu_action)
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            load_ui_state,
            save_ui_state,
            list_directory,
            read_file,
            save_file,
            create_file,
            extract_symbols,
            analyze_editor_semantics,
            request_editor_hover,
            request_editor_completions,
            request_editor_code_actions,
            request_editor_definition,
            request_editor_references,
            request_editor_inlay_hints,
            build_compact_context,
            refresh_agent_health,
            get_language_capabilities,
            create_agent_session,
            send_agent_session_message,
            cancel_agent_session_turn,
            save_agent_credentials,
            launch_agent_login,
            pick_workspace_directory,
            pick_service_account_file,
            inspect_python_environment,
            prepare_python_environment,
            inspect_c_family_environment,
            inspect_rust_environment,
            execute_terminal_command,
            start_terminal_session,
            write_terminal_session_input,
            write_terminal_session_command,
            stop_terminal_session,
            resize_terminal_session,
            refresh_tool_statuses,
            start_codeshare_session,
            join_codeshare_session,
            run_agent,
            start_codex_turn,
            respond_to_codex_approval,
            reset_codex_session,
            start_gemini_turn,
            respond_to_gemini_approval,
            reset_gemini_session,
            analyze_python_imports,
            analyze_rust_diagnostics,
            install_missing_python_imports,
            run_python_tooling_action,
            run_rust_tooling_action
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_capability_status_reports_missing_tool_without_features() {
        let status =
            language_capability_status_for_tool(SourceLanguage::Rust, "rust-analyzer", None, true);

        assert_eq!(status.language_id, "rust");
        assert_eq!(status.provider_name, "rust-analyzer");
        assert_eq!(status.availability, LanguageProviderAvailability::Missing);
        assert!(status.supported_features.is_empty());
        assert_eq!(
            status.inactive_reason.as_deref(),
            Some("rust-analyzer is not installed on PATH.")
        );
        assert_eq!(
            status.recommended_action.as_deref(),
            Some(
                "Install rust-analyzer or open Hematite from an environment where rust-analyzer is on PATH."
            )
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
        assert_eq!(
            status.resolved_path.as_deref(),
            Some("/tools/rust-analyzer")
        );
        assert!(
            status
                .supported_features
                .contains(&LanguageFeature::Diagnostics)
        );
        assert!(status.supported_features.contains(&LanguageFeature::Hover));
        assert!(
            status
                .supported_features
                .contains(&LanguageFeature::SemanticTokens)
        );
        assert!(status.inactive_reason.is_none());
        assert!(status.recommended_action.is_none());
    }

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
            .map(|status| {
                (
                    status.language_id.as_str(),
                    status.provider_name.as_str(),
                    status.availability,
                )
            })
            .collect();

        assert!(providers.contains(&("python", "ruff", LanguageProviderAvailability::Active)));
        assert!(providers.contains(&("python", "ty", LanguageProviderAvailability::Active)));
        assert!(providers.contains(&(
            "rust",
            "rust-analyzer",
            LanguageProviderAvailability::Active
        )));
        assert!(providers.contains(&("c", "clangd", LanguageProviderAvailability::Degraded)));
        assert!(providers.contains(&("cpp", "clangd", LanguageProviderAvailability::Degraded)));
        assert!(providers.contains(&(
            "cuda-cpp",
            "clangd",
            LanguageProviderAvailability::Degraded
        )));
    }

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

    #[test]
    fn agent_session_registry_creates_and_reuses_sessions() {
        let registry = AgentSessionRegistry::default();

        let created = registry.create_session(CreateAgentSessionRequest {
            provider_id: "codex".to_string(),
            model_id: Some("gpt-5.2".to_string()),
            permission_level: "ask".to_string(),
            workspace_root: Some("C:/work/project".to_string()),
        });
        let fetched = registry
            .get_session(&created.id)
            .expect("session should exist");

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

    #[test]
    fn agent_session_registry_bounds_session_count() {
        let registry = AgentSessionRegistry::default();
        let first = registry.create_session(CreateAgentSessionRequest {
            provider_id: "codex".to_string(),
            model_id: None,
            permission_level: "ask".to_string(),
            workspace_root: None,
        });

        for index in 0..MAX_AGENT_SESSIONS {
            registry.create_session(CreateAgentSessionRequest {
                provider_id: "codex".to_string(),
                model_id: Some(format!("model-{index}")),
                permission_level: "ask".to_string(),
                workspace_root: None,
            });
        }

        assert!(registry.get_session(&first.id).is_none());
        assert_eq!(
            registry
                .sessions
                .lock()
                .expect("agent session lock poisoned")
                .len(),
            MAX_AGENT_SESSIONS
        );
    }

    #[test]
    fn agent_session_message_and_attachment_content_are_bounded() {
        let mut session = AgentSession::new(
            "session-1".to_string(),
            "codex".to_string(),
            None,
            "ask".to_string(),
            None,
        );
        session.append_user_message(
            "x".repeat(MAX_AGENT_MESSAGE_CHARS + 16),
            vec![AgentContextAttachment {
                kind: "activeFile".to_string(),
                label: "large.py".to_string(),
                content: "y".repeat(MAX_AGENT_ATTACHMENT_CHARS + 16),
                estimated_chars: MAX_AGENT_ATTACHMENT_CHARS + 16,
            }],
        );

        let message = &session.messages[0];
        assert!(message.content.len() <= MAX_AGENT_MESSAGE_CHARS + "\n[truncated]".len());
        assert!(message.content.ends_with("[truncated]"));
        assert!(
            message.attachments[0].content.len()
                <= MAX_AGENT_ATTACHMENT_CHARS + "\n[truncated]".len()
        );
        assert!(message.attachments[0].content.ends_with("[truncated]"));
    }

    #[test]
    fn lsp_semantic_token_decoder_uses_ty_legend() {
        let response = json!({
            "data": [0, 0, 5, 0, 0, 0, 6, 3, 12, 0]
        });
        let token_types = vec!["namespace".into(), "variable".into(), "function".into()];
        let tokens = decode_lsp_semantic_tokens(&response, &token_types);

        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, "namespace");
        assert_eq!(tokens[0].start_line, 1);
        assert_eq!(tokens[0].start_column, 1);
        assert_eq!(tokens[1].kind, "identifier");
        assert_eq!(tokens[1].start_column, 7);
    }

    #[test]
    fn missing_import_diagnostics_are_install_candidates() {
        let candidates = vec![ImportCandidate {
            module: "torch".into(),
            line: 1,
            column: 1,
        }];
        let diagnostics = vec![EditorDiagnostic {
            module: "unresolved-import".into(),
            from: 0,
            to: 12,
            line: 1,
            column: 1,
            severity: "error".into(),
            message: "Cannot resolve imported module `torch`".into(),
        }];

        let missing = missing_imports_from_ty_diagnostics(&diagnostics, &candidates);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].module, "torch");
    }

    #[test]
    fn import_candidate_matching_tolerates_ty_message_shape() {
        let candidate = ImportCandidate {
            module: "peft".into(),
            line: 4,
            column: 1,
        };
        let diagnostic = EditorDiagnostic {
            module: "ty".into(),
            from: 0,
            to: 4,
            line: 4,
            column: 1,
            severity: "error".into(),
            message: "Cannot resolve imported module `peft`".into(),
        };

        assert!(diagnostic_matches_import_candidate(&diagnostic, &candidate));
    }

    #[test]
    fn lsp_publish_diagnostics_are_parsed_for_ty() {
        let source = "from unsloth import FastLanguageModel\n";
        let params = json!({
            "uri": "file:///workspace/train.py",
            "version": 3,
            "diagnostics": [{
                "range": {
                    "start": { "line": 0, "character": 5 },
                    "end": { "line": 0, "character": 12 }
                },
                "severity": 1,
                "code": "unresolved-import",
                "message": "Cannot resolve imported module `unsloth`"
            }]
        });

        let diagnostics = parse_lsp_publish_diagnostics(&params, "ty", source);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].module, "unresolved-import");
        assert_eq!(diagnostics[0].severity, "error");
        assert_eq!(diagnostics[0].line, 1);
        assert_eq!(diagnostics[0].column, 6);
        assert!(diagnostics[0].message.contains("unsloth"));
    }

    #[test]
    fn ty_concise_diagnostics_are_parsed_for_active_file() {
        let source = "import does_not_exist_hematite_probe\n";
        let file_path = Path::new(r"C:\workspace\sample.py");
        let raw = r"C:\workspace\sample.py:1:8: error[unresolved-import] Cannot resolve imported module `does_not_exist_hematite_probe`
Found 1 diagnostic";

        let diagnostics = parse_ty_concise_diagnostics(raw, file_path, source);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].module, "unresolved-import");
        assert_eq!(diagnostics[0].severity, "error");
        assert_eq!(diagnostics[0].line, 1);
        assert_eq!(diagnostics[0].column, 8);
        assert!(diagnostics[0].to > diagnostics[0].from);
    }

    #[test]
    fn codex_error_notification_reads_nested_turn_error() {
        let params = json!({
            "error": {
                "message": "The 'gpt-5.5' model requires a newer version of Codex.",
                "additionalDetails": "Upgrade the CLI or choose another model.",
                "codexErrorInfo": null
            },
            "willRetry": false,
            "threadId": "thread",
            "turnId": "turn"
        });

        let message = codex_error_notification_message(&params);

        assert!(message.contains("gpt-5.5"));
        assert!(message.contains("Upgrade the CLI"));
    }

    #[test]
    fn old_codex_cli_gets_safe_model_for_gpt_55_config() {
        assert_eq!(
            codex_model_override_from_parts(Some("gpt-5.5"), Some("codex-cli 0.118.0")),
            Some(CODEX_SAFE_MODEL_FOR_OLD_GPT55_CONFIG.into())
        );
        assert_eq!(
            codex_model_override_from_parts(Some("gpt-5.2"), Some("codex-cli 0.118.0")),
            None
        );
        assert_eq!(
            codex_model_override_from_parts(Some("gpt-5.5"), Some("codex-cli 0.119.0")),
            None
        );
    }

    #[test]
    fn ty_workspace_configuration_request_gets_editor_settings() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "workspace/configuration",
            "params": {
                "items": [
                    { "section": "ty" },
                    { "section": "python" },
                    { "section": "ty.disableLanguageServices" }
                ]
            }
        });

        let response = ty_server_request_response_for_root(r"C:\missing-root", &request)
            .expect("server request response");

        assert_eq!(response["id"], json!(7));
        assert_eq!(
            response["result"][0]["disableLanguageServices"],
            json!(false)
        );
        assert!(response["result"][0].get("configuration").is_some());
        assert_eq!(response["result"][1], json!({}));
        assert_eq!(response["result"][2], json!(false));
    }

    #[test]
    fn ty_configuration_item_can_return_python_environment() {
        let settings = json!({
            "configuration": {
                "environment": {
                    "python": r"C:\workspace\.venv\Scripts\python.exe"
                }
            },
            "disableLanguageServices": false
        });

        assert_eq!(
            ty_configuration_item_value(
                &json!({ "section": "ty.configuration.environment.python" }),
                &settings
            ),
            json!(r"C:\workspace\.venv\Scripts\python.exe")
        );
    }

    #[test]
    fn lsp_completion_items_are_parsed_for_python_features() {
        let response = json!({
            "isIncomplete": false,
            "items": [{
                "label": "DataFrame",
                "kind": 7,
                "detail": "class pandas.DataFrame",
                "insertText": "DataFrame"
            }]
        });

        let items = parse_lsp_completion_items(&response);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "DataFrame");
        assert_eq!(items[0].detail.as_deref(), Some("class pandas.DataFrame"));
        assert_eq!(items[0].insert_text.as_deref(), Some("DataFrame"));
    }

    #[test]
    fn lsp_locations_parse_plain_locations_and_location_links() {
        let response = json!([
            {
                "uri": "file:///C:/workspace/pkg/mod.py",
                "range": {
                    "start": { "line": 2, "character": 4 },
                    "end": { "line": 2, "character": 9 }
                }
            },
            {
                "targetUri": "file:///C:/workspace/pkg/other.py",
                "targetSelectionRange": {
                    "start": { "line": 9, "character": 1 },
                    "end": { "line": 9, "character": 6 }
                }
            }
        ]);

        let locations = parse_lsp_locations(&response);

        assert_eq!(locations.len(), 2);
        assert_eq!(
            locations[0].path.as_deref(),
            Some("C:/workspace/pkg/mod.py")
        );
        assert_eq!(locations[0].line, 3);
        assert_eq!(locations[0].column, 5);
        assert_eq!(
            locations[1].path.as_deref(),
            Some("C:/workspace/pkg/other.py")
        );
        assert_eq!(locations[1].line, 10);
        assert_eq!(locations[1].column, 2);
    }

    #[test]
    fn lsp_inlay_hints_parse_string_and_label_parts() {
        let response = json!([
            {
                "position": { "line": 0, "character": 12 },
                "label": ": int",
                "kind": 1
            },
            {
                "position": { "line": 1, "character": 8 },
                "label": [{ "value": " -> " }, { "value": "str" }],
                "kind": 2
            }
        ]);

        let hints = parse_lsp_inlay_hints(&response);

        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0].label, ": int");
        assert_eq!(hints[0].line, 1);
        assert_eq!(hints[0].column, 13);
        assert_eq!(hints[1].label, " -> str");
    }

    #[test]
    fn terminal_pty_shell_script_preserves_cwd_marker() {
        let script = terminal_pty_shell_script("echo hello", Path::new("/workspace"));

        assert!(script.contains("echo hello"));
        assert!(script.contains(TERMINAL_CWD_MARKER));
    }

    #[test]
    fn terminal_session_command_input_reports_status_and_cwd() {
        let input = terminal_session_command_input("echo hello");

        assert!(input.contains("echo hello"));
        assert!(input.contains(TERMINAL_STATUS_MARKER));
        assert!(input.contains(TERMINAL_CWD_MARKER));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn terminal_session_command_input_normalizes_venv_activation() {
        let input = terminal_session_command_input(".venv/Scripts/activate");

        assert!(input.contains(". '.\\.venv\\Scripts\\Activate.ps1'"));
        assert!(input.contains(TERMINAL_STATUS_MARKER));
        assert!(input.contains(TERMINAL_CWD_MARKER));
    }

    #[test]
    fn terminal_pty_executes_short_command_without_hanging() {
        let root =
            env::temp_dir().join(format!("hematite-terminal-pty-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temp root");

        #[cfg(target_os = "windows")]
        let command = "Write-Output hematite-terminal-ok";
        #[cfg(not(target_os = "windows"))]
        let command = "printf 'hematite-terminal-ok\\n'";

        let (success, output) =
            run_terminal_command_in_pty(command, &root, &AgentCredentials::default())
                .expect("run pty command");

        assert!(success);
        assert!(output.contains("hematite-terminal-ok"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn terminal_output_strips_ansi_sequences_before_display() {
        let output = "\u{1b}[?25l\u{1b}[2Jhello\u{1b}]0;title\u{7}\r\n";

        assert_eq!(strip_terminal_control_sequences(output), "hello\n");
    }

    #[test]
    fn lsp_hover_markup_uses_signature_as_title() {
        let hover = json!({
            "contents": {
                "kind": "markdown",
                "value": "```python\ndef add(x: int, y: int) -> int\n```\n---\nAdd two numbers."
            },
            "range": {
                "start": { "line": 4, "character": 8 },
                "end": { "line": 4, "character": 11 }
            }
        });

        let item = hover_item_from_lsp(&hover, 5, 9).expect("hover item");

        assert_eq!(item.title, "def add(x: int, y: int) -> int");
        assert_eq!(item.detail.as_deref(), Some("Add two numbers."));
    }

    #[test]
    fn lsp_hover_markup_keeps_multiline_signature_together() {
        let hover = json!({
            "contents": {
                "kind": "markdown",
                "value": "```python\ndef dummy(\n    input: torch.Tensor\n) -> torch.Tensor\n```\n---\nThis function is a dummy function."
            }
        });

        let item = hover_item_from_lsp(&hover, 1, 1).expect("hover item");

        assert_eq!(
            item.title,
            "def dummy(\n    input: torch.Tensor\n) -> torch.Tensor"
        );
        assert_eq!(
            item.detail.as_deref(),
            Some("This function is a dummy function.")
        );
    }

    #[test]
    fn function_name_hover_can_request_signature_help() {
        let source = "import torch\n\nvalue = torch.cumsum(input, dim=0)\n";
        let call = call_signature_request_position(source, 3, 15).expect("signature help position");

        assert_eq!(call.lsp_line, 2);
        assert_eq!(call.lsp_character, 21);
        assert_eq!(call.start_line, 3);
        assert_eq!(call.start_column, 15);
        assert_eq!(call.end_column, 21);
    }

    #[test]
    fn signature_help_becomes_hover_item() {
        let call = CallSignaturePosition {
            lsp_line: 0,
            lsp_character: 13,
            start_line: 1,
            start_column: 9,
            end_line: 1,
            end_column: 15,
        };
        let response = json!({
            "activeSignature": 0,
            "signatures": [{
                "label": "def cumsum(input: Tensor, dim: int) -> Tensor",
                "documentation": {
                    "kind": "markdown",
                    "value": "Return the cumulative sum."
                },
                "parameters": [{
                    "label": "input",
                    "documentation": "the input tensor"
                }]
            }]
        });

        let item = signature_help_item_from_lsp(&response, &call).expect("signature hover");

        assert_eq!(item.kind, "ty signature");
        assert_eq!(item.title, "def cumsum(input: Tensor, dim: int) -> Tensor");
        assert!(
            item.detail
                .as_deref()
                .is_some_and(|value| value.contains("the input tensor"))
        );
    }

    #[test]
    fn automatic_python_analysis_skips_large_buffers() {
        let large_source = "x = 1\n".repeat(30_000);

        assert!(should_run_automatic_python_analysis("x = 1\n"));
        assert!(!should_run_automatic_python_analysis(&large_source));
    }

    #[test]
    fn codex_turn_request_model_overrides_detected_default() {
        assert_eq!(
            codex_model_for_request(Some("gpt-5.5"), Some("gpt-5.2")),
            Some("gpt-5.5".into())
        );
        assert_eq!(
            codex_model_for_request(Some("  "), Some("gpt-5.2")),
            Some("gpt-5.2".into())
        );
        assert_eq!(codex_model_for_request(None, None), None);
    }

    #[test]
    fn codex_request_params_include_selected_model() {
        let mut params = json!({});
        apply_codex_model_param(&mut params, Some("gpt-5.5"), Some("gpt-5.2"));

        assert_eq!(params["model"], json!("gpt-5.5"));
    }

    #[test]
    fn gemini_acp_args_include_selected_model() {
        assert_eq!(
            gemini_acp_args(Some("gemini-2.5-pro")),
            vec![
                "--model",
                "gemini-2.5-pro",
                "--approval-mode",
                "default",
                "--acp"
            ]
        );
        assert_eq!(
            gemini_acp_args(Some("  ")),
            vec!["--approval-mode", "default", "--acp"]
        );
        assert_eq!(
            gemini_acp_args(None),
            vec!["--approval-mode", "default", "--acp"]
        );
    }

    #[test]
    fn cli_agent_model_args_are_inserted_per_agent() {
        let claude_args = vec!["-p".to_string(), "{prompt}".to_string()];
        assert_eq!(
            agent_args_with_selected_model("claude", &claude_args, Some("sonnet"), None),
            vec![
                "--permission-mode",
                "acceptEdits",
                "--model",
                "sonnet",
                "-p",
                "{prompt}"
            ]
        );

        let kilo_args = vec![
            "run".to_string(),
            "--auto".to_string(),
            "{prompt}".to_string(),
        ];
        assert_eq!(
            agent_args_with_selected_model("kilo", &kilo_args, Some("openai/gpt-5.5"), None),
            vec!["run", "--model", "openai/gpt-5.5", "--auto", "{prompt}"]
        );

        assert_eq!(
            agent_args_with_selected_model("claude", &claude_args, Some("  "), None),
            vec!["--permission-mode", "acceptEdits", "-p", "{prompt}"]
        );
    }

    #[test]
    fn cli_agent_permission_level_args_are_inserted_per_agent() {
        let claude_args = vec!["-p".to_string(), "{prompt}".to_string()];
        assert_eq!(
            agent_args_with_selected_model("claude", &claude_args, Some("sonnet"), Some("plan")),
            vec![
                "--permission-mode",
                "plan",
                "--model",
                "sonnet",
                "-p",
                "{prompt}"
            ]
        );

        let gemini_args = vec!["-p".to_string(), "{prompt}".to_string()];
        assert_eq!(
            agent_args_with_selected_model(
                "gemini",
                &gemini_args,
                Some("gemini-2.5-pro"),
                Some("autoEdits")
            ),
            vec![
                "--model",
                "gemini-2.5-pro",
                "--approval-mode",
                "auto_edit",
                "-p",
                "{prompt}"
            ]
        );

        let kilo_args = vec!["run".to_string(), "{prompt}".to_string()];
        assert_eq!(
            agent_args_with_selected_model(
                "kilo",
                &kilo_args,
                Some("openai/gpt-5.5"),
                Some("fullAuto")
            ),
            vec!["run", "--model", "openai/gpt-5.5", "--auto", "{prompt}"]
        );
    }

    #[test]
    fn codeshare_sessions_create_invites_and_track_local_participants() {
        let mut state = CodeShareState::default();
        let session = start_codeshare_session_in_state(
            &mut state,
            CodeShareSessionRequest {
                title: "Pairing on approval UI".into(),
                root: r"C:\workspace\hematite".into(),
                active_file: Some(r"C:\workspace\hematite\src\App.tsx".into()),
                agent_label: "OpenAI Codex".into(),
                permission_level: "plan".into(),
            },
            1_700_000_000,
        );

        assert_eq!(session.title, "Pairing on approval UI");
        assert_eq!(session.status, "hosting");
        assert_eq!(session.participant_count, 1);
        assert!(session.session_id.starts_with("codeshare-"));
        assert!(session.invite_code.starts_with("CS-"));
        assert_eq!(
            session.invite_link,
            format!("hematite://codeshare/{}", session.invite_code)
        );

        let joined = join_codeshare_session_in_state(
            &mut state,
            CodeShareJoinRequest {
                invite_code: session.invite_code.clone(),
            },
            1_700_000_005,
        )
        .expect("invite should resolve");

        assert_eq!(joined.session_id, session.session_id);
        assert_eq!(joined.status, "joined");
        assert_eq!(joined.participant_count, 2);
        assert_eq!(joined.updated_at, 1_700_000_005);
    }

    #[test]
    fn codeshare_join_rejects_unknown_invite_codes() {
        let mut state = CodeShareState::default();
        let error = join_codeshare_session_in_state(
            &mut state,
            CodeShareJoinRequest {
                invite_code: "CS-MISSING".into(),
            },
            1_700_000_000,
        )
        .expect_err("unknown invites should fail");

        assert!(error.contains("CodeShare invite was not found"));
    }

    #[test]
    fn python_tooling_actions_match_vscode_style_commands() {
        let path = Path::new(r"C:\workspace\pkg\app.py");

        let format = python_tooling_command_for_action(PythonToolingAction::Format, path);
        assert_eq!(format.binary, "ruff");
        assert_eq!(format.args, vec!["format", r"C:\workspace\pkg\app.py"]);

        let fix_all = python_tooling_command_for_action(PythonToolingAction::FixAll, path);
        assert_eq!(fix_all.binary, "ruff");
        assert_eq!(
            fix_all.args,
            vec!["check", "--fix", "--exit-zero", r"C:\workspace\pkg\app.py"]
        );

        let type_check = python_tooling_command_for_action(PythonToolingAction::TypeCheck, path);
        assert_eq!(type_check.binary, "ty");
        assert_eq!(type_check.args, vec!["check", r"C:\workspace\pkg\app.py"]);
    }

    #[test]
    fn rust_tool_status_specs_include_core_ide_quality_and_debug_tools() {
        let ids = rust_tool_status_specs()
            .iter()
            .map(|spec| spec.id)
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec![
                "rustup",
                "rustc",
                "cargo",
                "rustfmt",
                "cargo-clippy",
                "rust-analyzer",
                "lldb",
                "codelldb",
                "wasm-pack",
                "cargo-nextest",
                "cargo-watch",
                "cargo-audit",
                "cargo-deny",
                "cargo-expand",
                "cargo-llvm-cov",
            ]
        );
    }

    #[test]
    fn rustup_shimmed_rust_components_are_version_checked() {
        assert!(requires_version_probe("rust-analyzer"));
        assert!(requires_version_probe("rustfmt"));
        assert!(requires_version_probe("cargo-clippy"));
        assert!(!requires_version_probe("cargo"));
        assert!(!requires_version_probe("rustup"));
    }

    #[test]
    fn rust_tooling_actions_match_full_ide_commands() {
        let path = Path::new(r"C:\workspace\src\lib.rs");

        let check = rust_tooling_command_for_action(RustToolingAction::Check, path);
        assert_eq!(check.binary, "cargo");
        assert_eq!(check.args, vec!["check", "--message-format=json"]);
        assert!(check.parses_diagnostics);

        let clippy = rust_tooling_command_for_action(RustToolingAction::Clippy, path);
        assert_eq!(clippy.binary, "cargo");
        assert_eq!(clippy.args, vec!["clippy", "--message-format=json"]);
        assert!(clippy.parses_diagnostics);

        let format = rust_tooling_command_for_action(RustToolingAction::Format, path);
        assert_eq!(format.binary, "rustfmt");
        assert_eq!(format.args, vec![r"C:\workspace\src\lib.rs"]);
        assert!(!format.parses_diagnostics);

        let test = rust_tooling_command_for_action(RustToolingAction::Test, path);
        assert_eq!(test.binary, "cargo");
        assert_eq!(test.args, vec!["test", "--message-format=json"]);

        let doc = rust_tooling_command_for_action(RustToolingAction::Doc, path);
        assert_eq!(doc.binary, "cargo");
        assert_eq!(doc.args, vec!["doc", "--no-deps"]);

        let metadata = rust_tooling_command_for_action(RustToolingAction::Metadata, path);
        assert_eq!(metadata.binary, "cargo");
        assert_eq!(
            metadata.args,
            vec!["metadata", "--no-deps", "--format-version", "1"]
        );
    }

    #[test]
    fn cargo_json_diagnostics_are_filtered_to_active_rust_file() {
        let root = Path::new(r"C:\workspace");
        let file = Path::new(r"C:\workspace\src\main.rs");
        let source = "fn main() {\n    let value = nope;\n}\n";
        let stdout = r#"{"reason":"compiler-message","message":{"level":"error","message":"cannot find value `nope` in this scope","code":{"code":"E0425"},"spans":[{"file_name":"src/main.rs","line_start":2,"line_end":2,"column_start":17,"column_end":21,"is_primary":true}]}}"#;

        let diagnostics = parse_rust_tooling_diagnostics(stdout, root, file, source);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].module, "E0425");
        assert_eq!(diagnostics[0].severity, "error");
        assert_eq!(diagnostics[0].line, 2);
        assert_eq!(diagnostics[0].column, 17);
        assert_eq!(
            diagnostics[0].message,
            "cannot find value `nope` in this scope"
        );
    }

    #[test]
    fn rust_analyzer_publish_diagnostics_are_stored_by_uri() {
        let shared = Arc::new(Mutex::new(RustAnalyzerLspSharedState::new(
            r"C:\workspace".into(),
        )));
        let message = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": "file:///C:/workspace/src/main.rs",
                "version": 7,
                "diagnostics": [{
                    "range": {
                        "start": { "line": 1, "character": 16 },
                        "end": { "line": 1, "character": 20 }
                    },
                    "severity": 1,
                    "code": "E0425",
                    "source": "rustc",
                    "message": "cannot find value `nope` in this scope"
                }]
            }
        });

        handle_rust_analyzer_published_diagnostics(&shared, &message);

        let state = shared.lock().expect("state");
        let published = state
            .published_diagnostics
            .get("file:///C:/workspace/src/main.rs")
            .expect("published diagnostics");
        assert_eq!(published.version, Some(7));
        assert_eq!(published.diagnostics.len(), 1);
    }

    #[test]
    fn rust_analyzer_hover_items_are_labeled_as_rust_support() {
        let hover = json!({
            "contents": {
                "kind": "markdown",
                "value": "```rust\nfn compute(value: u32) -> u32\n```\n\nReturns the next value."
            },
            "range": {
                "start": { "line": 6, "character": 4 },
                "end": { "line": 6, "character": 11 }
            }
        });

        let item = hover_item_from_lsp_with_provider(
            &hover,
            7,
            5,
            "rust-analyzer",
            "Provided by rust-analyzer",
        )
        .expect("hover item");

        assert_eq!(item.kind, "rust-analyzer");
        assert_eq!(item.title, "fn compute(value: u32) -> u32");
        assert_eq!(item.detail.as_deref(), Some("Returns the next value."));
        assert_eq!(item.source.as_deref(), Some("Provided by rust-analyzer"));
    }

    #[test]
    fn rust_hover_parser_promotes_signature_over_crate_context() {
        let hover = json!({
            "contents": {
                "kind": "markdown",
                "value": "```rust\nhematite_hover_test\n```\n\n```rust\npub fn add(value: u32) -> u32\n```"
            },
            "range": {
                "start": { "line": 0, "character": 7 },
                "end": { "line": 0, "character": 10 }
            }
        });

        let item = hover_item_from_lsp_with_provider(
            &hover,
            1,
            8,
            "rust-analyzer",
            "Provided by rust-analyzer",
        )
        .expect("hover item");

        assert_eq!(item.title, "pub fn add(value: u32) -> u32");
        assert_eq!(item.detail.as_deref(), Some("hematite_hover_test"));
    }

    #[test]
    fn rust_analyzer_hover_works_for_open_cargo_document() {
        if probe_available_command("rust-analyzer").is_none()
            || probe_available_command("cargo").is_none()
        {
            return;
        }

        let root = env::temp_dir().join(format!("hematite-rust-hover-test-{}", std::process::id()));
        let source_dir = root.join("src");
        let file_path = source_dir.join("lib.rs");
        let source = "pub fn add(value: u32) -> u32 { value + 1 }\n";

        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&source_dir).expect("create temp src");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"hematite_hover_test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("write cargo manifest");
        fs::write(&file_path, source).expect("write source");

        let request = EditorHoverRequest {
            root: path_to_string(&root),
            file_path: path_to_string(&file_path),
            source: source.into(),
            line: 1,
            column: 8,
        };

        let mut item = request_rust_analyzer_hover(&request)
            .expect("rust-analyzer hover request")
            .expect("hover item");
        for _ in 0..5 {
            if item.kind == "rust-analyzer" {
                break;
            }
            thread::sleep(Duration::from_millis(250));
            item = request_rust_analyzer_hover(&request)
                .expect("rust-analyzer hover request")
                .expect("hover item");
        }

        assert_eq!(item.kind, "rust-analyzer");
        assert!(item.title.contains("add"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn standalone_rust_hover_handles_keywords_functions_and_attributes() {
        let source = "#[tokio::main]\n\nasync fn main() {\n  \n}\n";

        let keyword = rust_tree_sitter_hover(source, 3, 2).expect("async hover");
        assert_eq!(keyword.title, "Rust keyword `async`");

        let function = rust_tree_sitter_hover(source, 3, 11).expect("main hover");
        assert_eq!(function.kind, "rust function");
        assert_eq!(function.title, "async fn main()");

        let attribute = rust_tree_sitter_hover(source, 1, 4).expect("attribute hover");
        assert_eq!(attribute.kind, "rust attribute");
        assert_eq!(attribute.title, "#[tokio::main]");
    }

    #[test]
    fn rust_hover_falls_back_for_standalone_file_without_cargo_manifest() {
        let root = env::temp_dir().join(format!(
            "hematite-rust-standalone-hover-test-{}",
            std::process::id()
        ));
        let file_path = root.join("lib.rs");
        let source = "#[tokio::main]\n\nasync fn main() {\n  \n}\n";

        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(&file_path, source).expect("write source");

        let request = EditorHoverRequest {
            root: path_to_string(&root),
            file_path: path_to_string(&file_path),
            source: source.into(),
            line: 3,
            column: 11,
        };

        let item = request_rust_analyzer_hover(&request)
            .expect("hover request")
            .expect("fallback hover item");

        assert_eq!(item.kind, "rust function");
        assert_eq!(item.title, "async fn main()");
        assert_eq!(
            item.source.as_deref(),
            Some("Provided by Hematite Rust parser")
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn standalone_rust_semantics_use_parser_without_cargo_manifest() {
        let root = env::temp_dir().join(format!(
            "hematite-rust-standalone-semantics-test-{}",
            std::process::id()
        ));
        let file_path = root.join("lib.rs");
        let source = "#[tokio::main]\n\nasync fn main() {\n  let value: u32 = 42;\n}\n";

        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(&file_path, source).expect("write source");

        let payload = analyze_editor_semantics_for_path(&file_path, source);
        let kinds = payload
            .tokens
            .iter()
            .map(|token| token.kind.as_str())
            .collect::<BTreeSet<_>>();

        assert!(kinds.contains("attribute"));
        assert!(kinds.contains("keyword"));
        assert!(kinds.contains("functionDefinition"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rust_environment_reports_standalone_mode_without_cargo_manifest() {
        let root = env::temp_dir().join(format!(
            "hematite-rust-standalone-status-test-{}",
            std::process::id()
        ));

        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temp root");

        let status = inspect_rust_environment_sync(&path_to_string(&root)).expect("rust status");

        assert!(!status.cargo_toml_exists);
        assert!(status.summary.contains("standalone Rust parser"));
        assert_eq!(status.recommended_command, "cargo init");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn c_cpp_cuda_language_ids_are_first_class() {
        assert_eq!(language_id_from_path(Path::new("main.c")), "c");
        assert_eq!(language_id_from_path(Path::new("lib.hpp")), "cpp");
        assert_eq!(language_id_from_path(Path::new("kernel.cu")), "cuda-cpp");
        assert!(matches!(
            parser_language_for_path(Path::new("kernel.cuh")),
            Some(SourceLanguage::Cuda)
        ));
    }

    #[test]
    fn c_family_outline_extracts_functions_types_and_cuda_kernels() {
        let source = r#"
__global__ void add_kernel(float* out) {
  out[0] = 1.0f;
}

class Solver {
 public:
  void run();
};

int helper(int value) {
  return value + 1;
}
"#;

        let symbols = parse_symbols_for_path(Path::new("kernel.cu"), source);
        let labels = symbols
            .iter()
            .map(|symbol| (symbol.kind.as_str(), symbol.label.as_str()))
            .collect::<Vec<_>>();

        assert!(labels.contains(&("function", "add_kernel")));
        assert!(labels.contains(&("class", "Solver")));
        assert!(labels.contains(&("function", "helper")));
    }

    #[test]
    fn c_family_semantics_cover_c_cpp_and_cuda_without_clangd() {
        let source = r#"
#include <stdio.h>
#define SCALE 2

__global__ void add_kernel(float* out) {
  int value = SCALE;
  out[0] = value;
}

class Solver {
 public:
  void run();
};
"#;

        let payload = analyze_editor_semantics_for_path(Path::new("kernel.cu"), source);
        let kinds = payload
            .tokens
            .iter()
            .map(|token| token.kind.as_str())
            .collect::<BTreeSet<_>>();

        assert!(kinds.contains("macro"));
        assert!(kinds.contains("keyword"));
        assert!(kinds.contains("builtinType"));
        assert!(kinds.contains("functionDefinition"));
        assert!(kinds.contains("class"));
    }

    #[test]
    fn c_family_hover_handles_cuda_kernels_and_cpp_types() {
        let source = r#"
__global__ void add_kernel(float* out) {
  out[0] = 1.0f;
}

class Solver {
 public:
  void run();
};
"#;

        let kernel = c_family_tree_sitter_hover(source, SourceLanguage::Cuda, 2, 18)
            .expect("CUDA kernel hover");
        assert_eq!(kernel.kind, "cuda kernel");
        assert!(kernel.title.contains("add_kernel"));

        let class =
            c_family_tree_sitter_hover(source, SourceLanguage::Cpp, 6, 7).expect("class hover");
        assert_eq!(class.kind, "cpp class");
        assert_eq!(class.title, "class Solver");
    }

    #[test]
    fn c_family_environment_reports_parser_fallback_and_project_metadata() {
        let root = env::temp_dir().join(format!(
            "hematite-c-family-status-test-{}",
            std::process::id()
        ));

        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temp root");

        let fallback =
            inspect_c_family_environment_sync(&path_to_string(&root)).expect("c-family status");
        assert!(!fallback.compile_commands_exists);
        assert!(!fallback.cmake_lists_exists);
        assert!(fallback.summary.contains("standalone C-family parser"));

        fs::write(
            root.join("CMakeLists.txt"),
            "cmake_minimum_required(VERSION 3.20)\nproject(sample LANGUAGES C CXX CUDA)\n",
        )
        .expect("write cmake");
        fs::write(root.join("compile_commands.json"), "[]\n").expect("write compile commands");

        let project =
            inspect_c_family_environment_sync(&path_to_string(&root)).expect("c-family status");
        assert!(project.compile_commands_exists);
        assert!(project.cmake_lists_exists);
        assert_eq!(project.recommended_command, "clangd");

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_verbatim_paths_are_standard_file_uris() {
        assert_eq!(
            path_string_to_file_uri(r"\\?\C:\Users\ss ch\project\main.py"),
            "file:///C:/Users/ss%20ch/project/main.py"
        );
        assert_eq!(
            path_string_to_file_uri(r"\\?\UNC\server\share\main.py"),
            "file://server/share/main.py"
        );
    }
}
