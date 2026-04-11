use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{mpsc, Arc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};
use tauri::{
    menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu},
    Emitter, Manager,
};
use tree_sitter::{Node, Parser};
use walkdir::WalkDir;

const MAX_EDITOR_FILE_BYTES: u64 = 512 * 1024;
const MAX_CONTEXT_CHARS: usize = 4_200;
const CONTEXT_FILE_LIMIT: usize = 8;
const TERMINAL_CWD_MARKER: &str = "__HEMATITE_CWD__=";
const PYTHON_INSTALL_FAILURE_COOLDOWN: Duration = Duration::from_secs(45);
const UI_STATE_FILE_NAME: &str = "ui-state.json";
const LEGACY_UI_STATE_IDENTIFIERS: &[&str] = &["com.entity_27th.hematite"];
#[cfg(target_os = "windows")]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

static PYTHON_INSTALL_FAILURES: OnceLock<Mutex<BTreeMap<String, Instant>>> = OnceLock::new();
static PYTHON_INSTALL_IN_PROGRESS: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
static CODEX_APP_SERVER: OnceLock<Mutex<CodexAppServerState>> = OnceLock::new();
static GEMINI_ACP: OnceLock<Mutex<GeminiAcpState>> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
enum SourceLanguage {
    Python,
    Rust,
    JavaScript,
    TypeScript,
    Tsx,
}

#[derive(Clone, Debug)]
struct ImportCandidate {
    module: String,
    from: usize,
    to: usize,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PythonImportRequest {
    root: String,
    file_path: String,
    source: String,
    auto_install: bool,
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
struct ProcessOutcome {
    success: bool,
    command: String,
    stdout: String,
    stderr: String,
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
    active_turn_id: Option<String>,
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
            active_turn_id: None,
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
    current_session_id: Option<String>,
    prompt_in_progress: bool,
    pending_responses: BTreeMap<String, mpsc::Sender<Result<Value, String>>>,
    pending_requests: BTreeMap<String, GeminiPendingRequest>,
}

impl GeminiSharedState {
    fn new(root: String) -> Self {
        Self {
            next_request_id: 1,
            initialized: false,
            current_root: root,
            current_session_id: None,
            prompt_in_progress: false,
            pending_responses: BTreeMap::new(),
            pending_requests: BTreeMap::new(),
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
fn refresh_tool_statuses() -> Result<Vec<ToolStatus>, String> {
    Ok(vec![
        make_tool_status("uv", "astral-uv"),
        make_tool_status("python", "Python"),
        make_tool_status("codex", "OpenAI Codex"),
        make_tool_status("gemini", "Gemini CLI"),
        make_tool_status("claude", "Claude Code"),
    ])
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
            push_unique_path(
                &mut paths,
                root.join(identifier).join(UI_STATE_FILE_NAME),
            );
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
fn extract_symbols(path: String, content: String) -> Result<Vec<SymbolEntry>, String> {
    let path_buf = PathBuf::from(path);
    Ok(parse_symbols_for_path(&path_buf, &content))
}

#[tauri::command]
fn analyze_editor_semantics(path: String, content: String) -> Result<EditorSemanticsPayload, String> {
    let path_buf = PathBuf::from(path);
    Ok(analyze_editor_semantics_for_path(&path_buf, &content))
}

#[tauri::command]
fn build_compact_context(request: CompactContextRequest) -> Result<CompactContextPayload, String> {
    let root = PathBuf::from(&request.root);
    let current_file = request.current_file.as_ref().map(PathBuf::from);
    let context =
        compose_compact_context(&root, current_file.as_ref(), request.content.as_deref())?;

    Ok(CompactContextPayload { context })
}

#[tauri::command]
fn refresh_agent_health() -> Result<AgentHealthPayload, String> {
    Ok(build_agent_health_payload())
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
    })
}

#[tauri::command]
fn execute_terminal_command(
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

    #[cfg(target_os = "windows")]
    let mut command = {
        let inline = format!(
            "$ErrorActionPreference = 'Continue'; \
             try {{ Set-Location -LiteralPath {cwd}; }} catch {{ Write-Error $_; Write-Output ('{marker}' + {cwd}); exit 1 }}; \
             $global:LASTEXITCODE = $null; \
             try {{ Invoke-Expression {input}; }} catch {{ Write-Error $_; }}; \
             $exitCode = if (($LASTEXITCODE -as [int]) -ne $null) {{ [int]$LASTEXITCODE }} elseif ($?) {{ 0 }} else {{ 1 }}; \
             Write-Output ('{marker}' + (Get-Location).Path); \
             exit $exitCode",
            cwd = powershell_quote(&path_to_string(&resolved_cwd)),
            input = powershell_quote(command_text),
            marker = TERMINAL_CWD_MARKER,
        );

        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &inline,
        ]);
        hide_background_window(&mut command);
        command
    };

    #[cfg(not(target_os = "windows"))]
    let mut command = {
        let inline = format!(
            "cd {cwd} && {{ {input}; }}; status=$?; printf '%s%s\\n' '{marker}' \"$PWD\"; exit $status",
            cwd = shell_quote(&path_to_string(&resolved_cwd)),
            input = command_text,
            marker = TERMINAL_CWD_MARKER,
        );

        let mut command = Command::new("sh");
        command.args(["-lc", &inline]);
        command
    };

    command.current_dir(&resolved_cwd);
    apply_agent_env(&mut command, &stored);
    apply_workspace_env(&mut command, &resolved_cwd);
    hide_background_window(&mut command);

    let output = command.output().map_err(|err| err.to_string())?;
    let (stdout, cwd) =
        split_terminal_output(&String::from_utf8_lossy(&output.stdout), &resolved_cwd);

    Ok(TerminalCommandResponse {
        success: output.status.success(),
        command: command_text.to_string(),
        stdout,
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        cwd,
    })
}

#[tauri::command]
fn run_agent(request: AgentRunRequest) -> Result<AgentRunResponse, String> {
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

    let resolved_args = request
        .args
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
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        context,
    })
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
    let thread_id = ensure_codex_thread(session, &root_string)?;
    let turn_response = codex_send_request(
        session,
        "turn/start",
        json!({
            "threadId": thread_id,
            "cwd": root_string,
            "approvalPolicy": "on-request",
            "input": [
                {
                    "type": "text",
                    "text": prompt,
                    "text_elements": [],
                }
            ],
        }),
        Duration::from_secs(20),
    )?;

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
    let state = gemini_acp_state();
    let mut bridge = state
        .lock()
        .map_err(|_| "Gemini bridge lock was poisoned.".to_string())?;
    let session = ensure_gemini_acp_session(&mut bridge, &app, &root_string)?;

    ensure_gemini_initialized(session)?;
    let session_id = ensure_gemini_chat_session(session, &root_string)?;

    if let Ok(mut shared) = session.shared.lock() {
        shared.prompt_in_progress = true;
    }

    let response = gemini_send_request(
        session,
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

    if let Ok(mut shared) = session.shared.lock() {
        shared.prompt_in_progress = false;
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
                return Err("Wait for the current Gemini turn to finish before starting a new chat.".into());
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

#[tauri::command]
fn analyze_python_imports(request: PythonImportRequest) -> Result<PythonImportResponse, String> {
    resolve_python_imports(request, false)
}

#[tauri::command]
fn install_missing_python_imports(
    request: PythonImportRequest,
) -> Result<PythonImportResponse, String> {
    resolve_python_imports(request, true)
}

fn resolve_python_imports(
    request: PythonImportRequest,
    force_install: bool,
) -> Result<PythonImportResponse, String> {
    let root = PathBuf::from(&request.root);
    let _current_file = PathBuf::from(&request.file_path);
    let uv_path = match probe_command("uv") {
        Some(path) => path,
        None => {
            return Ok(PythonImportResponse {
                environment_ready: false,
                environment_path: None,
                diagnostics: vec![EditorDiagnostic {
                    module: "uv".into(),
                    from: 0,
                    to: 0,
                    line: 0,
                    column: 0,
                    severity: "warning".into(),
                    message: "astral-uv is not available on PATH, so automatic Python dependency management is paused.".into(),
                }],
                events: Vec::new(),
            })
        }
    };

    let candidates = collect_python_imports(&request.source)?;
    if candidates.is_empty() {
        return Ok(PythonImportResponse {
            environment_ready: true,
            environment_path: Some(path_to_string(&venv_python_path(&root))),
            diagnostics: Vec::new(),
            events: Vec::new(),
        });
    }

    ensure_python_environment(&root, &uv_path)?;

    let mut diagnostics = Vec::new();
    let mut events = Vec::new();
    let mut seen = BTreeSet::new();

    for candidate in candidates {
        if !seen.insert(candidate.module.clone()) {
            continue;
        }

        if is_local_python_module(&root, &candidate.module) {
            continue;
        }

        if python_module_exists(&root, &candidate.module)? {
            continue;
        }

        let package = python_package_name(&candidate.module);
        if request.auto_install {
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
                diagnostics.push(EditorDiagnostic {
                    module: candidate.module.clone(),
                    from: candidate.from,
                    to: candidate.to,
                    line: candidate.line,
                    column: candidate.column,
                    severity: "info".into(),
                    message: format!(
                        "Import `{}` is waiting for an in-progress uv installation to finish.",
                        candidate.module
                    ),
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
                    diagnostics.push(EditorDiagnostic {
                        module: candidate.module.clone(),
                        from: candidate.from,
                        to: candidate.to,
                        line: candidate.line,
                        column: candidate.column,
                        severity: "warning".into(),
                        message: format!(
                            "Import `{}` is still unresolved. Hematite is holding the last failed install on cooldown before retrying.",
                            candidate.module
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

                    if resolved {
                        continue;
                    }
                }
                Err(output) => {
                    mark_python_install_failed(&install_key);
                    events.push(PythonImportEvent {
                        module: candidate.module.clone(),
                        package: package.clone(),
                        success: false,
                        state: "failed".into(),
                        command: install_command_preview(&root, &package),
                        output,
                    });
                    clear_python_install_started(&install_key);
                }
            }
        }

        diagnostics.push(EditorDiagnostic {
            module: candidate.module.clone(),
            from: candidate.from,
            to: candidate.to,
            line: candidate.line,
            column: candidate.column,
            severity: "error".into(),
            message: format!(
                "Import `{}` could not be resolved in the project virtual environment.",
                candidate.module
            ),
        });
    }

    Ok(PythonImportResponse {
        environment_ready: true,
        environment_path: Some(path_to_string(&venv_python_path(&root))),
        diagnostics,
        events,
    })
}

fn build_agent_health_payload() -> AgentHealthPayload {
    let stored = load_agent_credentials();
    AgentHealthPayload {
        agents: vec![
            codex_status(&stored),
            gemini_status(&stored),
            claude_status(&stored),
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
    let output = prepare_cli_command("codex", &auth_args).command.output();
    match output {
        Ok(result) if result.status.success() => AgentStatus {
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
        Ok(result) => AgentStatus {
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
        Err(_) => AgentStatus {
            id: "codex".into(),
            label: "OpenAI Codex".into(),
            available: true,
            resolved_path,
            auth_state: "missing".into(),
            auth_source: None,
            summary: "Codex CLI is installed, but no login or API key was detected.".into(),
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
                    format!("Claude Code reports that you are logged in via {}.", provider)
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

fn read_claude_auth_status(stored: &AgentCredentials) -> Option<ClaudeAuthStatusPayload> {
    let args = vec!["auth".to_string(), "status".to_string()];
    let mut prepared = prepare_cli_command("claude", &args);
    apply_agent_env(&mut prepared.command, stored);

    let output = prepared.command.output().ok()?;
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
    spawn_codex_stderr_reader(stderr);

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
        for line in reader.lines() {
            let Ok(line) = line else {
                emit_codex_frontend_event(
                    &app,
                    CodexFrontendEvent::Error {
                        message: "Lost the Codex event stream.".into(),
                    },
                );
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
    });
}

fn spawn_codex_stderr_reader(stderr: impl std::io::Read + Send + 'static) {
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if line.is_err() {
                break;
            }
        }
    });
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
            let _ = sender.send(Err(error));
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
            emit_codex_frontend_event(
                app,
                CodexFrontendEvent::Error {
                    message: params
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("Codex reported an unknown background error.")
                        .to_string(),
                },
            );
        }
        _ => {}
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

fn ensure_codex_thread(session: &CodexAppServerSession, root: &str) -> Result<String, String> {
    if let Some(thread_id) = session
        .shared
        .lock()
        .map_err(|_| "Codex shared state lock was poisoned.".to_string())?
        .current_thread_id
        .clone()
    {
        return Ok(thread_id);
    }

    let response = codex_send_request(
        session,
        "thread/start",
        json!({
            "cwd": root,
            "approvalPolicy": "on-request",
            "approvalsReviewer": "user",
            "sandbox": "workspace-write",
            "ephemeral": false,
            "experimentalRawEvents": false,
            "persistExtendedHistory": true,
            "serviceName": "Hematite",
        }),
        Duration::from_secs(15),
    )?;

    let thread_id = response
        .get("thread")
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| "Codex app-server did not return a thread id.".to_string())?
        .to_string();

    if let Ok(mut shared) = session.shared.lock() {
        shared.current_thread_id = Some(thread_id.clone());
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
            ))
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
) -> Result<&'a mut GeminiAcpSession, String> {
    let mut needs_restart = bridge.session.is_none();

    if let Some(session) = bridge.session.as_mut() {
        let exited = session
            .child
            .try_wait()
            .map_err(|err| format!("Could not inspect Gemini background process. {}", err))?
            .is_some();
        let current_root = session
            .shared
            .lock()
            .map_err(|_| "Gemini shared state lock was poisoned.".to_string())?
            .current_root
            .clone();

        needs_restart = exited || current_root != root;
    }

    if needs_restart {
        if let Some(session) = bridge.session.as_mut() {
            dispose_gemini_session(session);
        }
        bridge.session = Some(spawn_gemini_acp_session(app, root)?);
    }

    bridge
        .session
        .as_mut()
        .ok_or_else(|| "Gemini ACP session did not start.".to_string())
}

fn spawn_gemini_acp_session(app: &tauri::AppHandle, root: &str) -> Result<GeminiAcpSession, String> {
    let stored = load_agent_credentials();
    let args = vec!["--acp".to_string()];
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
    let shared = Arc::new(Mutex::new(GeminiSharedState::new(root.to_string())));

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
    let method = message.get("method").and_then(Value::as_str).map(str::to_string);
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
                    message: format!("Gemini requested `{}` which Hematite does not handle yet.", other),
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
    let (tx, rx) = mpsc::channel();
    let (request_id, message) = {
        let mut shared = session
            .shared
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

    if let Err(error) = send_gemini_json(&session.stdin, &message) {
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

fn venv_bin_dir(root: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        root.join(".venv").join("Scripts")
    } else {
        root.join(".venv").join("bin")
    }
}

fn make_tool_status(id: &str, label: &str) -> ToolStatus {
    let resolved_path = probe_command(id);
    ToolStatus {
        id: id.into(),
        label: label.into(),
        available: resolved_path.is_some(),
        resolved_path,
    }
}

fn probe_command(binary: &str) -> Option<String> {
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

#[cfg(target_os = "windows")]
fn probe_windows_command(binary: &str) -> Option<String> {
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
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
    {
        "py" => "python",
        "rs" => "rust",
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
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
    {
        "py" => Some(SourceLanguage::Python),
        "rs" => Some(SourceLanguage::Rust),
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

fn analyze_editor_semantics_for_path(path: &Path, content: &str) -> EditorSemanticsPayload {
    match parser_language_for_path(path) {
        Some(SourceLanguage::Python) => analyze_python_editor_semantics(content),
        _ => EditorSemanticsPayload::default(),
    }
}

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

    EditorSemanticsPayload { tokens, hover_items }
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
                        source: Some(format!("Defined in this file · line {}", span.start_line)),
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
                        source: Some(format!("Defined in this file · line {}", span.start_line)),
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
                        let object_name = function_node
                            .child_by_field_name("object")
                            .and_then(|object_node| {
                                if object_node.kind() == "identifier" {
                                    node_text(object_node, source)
                                } else {
                                    None
                                }
                            });

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

                        if let Some(attribute_node) = function_node.child_by_field_name("attribute") {
                            let span = span_from_node(attribute_node);
                            let attribute_name =
                                node_text(attribute_node, source).unwrap_or_default();
                            let token_kind = if let Some(definition) = definitions.get(&attribute_name)
                            {
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
                    let token_kind = if let Some(definition) = definitions.get(&attribute_name) {
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
        hover.kind, hover.title, span.start_line, span.start_column, span.end_line, span.end_column
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
                    if let Some(span) = identifier_span_on_line(line, line_index, part, module_search) {
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

                if let Some(span) = identifier_span_on_line(line, line_index, &alias, module_search) {
                    let alias_entry = PythonImportAlias {
                        alias: alias.clone(),
                        statement: statement.clone(),
                        span: span.clone(),
                        token_kind: "namespace".into(),
                        hover_kind: "Module".into(),
                    };
                    aliases.entry(alias.clone()).or_insert_with(|| alias_entry.clone());
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
                if let Some(span) = identifier_span_on_line(line, line_index, part, module_search) {
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
                if let Some(span) = identifier_span_on_line(line, line_index, &alias, search_start) {
                    let (token_kind, hover_kind) = import_symbol_kind(&alias);
                    let alias_entry = PythonImportAlias {
                        alias: alias.clone(),
                        statement: format!("from {} import {}", module, entry),
                        span: span.clone(),
                        token_kind: token_kind.into(),
                        hover_kind: hover_kind.into(),
                    };
                    aliases.entry(alias.clone()).or_insert_with(|| alias_entry.clone());
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
    if name.chars().next().is_some_and(|value| value.is_uppercase()) {
        ("classReference", "Imported class")
    } else {
        ("variable", "Imported symbol")
    }
}

fn python_callable_token_kind(name: &str) -> &'static str {
    if name.chars().next().is_some_and(|value| value.is_uppercase()) {
        "classReference"
    } else {
        "functionCall"
    }
}

fn python_attribute_token_kind(name: &str) -> &'static str {
    if name.chars().next().is_some_and(|value| value.is_uppercase()) {
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

fn prettify_symbol_kind(kind: &str) -> &'static str {
    match kind {
        "function_definition" | "function_declaration" | "function_item" => "function",
        "class_definition" | "class_declaration" => "class",
        "method_definition" => "method",
        "struct_item" => "struct",
        "enum_item" | "enum_declaration" => "enum",
        "trait_item" => "trait",
        "interface_declaration" => "interface",
        "impl_item" => "impl",
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
                    from: node.start_byte(),
                    to: node.end_byte(),
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

fn is_local_python_module(root: &Path, module: &str) -> bool {
    let needle = module.split('.').next().unwrap_or(module);
    let direct_file = root.join(format!("{needle}.py"));
    let package_dir = root.join(needle).join("__init__.py");
    if direct_file.exists() || package_dir.exists() {
        return true;
    }

    for entry in WalkDir::new(root)
        .max_depth(4)
        .into_iter()
        .filter_entry(|entry| !should_ignore_name(&entry.file_name().to_string_lossy()))
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if path
            .file_name()
            .map(|value| value.to_string_lossy() == format!("{needle}.py"))
            .unwrap_or(false)
        {
            return true;
        }

        if path
            .parent()
            .and_then(Path::file_name)
            .map(|value| value == needle)
            .unwrap_or(false)
            && path
                .file_name()
                .map(|value| value == "__init__.py")
                .unwrap_or(false)
        {
            return true;
        }
    }

    false
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
            "A lightweight desktop IDE with agent chat, tree-sitter context, and uv-backed Python management."
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
        "&New Agent Chat",
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
    let focus_chat = MenuItem::with_id(app, "view.focus_chat", "Show &Chat", true, Some("Alt+1"))?;
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
    let focus_outline = MenuItem::with_id(
        app,
        "view.focus_outline",
        "Show &Outline",
        true,
        Some("Alt+4"),
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
            build_compact_context,
            refresh_agent_health,
            save_agent_credentials,
            launch_agent_login,
            pick_workspace_directory,
            pick_service_account_file,
            inspect_python_environment,
            prepare_python_environment,
            execute_terminal_command,
            refresh_tool_statuses,
            run_agent,
            start_codex_turn,
            respond_to_codex_approval,
            reset_codex_session,
            start_gemini_turn,
            respond_to_gemini_approval,
            reset_gemini_session,
            analyze_python_imports,
            install_missing_python_imports
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_semantics_emits_tokens_and_hover_items() {
        let source = r#"
import torch.optim as optim

class Model:
    """Simple model wrapper."""

    def scan(self, value):
        optimizer = optim.AdamW(value)
        return optimizer
"#;

        let semantics = analyze_editor_semantics_for_path(Path::new("test.py"), source);
        let (imports, _) = collect_python_import_entries(source);
        assert!(
            semantics.tokens.iter().any(|token| token.kind == "namespace"),
            "expected module namespace token, imports={imports:?}, tokens={:?}",
            semantics
                .tokens
                .iter()
                .map(|token| (&token.kind, token.start_line, token.start_column, token.end_line, token.end_column))
                .collect::<Vec<_>>()
        );
        assert!(
            semantics
                .tokens
                .iter()
                .any(|token| matches!(token.kind.as_str(), "functionDefinition" | "methodDefinition")),
            "expected function or method definition token"
        );
        assert!(
            semantics
                .tokens
                .iter()
                .any(|token| matches!(token.kind.as_str(), "functionCall" | "classReference")),
            "expected callable reference token"
        );
        assert!(
            semantics
                .hover_items
                .iter()
                .any(|item| item.title.contains("scan") || item.title.contains("AdamW")),
            "expected hover metadata for local definitions or calls"
        );
    }

    #[test]
    fn python_semantics_distinguish_modules_classes_and_calls() {
        let source = r#"
import torch
from trl import SFTTrainer

value = torch.exp(data)
trainer = SFTTrainer(model)
"#;

        let semantics = analyze_editor_semantics_for_path(Path::new("test.py"), source);

        assert!(
            semantics
                .tokens
                .iter()
                .any(|token| token.kind == "namespace" && token.start_line == 2),
            "expected namespace token on import line"
        );
        assert!(
            semantics
                .tokens
                .iter()
                .any(|token| token.kind == "classReference" && token.start_line >= 3),
            "expected class reference token for imported class or constructor call"
        );
        assert!(
            semantics
                .tokens
                .iter()
                .any(|token| token.kind == "functionCall" && token.start_line == 5),
            "expected call token for imported function-like member"
        );
        assert!(
            semantics.hover_items.iter().any(|item| {
                item.title == "torch.exp" || item.title.contains("SFTTrainer")
            }),
            "expected imported member hover info"
        );
    }
}
