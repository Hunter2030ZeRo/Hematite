import type { Diagnostic } from "@codemirror/lint";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  For,
  Show,
  Suspense,
  createEffect,
  createMemo,
  createSignal,
  lazy,
  onCleanup,
  onMount,
} from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import "./App.css";

type ToolStatus = {
  id: string;
  label: string;
  available: boolean;
  resolvedPath?: string | null;
};

type FileEntry = {
  name: string;
  path: string;
  isDir: boolean;
  size?: number | null;
};

type DirectoryListing = {
  path: string;
  entries: FileEntry[];
};

type FileDocument = {
  path: string;
  name: string;
  language: string;
  content: string;
};

type SymbolEntry = {
  kind: string;
  label: string;
  startLine: number;
  endLine: number;
};

type EditorSemanticToken = {
  kind: string;
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
};

type EditorHoverItem = {
  kind: string;
  title: string;
  detail?: string | null;
  source?: string | null;
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
};

type EditorHoverRequest = {
  path: string;
  content: string;
  line: number;
  column: number;
};

type EditorSemanticsPayload = {
  tokens: EditorSemanticToken[];
  hoverItems: EditorHoverItem[];
};

type CompactContextPayload = {
  context: string;
};

type EditorDiagnostic = {
  module: string;
  from: number;
  to: number;
  line: number;
  column: number;
  severity: "error" | "warning" | "info" | string;
  message: string;
};

type CursorPosition = {
  line: number;
  column: number;
};

type PythonImportEvent = {
  module: string;
  package: string;
  success: boolean;
  state: string;
  command: string;
  output: string;
};

type PythonImportResponse = {
  environmentReady: boolean;
  environmentPath?: string | null;
  diagnostics: EditorDiagnostic[];
  events: PythonImportEvent[];
};

type CredentialSnapshot = {
  hasOpenaiApiKey: boolean;
  hasGeminiApiKey: boolean;
  hasGoogleApiKey: boolean;
  hasAnthropicApiKey: boolean;
  hasKiloApiKey: boolean;
  googleCloudProject?: string | null;
  googleCloudLocation?: string | null;
  googleApplicationCredentials?: string | null;
};

type AgentStatus = {
  id: string;
  label: string;
  available: boolean;
  resolvedPath?: string | null;
  authState: string;
  authSource?: string | null;
  summary: string;
  supportsOauth: boolean;
  supportsApiKey: boolean;
};

type AgentHealthPayload = {
  agents: AgentStatus[];
  credentials: CredentialSnapshot;
};

type PythonEnvironmentStatus = {
  root: string;
  uvAvailable: boolean;
  pyprojectExists: boolean;
  venvExists: boolean;
  pythonPath?: string | null;
  summary: string;
  recommendedCommand: string;
};

type RustEnvironmentStatus = {
  root: string;
  cargoTomlExists: boolean;
  rustToolchainFile?: string | null;
  activeToolchain?: string | null;
  installedToolchains: string[];
  installedComponents: string[];
  rustcVersion?: string | null;
  cargoVersion?: string | null;
  rustAnalyzerAvailable: boolean;
  rustfmtAvailable: boolean;
  clippyAvailable: boolean;
  lldbAvailable: boolean;
  codelldbAvailable: boolean;
  summary: string;
  recommendedCommand: string;
};

type CFamilyEnvironmentStatus = {
  root: string;
  cmakeListsExists: boolean;
  compileCommandsExists: boolean;
  clangdAvailable: boolean;
  clangAvailable: boolean;
  clangxxAvailable: boolean;
  gccAvailable: boolean;
  gxxAvailable: boolean;
  msvcClAvailable: boolean;
  cmakeAvailable: boolean;
  ninjaAvailable: boolean;
  makeAvailable: boolean;
  nvccAvailable: boolean;
  cudaGdbAvailable: boolean;
  lldbAvailable: boolean;
  codelldbAvailable: boolean;
  clangdVersion?: string | null;
  clangVersion?: string | null;
  gccVersion?: string | null;
  nvccVersion?: string | null;
  summary: string;
  recommendedCommand: string;
};

type ProcessOutcome = {
  success: boolean;
  command: string;
  stdout: string;
  stderr: string;
  diagnostics?: EditorDiagnostic[];
};

type PythonToolingAction = "check" | "fixAll" | "format" | "organizeImports" | "typeCheck";
type RustToolingAction = "check" | "clippy" | "format" | "test" | "build" | "doc" | "metadata";

type TerminalCommandResponse = {
  success: boolean;
  command: string;
  stdout: string;
  stderr: string;
  cwd: string;
};

type AgentRunResponse = {
  success: boolean;
  command: string[];
  prompt: string;
  stdout: string;
  stderr: string;
  context?: string | null;
};

type CodexTurnResponse = {
  threadId: string;
  turnId: string;
  prompt: string;
  context?: string | null;
};

type CodexPermissionSummary = {
  networkEnabled?: boolean | null;
  readRoots: string[];
  writeRoots: string[];
};

type GeminiToolLocation = {
  path: string;
  line?: number | null;
};

type ApprovalChoice = {
  id: string;
  label: string;
};

type ApprovalState = "pending" | "submitted" | "resolved";

type AgentApproval = {
  requestId: string;
  agentId: "codex" | "gemini";
  approvalType: "command" | "fileChange" | "permissions" | string;
  title?: string | null;
  reason?: string | null;
  command?: string | null;
  cwd?: string | null;
  grantRoot?: string | null;
  permissions?: CodexPermissionSummary | null;
  locations?: GeminiToolLocation[];
  choices: ApprovalChoice[];
  state: ApprovalState;
};

type CodexFrontendEvent =
  | {
      kind: "agentMessageDelta";
      turnId: string;
      itemId: string;
      delta: string;
    }
  | {
      kind: "agentMessageCompleted";
      turnId: string;
      itemId: string;
      text: string;
    }
  | {
      kind: "approvalRequested";
      requestId: string;
      approvalType: "command" | "fileChange" | "permissions" | string;
      turnId: string;
      itemId: string;
      reason?: string | null;
      command?: string | null;
      cwd?: string | null;
      grantRoot?: string | null;
      permissions?: CodexPermissionSummary | null;
      choices: ApprovalChoice[];
    }
  | {
      kind: "approvalResolved";
      requestId: string;
    }
  | {
      kind: "turnCompleted";
      turnId: string;
      success: boolean;
      error?: string | null;
    }
  | {
      kind: "error";
      message: string;
    };

type GeminiTurnResponse = {
  sessionId: string;
  prompt: string;
  context?: string | null;
  stopReason: string;
};

type GeminiFrontendEvent =
  | {
      kind: "agentMessageDelta";
      sessionId: string;
      delta: string;
    }
  | {
      kind: "approvalRequested";
      requestId: string;
      sessionId: string;
      title: string;
      toolKind?: string | null;
      command?: string | null;
      locations: GeminiToolLocation[];
      choices: ApprovalChoice[];
    }
  | {
      kind: "approvalResolved";
      requestId: string;
    }
  | {
      kind: "promptCompleted";
      sessionId: string;
      success: boolean;
      stopReason: string;
      error?: string | null;
    }
  | {
      kind: "error";
      message: string;
    };

type MenuEventPayload = {
  id: string;
};

type BootstrapPayload = {
  defaultRoot: string;
};

type PersistedDocumentSnapshot = {
  path: string;
  name: string;
  language: string;
  content: string;
};

type PersistedWorkspaceSnapshot = {
  activeTab: string | null;
  expandedDirectories: string[];
  directoryCache: Record<string, FileEntry[]>;
  activeDocumentSnapshot: PersistedDocumentSnapshot | null;
};

type PersistedAppState = {
  version: 1;
  lastWorkspace: string;
  workspaces: Record<string, PersistedWorkspaceSnapshot>;
};

type EditorDocument = FileDocument & {
  savedContent: string;
  dirty: boolean;
  diagnostics: EditorDiagnostic[];
  installEvents: PythonImportEvent[];
};

type DirectoryState = {
  entries: FileEntry[];
  loaded: boolean;
  loading: boolean;
  error?: string;
};

type TerminalEntry = {
  id: string;
  command: string;
  stdout: string;
  stderr: string;
  cwd: string;
  success: boolean;
  timestamp: string;
};

type ChatRole = "system" | "user" | "assistant";
type ChatStatus = "complete" | "running" | "error";

type ChatMessage = {
  id: string;
  role: ChatRole;
  content: string;
  timestamp: string;
  agentId?: string;
  agentLabel?: string;
  status: ChatStatus;
  command?: string;
  prompt?: string;
  stderr?: string;
  approval?: AgentApproval;
};

type UtilityTab = "chat" | "access" | "project" | "problems" | "outline";

type CredentialsForm = {
  openaiApiKey: string;
  geminiApiKey: string;
  googleApiKey: string;
  googleCloudProject: string;
  googleCloudLocation: string;
  googleApplicationCredentials: string;
  anthropicApiKey: string;
  kiloApiKey: string;
};

type AgentModelOption = {
  id: string;
  label: string;
};

type AgentDefinition = {
  id: string;
  label: string;
  binary: string;
  args: string[];
  description: string;
  promptMode?: "stdin" | "arg";
  models?: AgentModelOption[];
};

const LazyCodeEditor = lazy(() => import("./components/CodeEditor"));

const AGENTS: AgentDefinition[] = [
  {
    id: "codex",
    label: "OpenAI Codex",
    binary: "codex",
    args: ["exec", "-", "--skip-git-repo-check"],
    promptMode: "stdin",
    description: "Fast repo-aware Codex execution with compact workspace context.",
    models: [
      { id: "gpt-5.5", label: "GPT-5.5" },
      { id: "gpt-5.4", label: "GPT-5.4" },
      { id: "gpt-5.4-mini", label: "GPT-5.4 Mini" },
      { id: "gpt-5.3-codex", label: "GPT-5.3 Codex" },
      { id: "gpt-5.2", label: "GPT-5.2" },
    ],
  },
  {
    id: "gemini",
    label: "Gemini CLI",
    binary: "gemini",
    args: ["-p", "{prompt}"],
    description: "Gemini prompt mode with Google sign-in or API-key based access.",
    models: [
      { id: "", label: "Provider default" },
      { id: "gemini-2.5-pro", label: "Gemini 2.5 Pro" },
      { id: "gemini-2.5-flash", label: "Gemini 2.5 Flash" },
      { id: "gemini-2.0-flash", label: "Gemini 2.0 Flash" },
    ],
  },
  {
    id: "claude",
    label: "Claude Code",
    binary: "claude",
    args: ["-p", "{prompt}"],
    description: "Claude Code execution using the installed CLI and local credentials.",
    models: [
      { id: "", label: "Provider default" },
      { id: "opus", label: "Opus" },
      { id: "sonnet", label: "Sonnet" },
    ],
  },
  {
    id: "kilo",
    label: "Kilo Code",
    binary: "kilo",
    args: ["run", "--auto", "{prompt}"],
    description: "Kilo Code CLI agent flow with bundled binary fallback.",
    models: [
      { id: "", label: "Provider default" },
      { id: "openai/gpt-5.5", label: "OpenAI GPT-5.5" },
      { id: "openai/gpt-5.4", label: "OpenAI GPT-5.4" },
      { id: "anthropic/sonnet", label: "Claude Sonnet" },
      { id: "google/gemini-2.5-pro", label: "Gemini 2.5 Pro" },
    ],
  },
];

const DEFAULT_TOOLS: ToolStatus[] = [
  { id: "uv", label: "astral-uv", available: false, resolvedPath: null },
  { id: "python", label: "Python", available: false, resolvedPath: null },
  { id: "ruff", label: "Ruff", available: false, resolvedPath: null },
  { id: "ty", label: "ty", available: false, resolvedPath: null },
  { id: "codex", label: "OpenAI Codex", available: false, resolvedPath: null },
  { id: "gemini", label: "Gemini CLI", available: false, resolvedPath: null },
  { id: "claude", label: "Claude Code", available: false, resolvedPath: null },
  { id: "kilo", label: "Kilo Code", available: false, resolvedPath: null },
  { id: "rustup", label: "rustup", available: false, resolvedPath: null },
  { id: "rustc", label: "Rust compiler", available: false, resolvedPath: null },
  { id: "cargo", label: "Cargo", available: false, resolvedPath: null },
  { id: "rustfmt", label: "rustfmt", available: false, resolvedPath: null },
  { id: "cargo-clippy", label: "Clippy", available: false, resolvedPath: null },
  { id: "rust-analyzer", label: "rust-analyzer", available: false, resolvedPath: null },
  { id: "lldb", label: "LLDB", available: false, resolvedPath: null },
  { id: "codelldb", label: "CodeLLDB", available: false, resolvedPath: null },
  { id: "wasm-pack", label: "wasm-pack", available: false, resolvedPath: null },
  { id: "cargo-nextest", label: "cargo-nextest", available: false, resolvedPath: null },
  { id: "cargo-watch", label: "cargo-watch", available: false, resolvedPath: null },
  { id: "cargo-audit", label: "cargo-audit", available: false, resolvedPath: null },
  { id: "cargo-deny", label: "cargo-deny", available: false, resolvedPath: null },
  { id: "cargo-expand", label: "cargo-expand", available: false, resolvedPath: null },
  { id: "cargo-llvm-cov", label: "cargo-llvm-cov", available: false, resolvedPath: null },
  { id: "clangd", label: "clangd", available: false, resolvedPath: null },
  { id: "clang", label: "Clang", available: false, resolvedPath: null },
  { id: "clang++", label: "Clang++", available: false, resolvedPath: null },
  { id: "gcc", label: "GCC", available: false, resolvedPath: null },
  { id: "g++", label: "G++", available: false, resolvedPath: null },
  { id: "cl", label: "MSVC cl", available: false, resolvedPath: null },
  { id: "cmake", label: "CMake", available: false, resolvedPath: null },
  { id: "ninja", label: "Ninja", available: false, resolvedPath: null },
  { id: "make", label: "Make", available: false, resolvedPath: null },
  { id: "nvcc", label: "NVCC", available: false, resolvedPath: null },
  { id: "cuda-gdb", label: "cuda-gdb", available: false, resolvedPath: null },
];

const CODEX_AGENT_LABEL =
  AGENTS.find((agent) => agent.id === "codex")?.label ?? "OpenAI Codex";
const GEMINI_AGENT_LABEL =
  AGENTS.find((agent) => agent.id === "gemini")?.label ?? "Gemini CLI";
const DEFAULT_AGENT_MODELS = Object.fromEntries(
  AGENTS.map((agent) => [agent.id, agent.models?.[0]?.id ?? ""])
);

const QUICK_PROMPTS = [
  "Explain the active file and call out the riskiest part.",
  "Suggest the next concrete implementation step for this project.",
  "Review the current file for bugs or regressions.",
  "Refactor the active file into a cleaner structure.",
];

const EMPTY_EDITOR_SEMANTICS: EditorSemanticsPayload = {
  tokens: [],
  hoverItems: [],
};

function defaultAgentModel(agent: AgentDefinition) {
  return agent.models?.[0] ?? null;
}

const PERSISTED_APP_STATE_KEY = "hematite.app-state.v1";
const MAX_CACHED_DIRECTORY_COUNT = 18;
const MAX_CACHED_DIRECTORY_ENTRIES = 160;
const MAX_CACHED_DOCUMENT_CHARS = 160_000;
let persistedAppStateFlushHandle: number | null = null;

function queuePersistedAppStateWrite(state: PersistedAppState) {
  if (typeof window === "undefined") {
    return;
  }

  if (persistedAppStateFlushHandle != null) {
    window.clearTimeout(persistedAppStateFlushHandle);
  }

  const serialized = JSON.stringify(state);
  persistedAppStateFlushHandle = window.setTimeout(() => {
    persistedAppStateFlushHandle = null;
    void invoke("save_ui_state", {
      request: {
        stateJson: serialized,
      },
    }).catch(() => {
      // Ignore persistence failures so the UI stays responsive.
    });
  }, 180);
}

function emptyPersistedAppState(): PersistedAppState {
  return {
    version: 1,
    lastWorkspace: "",
    workspaces: {},
  };
}

function loadPersistedAppState(): PersistedAppState {
  if (typeof window === "undefined") {
    return emptyPersistedAppState();
  }

  try {
    const raw = window.localStorage.getItem(PERSISTED_APP_STATE_KEY);
    if (!raw) {
      return emptyPersistedAppState();
    }

    const parsed = JSON.parse(raw) as Partial<PersistedAppState>;
    return {
      version: 1,
      lastWorkspace:
        typeof parsed.lastWorkspace === "string" ? sanitizeStoredPath(parsed.lastWorkspace) : "",
      workspaces:
        parsed.workspaces && typeof parsed.workspaces === "object"
          ? Object.fromEntries(
              Object.entries(parsed.workspaces).map(([root, snapshot]) => [
                sanitizeStoredPath(root),
                {
                  activeTab:
                    snapshot &&
                    typeof snapshot === "object" &&
                    "activeTab" in snapshot &&
                    typeof snapshot.activeTab === "string"
                      ? sanitizeStoredPath(snapshot.activeTab)
                      : null,
                  expandedDirectories:
                    snapshot &&
                    typeof snapshot === "object" &&
                    "expandedDirectories" in snapshot &&
                    Array.isArray(snapshot.expandedDirectories)
                      ? snapshot.expandedDirectories
                          .filter((value): value is string => typeof value === "string")
                          .map((value) => sanitizeStoredPath(value))
                          .filter(Boolean)
                      : [],
                  directoryCache:
                    snapshot &&
                    typeof snapshot === "object" &&
                    "directoryCache" in snapshot &&
                    snapshot.directoryCache &&
                    typeof snapshot.directoryCache === "object"
                      ? Object.fromEntries(
                          Object.entries(snapshot.directoryCache).map(([path, entries]) => [
                            sanitizeStoredPath(path),
                            sanitizeCachedFileEntries(entries),
                          ])
                        )
                      : {},
                  activeDocumentSnapshot:
                    snapshot &&
                    typeof snapshot === "object" &&
                    "activeDocumentSnapshot" in snapshot
                      ? sanitizePersistedDocumentSnapshot(snapshot.activeDocumentSnapshot)
                      : null,
                },
              ])
            )
          : {},
    };
  } catch {
    return emptyPersistedAppState();
  }
}

function savePersistedAppState(state: PersistedAppState) {
  if (typeof window === "undefined") {
    return;
  }

  try {
    window.localStorage.setItem(PERSISTED_APP_STATE_KEY, JSON.stringify(state));
    queuePersistedAppStateWrite(state);
  } catch {
    // Ignore persistence failures so the UI keeps working normally.
  }
}

function sanitizeCachedFileEntries(value: unknown): FileEntry[] {
  if (!Array.isArray(value)) {
    return [];
  }

  return value
    .filter(
      (entry): entry is FileEntry =>
        Boolean(
          entry &&
            typeof entry === "object" &&
            "name" in entry &&
            typeof entry.name === "string" &&
            "path" in entry &&
            typeof entry.path === "string" &&
            "isDir" in entry &&
            typeof entry.isDir === "boolean"
        )
    )
    .slice(0, MAX_CACHED_DIRECTORY_ENTRIES)
    .map((entry) => ({
      name: entry.name,
      path: sanitizeStoredPath(entry.path),
      isDir: entry.isDir,
      size: typeof entry.size === "number" ? entry.size : null,
    }));
}

function sanitizeStoredPath(path: string) {
  const trimmed = path.trim();
  if (!trimmed) {
    return "";
  }

  if (trimmed.startsWith("\\\\?\\UNC\\")) {
    return `\\\\${trimmed.slice("\\\\?\\UNC\\".length)}`;
  }

  if (trimmed.startsWith("\\\\?\\")) {
    return trimmed.slice("\\\\?\\".length);
  }

  return trimmed;
}

function sanitizePersistedDocumentSnapshot(value: unknown): PersistedDocumentSnapshot | null {
  if (!value || typeof value !== "object") {
    return null;
  }

  if (
    !("path" in value) ||
    typeof value.path !== "string" ||
    !("name" in value) ||
    typeof value.name !== "string" ||
    !("language" in value) ||
    typeof value.language !== "string" ||
    !("content" in value) ||
    typeof value.content !== "string"
  ) {
    return null;
  }

  return {
    path: sanitizeStoredPath(value.path),
    name: value.name,
    language: value.language,
    content: value.content.slice(0, MAX_CACHED_DOCUMENT_CHARS),
  };
}

function normalizePathForCompare(path: string) {
  return sanitizeStoredPath(path).replace(/\//g, "\\").replace(/\\+$/, "").toLowerCase();
}

function isPathInsideRoot(path: string, root: string) {
  const normalizedPath = normalizePathForCompare(path);
  const normalizedRoot = normalizePathForCompare(root);
  return (
    normalizedPath === normalizedRoot ||
    normalizedPath.startsWith(`${normalizedRoot}\\`)
  );
}

function dedupePaths(paths: Array<string | null | undefined>) {
  const seen = new Set<string>();
  const ordered: string[] = [];

  for (const rawPath of paths) {
    const path = rawPath ? sanitizeStoredPath(rawPath) : "";
    if (!path) {
      continue;
    }

    const normalized = normalizePathForCompare(path);
    if (seen.has(normalized)) {
      continue;
    }
    seen.add(normalized);
    ordered.push(path);
  }

  return ordered;
}

function joinPath(root: string, ...parts: string[]) {
  const separator = root.includes("\\") ? "\\" : "/";
  const trimmedRoot = root.replace(/[\\/]+$/, "");
  const trimmedParts = parts.map((part) => part.replace(/^[\\/]+|[\\/]+$/g, ""));
  return [trimmedRoot, ...trimmedParts].join(separator);
}

function basename(path: string) {
  return path.split(/[/\\]/).filter(Boolean).pop() ?? path;
}

function dirname(path: string) {
  const boundary = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return boundary >= 0 ? path.slice(0, boundary) : "";
}

function relativePath(root: string, path: string) {
  if (!isPathInsideRoot(path, root)) {
    return path;
  }

  return path.slice(root.replace(/[\\/]+$/, "").length).replace(/^[\\/]+/, "");
}

function formatFileSize(bytes?: number | null) {
  if (bytes == null) {
    return "";
  }
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${Math.round(bytes / 1024)} KB`;
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function makeId() {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

function formatTime(date = new Date()) {
  return new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

function badgeTone(state?: string | null) {
  switch (state) {
    case "ready":
    case "installed":
      return "ready";
    case "partial":
    case "in_progress":
      return "partial";
    case "missing":
    case "warning":
    case "failed":
    case "cooldown":
      return "warning";
    default:
      return "muted";
  }
}

function createSystemMessage(workspace?: string): ChatMessage {
  const prefix = workspace
    ? `Workspace: ${basename(workspace)}.`
    : "Open a workspace to let agents use compact project context.";

  return {
    id: makeId(),
    role: "system",
    content: `${prefix} Ask for edits, reviews, explanations, or next steps and Hematite will keep the exchange in a threaded chat view.`,
    timestamp: formatTime(),
    agentLabel: "Hematite",
    status: "complete",
  };
}

function buildConversationPrompt(messages: ChatMessage[], latestInput: string) {
  const history = messages
    .filter((message) => message.role !== "system" && message.status !== "running")
    .slice(-8)
    .map((message) =>
      message.role === "user"
        ? `User:\n${message.content}`
        : `${message.agentLabel ?? "Assistant"}:\n${message.content}`
    )
    .join("\n\n");

  return history ? `${history}\n\nUser:\n${latestInput}` : latestInput;
}

function approvalTitle(approval: AgentApproval) {
  if (approval.title) {
    return approval.title;
  }
  switch (approval.approvalType) {
    case "command":
      return "Command approval requested";
    case "fileChange":
      return "File access approval requested";
    case "permissions":
      return "Extra permissions requested";
    default:
      return "Approval requested";
  }
}

function approvalStateLabel(state: ApprovalState) {
  switch (state) {
    case "submitted":
      return "Waiting for agent...";
    case "resolved":
      return "Resolved";
    default:
      return "Pending";
  }
}

function pythonPackageName(module: string) {
  const normalized = module.startsWith("import:") ? module.slice("import:".length) : module;
  switch (normalized) {
    case "PIL":
      return "Pillow";
    case "bs4":
      return "beautifulsoup4";
    case "cv2":
      return "opencv-python";
    case "dotenv":
      return "python-dotenv";
    case "sklearn":
      return "scikit-learn";
    case "yaml":
      return "PyYAML";
    default:
      return normalized;
  }
}

function pythonDiagnosticImportModule(module: string) {
  return module.startsWith("import:") ? module.slice("import:".length) : null;
}

async function invokeCommand<T>(command: string, args?: Record<string, unknown>) {
  return invoke<T>(command, args);
}

function eventStateLabel(event: PythonImportEvent) {
  switch (event.state) {
    case "installed":
      return "Installed";
    case "failed":
      return "Failed";
    case "cooldown":
      return "Cooldown";
    case "in_progress":
      return "In progress";
    default:
      return event.state;
  }
}

function installCommandPreview(environment: PythonEnvironmentStatus | null, packageName: string) {
  if (environment?.pyprojectExists) {
    return `uv add ${packageName}`;
  }
  return `uv pip install --python .venv ${packageName}`;
}

function toCodeMirrorDiagnostics(diagnostics: EditorDiagnostic[]): Diagnostic[] {
  return diagnostics.map((diagnostic) => ({
    from: diagnostic.from,
    to: diagnostic.to,
    severity:
      diagnostic.severity === "error" ||
      diagnostic.severity === "warning" ||
      diagnostic.severity === "info"
        ? diagnostic.severity
        : "warning",
    message: diagnostic.message,
    source: diagnostic.module,
  }));
}

function diagnosticSeverityRank(diagnostic: EditorDiagnostic) {
  switch (diagnostic.severity) {
    case "error":
      return 0;
    case "warning":
      return 1;
    case "info":
      return 2;
    default:
      return 3;
  }
}

function summarizeDiagnostics(diagnostics: EditorDiagnostic[]) {
  let errors = 0;
  let warnings = 0;
  let info = 0;

  for (const diagnostic of diagnostics) {
    if (diagnostic.severity === "error") {
      errors += 1;
    } else if (diagnostic.severity === "warning") {
      warnings += 1;
    } else {
      info += 1;
    }
  }

  return {
    errors,
    warnings,
    info,
    total: diagnostics.length,
  };
}

function diagnosticSourceLabel(module: string) {
  if (module.startsWith("import:") || module.includes("unresolved")) {
    return "ty";
  }
  if (/^[A-Z]\d{3,}$/.test(module) || module === "ruff") {
    return "Ruff";
  }
  if (module.startsWith("E") && /\d/.test(module)) {
    return "rustc";
  }
  return module || "tool";
}

function diagnosticBadgeTone(stats: ReturnType<typeof summarizeDiagnostics>) {
  if (stats.errors) {
    return "error";
  }
  if (stats.warnings) {
    return "warning";
  }
  if (stats.info) {
    return "info";
  }
  return "muted";
}

function resetLineJump(setter: (value: number | null) => void, line: number) {
  setter(null);
  queueMicrotask(() => setter(line));
}

function TreeNode(props: {
  entry: FileEntry;
  depth: number;
  activePath: string | null;
  isExpanded: boolean;
  directoryState?: DirectoryState;
  children?: any;
  onToggleDirectory: (entry: FileEntry) => void;
  onOpenFile: (path: string) => void;
}) {
  return (
    <li>
      <button
        type="button"
        class={`tree-item${props.activePath === props.entry.path ? " active" : ""}${
          props.entry.isDir ? " directory" : ""
        }`}
        style={{ "padding-left": `${props.depth * 14 + 12}px` }}
        onClick={() =>
          props.entry.isDir
            ? props.onToggleDirectory(props.entry)
            : props.onOpenFile(props.entry.path)
        }
      >
        <span class="tree-icon">
          <Show when={props.entry.isDir} fallback="-">
            {props.isExpanded ? "v" : ">"}
          </Show>
        </span>
        <span class="tree-label">{props.entry.name}</span>
        <Show when={!props.entry.isDir && props.entry.size}>
          <span class="tree-meta">{formatFileSize(props.entry.size)}</span>
        </Show>
      </button>

      <Show when={props.entry.isDir && props.isExpanded}>
        <Show
          when={props.directoryState?.entries?.length}
          fallback={
            <div class="empty-note" style={{ "padding-left": `${props.depth * 14 + 38}px` }}>
              <Show
                when={props.directoryState?.loading}
                fallback={props.directoryState?.error ?? "Empty"}
              >
                Loading...
              </Show>
            </div>
          }
        >
          <ul class="tree-children">{props.children}</ul>
        </Show>
      </Show>
    </li>
  );
}

export default function App() {
  const [workspaceRoot, setWorkspaceRoot] = createSignal("");
  const [workspaceInput, setWorkspaceInput] = createSignal("");
  const [tools, setTools] = createSignal<ToolStatus[]>(DEFAULT_TOOLS);
  const [statusMessage, setStatusMessage] = createSignal("Starting Hematite workbench...");
  const [venvPathHint, setVenvPathHint] = createSignal("");
  const [expandedDirectories, setExpandedDirectories] = createSignal<Set<string>>(new Set());
  const [openTabs, setOpenTabs] = createSignal<string[]>([]);
  const [activeTab, setActiveTab] = createSignal<string | null>(null);
  const [symbols, setSymbols] = createSignal<SymbolEntry[]>([]);
  const [editorSemantics, setEditorSemantics] =
    createSignal<EditorSemanticsPayload>(EMPTY_EDITOR_SEMANTICS);
  const [compactContext, setCompactContext] = createSignal("");
  const [isRefreshingContext, setIsRefreshingContext] = createSignal(false);
  const [jumpToLine, setJumpToLine] = createSignal<number | null>(null);
  const [cursorPosition, setCursorPosition] = createSignal<CursorPosition>({
    line: 1,
    column: 1,
  });
  const [lineWrapping, setLineWrapping] = createSignal(true);
  const [isOpeningWorkspace, setIsOpeningWorkspace] = createSignal(false);
  const [isHydratingWorkspaceState, setIsHydratingWorkspaceState] = createSignal(false);
  const [utilityTab, setUtilityTab] = createSignal<UtilityTab>("chat");
  const [selectedAgentId, setSelectedAgentId] = createSignal(AGENTS[0].id);
  const [chatDraft, setChatDraft] = createSignal("");
  const [includeCompactContext, setIncludeCompactContext] = createSignal(true);
  const [isRunningAgent, setIsRunningAgent] = createSignal(false);
  const [chatMessages, setChatMessages] = createSignal<ChatMessage[]>([createSystemMessage()]);
  const [activeCodexAssistantMessageId, setActiveCodexAssistantMessageId] =
    createSignal<string | null>(null);
  const [activeCodexTurnId, setActiveCodexTurnId] = createSignal<string | null>(null);
  const [activeGeminiAssistantMessageId, setActiveGeminiAssistantMessageId] =
    createSignal<string | null>(null);
  const [activeGeminiSessionId, setActiveGeminiSessionId] = createSignal<string | null>(null);
  const [agentHealth, setAgentHealth] = createSignal<AgentHealthPayload | null>(null);
  const [isSavingCredentials, setIsSavingCredentials] = createSignal(false);
  const [launchingAgentId, setLaunchingAgentId] = createSignal<string | null>(null);
  const [pythonEnvironment, setPythonEnvironment] =
    createSignal<PythonEnvironmentStatus | null>(null);
  const [rustEnvironment, setRustEnvironment] =
    createSignal<RustEnvironmentStatus | null>(null);
  const [cFamilyEnvironment, setCFamilyEnvironment] =
    createSignal<CFamilyEnvironmentStatus | null>(null);
  const [isPreparingPythonEnvironment, setIsPreparingPythonEnvironment] =
    createSignal(false);
  const [prepareOutcome, setPrepareOutcome] = createSignal<ProcessOutcome | null>(null);
  const [terminalInput, setTerminalInput] = createSignal("");
  const [isRunningTerminal, setIsRunningTerminal] = createSignal(false);
  const [terminalCwd, setTerminalCwd] = createSignal("");
  const [terminalEntries, setTerminalEntries] = createSignal<TerminalEntry[]>([]);
  const [isTerminalVisible, setIsTerminalVisible] = createSignal(true);
  const [explorerWidth, setExplorerWidth] = createSignal(320);
  const [utilityWidth, setUtilityWidth] = createSignal(384);
  const [terminalHeight, setTerminalHeight] = createSignal(248);
  const [isInstallingMissingPackages, setIsInstallingMissingPackages] =
    createSignal(false);
  const [runningPythonToolingAction, setRunningPythonToolingAction] =
    createSignal<PythonToolingAction | null>(null);
  const [pythonToolingOutcome, setPythonToolingOutcome] =
    createSignal<ProcessOutcome | null>(null);
  const [runningRustToolingAction, setRunningRustToolingAction] =
    createSignal<RustToolingAction | null>(null);
  const [rustToolingOutcome, setRustToolingOutcome] =
    createSignal<ProcessOutcome | null>(null);
  const [isCreatingFile, setIsCreatingFile] = createSignal(false);
  const [newFileDraft, setNewFileDraft] = createSignal("");

  const [directories, setDirectories] = createStore<Record<string, DirectoryState>>({});
  const [documents, setDocuments] = createStore<Record<string, EditorDocument>>({});
  const [selectedAgentModels, setSelectedAgentModels] =
    createStore<Record<string, string>>(DEFAULT_AGENT_MODELS);
  const [credentialsForm, setCredentialsForm] = createStore<CredentialsForm>({
    openaiApiKey: "",
    geminiApiKey: "",
    googleApiKey: "",
    googleCloudProject: "",
    googleCloudLocation: "",
    googleApplicationCredentials: "",
    anthropicApiKey: "",
    kiloApiKey: "",
  });

  let shellRef: HTMLDivElement | undefined;
  let gridRef: HTMLDivElement | undefined;
  let chatTimelineRef: HTMLDivElement | undefined;
  let chatInputRef: HTMLTextAreaElement | undefined;
  let terminalLogRef: HTMLDivElement | undefined;
  let newFileInputRef: HTMLInputElement | undefined;

  const activeDocument = createMemo(() => {
    const path = activeTab();
    return path ? documents[path] : undefined;
  });

  const activeCodeMirrorDiagnostics = createMemo(() =>
    toCodeMirrorDiagnostics(activeDocument()?.diagnostics ?? [])
  );

  const activeDiagnostics = createMemo(() => activeDocument()?.diagnostics ?? []);

  const activeDiagnosticStats = createMemo(() =>
    summarizeDiagnostics(activeDiagnostics())
  );

  const activeDiagnosticsSorted = createMemo(() =>
    [...activeDiagnostics()].sort(
      (left, right) =>
        diagnosticSeverityRank(left) - diagnosticSeverityRank(right) ||
        left.line - right.line ||
        left.column - right.column
    )
  );

  const activeDiagnosticSources = createMemo(() => {
    const counts = new Map<string, number>();
    for (const diagnostic of activeDiagnostics()) {
      const label = diagnosticSourceLabel(diagnostic.module);
      counts.set(label, (counts.get(label) ?? 0) + 1);
    }
    return [...counts.entries()].map(([label, count]) => ({ label, count }));
  });

  const selectedAgent = createMemo(
    () => AGENTS.find((agent) => agent.id === selectedAgentId()) ?? AGENTS[0]
  );

  const selectedAgentStatus = createMemo(() =>
    agentHealth()?.agents.find((agent) => agent.id === selectedAgentId())
  );

  const selectedAgentModel = createMemo(() => {
    const agent = selectedAgent();
    const selectedModelId = selectedAgentModels[agent.id] ?? defaultAgentModel(agent)?.id ?? "";
    return agent.models?.find((model) => model.id === selectedModelId) ?? defaultAgentModel(agent);
  });

  const activeInstallEvents = createMemo(() => activeDocument()?.installEvents ?? []);

  const missingImports = createMemo(() => {
    const document = activeDocument();
    if (!document || document.language !== "python") {
      return [];
    }

    const seen = new Set<string>();
    const results: Array<{ module: string; package: string }> = [];

    for (const diagnostic of document.diagnostics) {
      const importModule = pythonDiagnosticImportModule(diagnostic.module);
      if (diagnostic.severity !== "error" || !importModule) {
        continue;
      }
      if (seen.has(importModule)) {
        continue;
      }
      seen.add(importModule);
      results.push({
        module: importModule,
        package: pythonPackageName(importModule),
      });
    }

    return results;
  });

  const layoutStyle = createMemo<Record<string, string>>(() => ({
    "--explorer-width": `${explorerWidth()}px`,
    "--utility-width": `${utilityWidth()}px`,
    "--terminal-height": `${terminalHeight()}px`,
  }));

  const explorerEntries = createMemo(() => directories[workspaceRoot()]?.entries ?? []);

  const isPythonInstallActionAvailable = createMemo(() => {
    const environment = pythonEnvironment();
    const document = activeDocument();
    return Boolean(
      workspaceRoot() &&
        environment?.uvAvailable &&
        document?.language === "python" &&
        missingImports().length
    );
  });

  const isPythonToolingAvailable = createMemo(
    () => Boolean(workspaceRoot() && activeDocument()?.language === "python")
  );

  const isRustToolingAvailable = createMemo(
    () =>
      Boolean(
        workspaceRoot() &&
          activeDocument()?.language === "rust" &&
          rustEnvironment()?.cargoTomlExists
      )
  );

  const isCFamilyDocumentActive = createMemo(() =>
    ["c", "cpp", "cuda-cpp"].includes(activeDocument()?.language ?? "")
  );

  const rustToolStatus = createMemo(() => {
    const byId = new Map(tools().map((tool) => [tool.id, tool]));
    return [
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
      .map((id) => byId.get(id))
      .filter((tool): tool is ToolStatus => Boolean(tool));
  });

  const cFamilyToolStatus = createMemo(() => {
    const byId = new Map(tools().map((tool) => [tool.id, tool]));
    return [
      "clangd",
      "clang",
      "clang++",
      "gcc",
      "g++",
      "cl",
      "cmake",
      "ninja",
      "make",
      "nvcc",
      "cuda-gdb",
      "lldb",
      "codelldb",
    ]
      .map((id) => byId.get(id))
      .filter((tool): tool is ToolStatus => Boolean(tool));
  });

  const pythonManagementSummary = createMemo(() => {
    const document = activeDocument();
    if (!workspaceRoot()) {
      return "Open a workspace to run Ruff and ty for Python diagnostics.";
    }
    if (!document) {
      return "Open a Python file and Hematite will collect Ruff/ty diagnostics as you type.";
    }
    if (document.language !== "python") {
      return "Switch to a Python file to scan Ruff/ty diagnostics and missing imports.";
    }
    if (!missingImports().length) {
      return "No unresolved third-party imports were detected in the active Python file.";
    }
    return `The active Python file has ${missingImports().length} unresolved third-party import${
      missingImports().length === 1 ? "" : "s"
    }. Review the list below, then install them together with one click.`;
  });

  const rustManagementSummary = createMemo(() => {
    const document = activeDocument();
    const environment = rustEnvironment();
    if (!workspaceRoot()) {
      return "Open a workspace to inspect Rust toolchains and run Cargo automation.";
    }
    if (!environment?.cargoTomlExists) {
      return "Standalone Rust parser support is active. Open a Cargo package or workspace to enable rust-analyzer, Cargo checks, Clippy, tests, docs, and metadata.";
    }
    if (!document) {
      return "Open a Rust file to use rust-analyzer hover, semantic coloring, rustfmt, and active-file diagnostics.";
    }
    if (document.language !== "rust") {
      return "Switch to a Rust file to run Cargo diagnostics and rustfmt against the active source.";
    }
    return "Rust IDE support is active for the current file. Cargo actions run explicitly so Hematite stays quiet while idle.";
  });

  const cFamilyManagementSummary = createMemo(() => {
    const document = activeDocument();
    const environment = cFamilyEnvironment();
    if (!workspaceRoot()) {
      return "Open a workspace to inspect C/C++/CUDA toolchains and compile metadata.";
    }
    if (!environment?.compileCommandsExists) {
      return "Standalone C-family parser support is active. Add compile_commands.json or CMake metadata to prepare clangd-backed project intelligence.";
    }
    if (!document) {
      return "Open a C, C++, or CUDA file to use parser-backed hover, semantic coloring, and outline.";
    }
    if (!isCFamilyDocumentActive()) {
      return "Switch to a C, C++, or CUDA file to inspect the active source with the C-family parser.";
    }
    return environment.clangdAvailable
      ? "C-family parser support is active, with clangd and compile_commands.json detected for project-aware integration."
      : "C-family parser support is active. Install clangd to use the detected compile database for project-aware language service features.";
  });

  const pythonInstallModeLabel = createMemo(() =>
    missingImports().length ? `Python batch install ${missingImports().length}` : "Python batch install"
  );

  const pendingApprovalCount = createMemo(
    () => chatMessages().filter((message) => message.approval?.state === "pending").length
  );

  function setStatus(message: string) {
    setStatusMessage(message);
  }

  function resetChat(workspace = workspaceRoot()) {
    setChatMessages([createSystemMessage(workspace || undefined)]);
    setChatDraft("");
    setActiveCodexAssistantMessageId(null);
    setActiveCodexTurnId(null);
    setActiveGeminiAssistantMessageId(null);
    setActiveGeminiSessionId(null);
  }

  function updateChatMessage(messageId: string, updater: (message: ChatMessage) => ChatMessage) {
    setChatMessages((messages) =>
      messages.map((message) => (message.id === messageId ? updater(message) : message))
    );
  }

  function updateApprovalState(requestId: string, state: ApprovalState) {
    setChatMessages((messages) =>
      messages.map((message) =>
        message.approval?.requestId === requestId
          ? {
              ...message,
              approval: {
                ...message.approval,
                state,
              },
            }
          : message
      )
    );
  }

  async function resetCodexBackend(root = workspaceRoot()) {
    if (!root) {
      return;
    }

    try {
      await invokeCommand("reset_codex_session", {
        request: {
          root,
        },
      });
    } catch (error) {
      setStatus(`Could not reset the Codex session: ${String(error)}`);
    }
  }

  async function resetGeminiBackend(root = workspaceRoot()) {
    if (!root) {
      return;
    }

    try {
      await invokeCommand("reset_gemini_session", {
        request: {
          root,
        },
      });
    } catch (error) {
      setStatus(`Could not reset the Gemini session: ${String(error)}`);
    }
  }

  async function startNewChat(workspace = workspaceRoot()) {
    if (isRunningAgent()) {
      setStatus("Wait for the current agent turn to finish before starting a fresh chat.");
      return;
    }

    await resetCodexBackend(workspace);
    await resetGeminiBackend(workspace);
    resetChat(workspace);
    setStatus("Started a fresh agent chat.");
  }

  function toggleTerminal(force?: boolean) {
    const next = force ?? !isTerminalVisible();
    setIsTerminalVisible(next);
    setStatus(next ? "Opened the integrated terminal." : "Collapsed the integrated terminal.");
  }

  function beginResize(
    event: MouseEvent,
    axis: "vertical" | "horizontal",
    onMove: (deltaX: number, deltaY: number) => void
  ) {
    event.preventDefault();
    const startX = event.clientX;
    const startY = event.clientY;
    const previousCursor = document.body.style.cursor;
    const previousUserSelect = document.body.style.userSelect;

    document.body.style.cursor = axis === "vertical" ? "col-resize" : "row-resize";
    document.body.style.userSelect = "none";

    const handleMove = (moveEvent: MouseEvent) => {
      onMove(moveEvent.clientX - startX, moveEvent.clientY - startY);
    };

    const handleUp = () => {
      document.body.style.cursor = previousCursor;
      document.body.style.userSelect = previousUserSelect;
      window.removeEventListener("mousemove", handleMove);
      window.removeEventListener("mouseup", handleUp);
    };

    window.addEventListener("mousemove", handleMove);
    window.addEventListener("mouseup", handleUp, { once: true });
  }

  function resizeExplorer(event: MouseEvent) {
    const width = gridRef?.clientWidth ?? window.innerWidth;
    const current = explorerWidth();
    const min = 220;
    const max = Math.max(min, width - utilityWidth() - 420);
    beginResize(event, "vertical", (deltaX) => {
      setExplorerWidth(clamp(current + deltaX, min, max));
    });
  }

  function resizeUtility(event: MouseEvent) {
    const width = gridRef?.clientWidth ?? window.innerWidth;
    const current = utilityWidth();
    const min = 320;
    const max = Math.max(min, width - explorerWidth() - 420);
    beginResize(event, "vertical", (deltaX) => {
      setUtilityWidth(clamp(current - deltaX, min, max));
    });
  }

  function resizeTerminal(event: MouseEvent) {
    const height = shellRef?.clientHeight ?? window.innerHeight;
    const current = terminalHeight();
    const min = 170;
    const max = Math.max(min, height - 240);
    beginResize(event, "horizontal", (_deltaX, deltaY) => {
      setTerminalHeight(clamp(current - deltaY, min, max));
    });
  }

  function syncCredentialForm(snapshot: CredentialSnapshot) {
    setCredentialsForm("googleCloudProject", snapshot.googleCloudProject ?? "");
    setCredentialsForm("googleCloudLocation", snapshot.googleCloudLocation ?? "");
    setCredentialsForm(
      "googleApplicationCredentials",
      snapshot.googleApplicationCredentials ?? ""
    );
  }

  async function refreshAgentHealth() {
    const payload = await invokeCommand<AgentHealthPayload>("refresh_agent_health");
    setAgentHealth(payload);
    syncCredentialForm(payload.credentials);
  }

  async function refreshToolStatuses() {
    try {
      const payload = await invokeCommand<ToolStatus[]>("refresh_tool_statuses");
      setTools(payload);
    } catch (error) {
      setStatus(`Could not refresh local tool status: ${String(error)}`);
    }
  }

  async function refreshPythonEnvironment(root: string | undefined = workspaceRoot()) {
    if (!root) {
      setPythonEnvironment(null);
      return;
    }

    try {
      const status = await invokeCommand<PythonEnvironmentStatus>(
        "inspect_python_environment",
        { root }
      );
      setPythonEnvironment(status);
    } catch (error) {
      setStatus(`Could not inspect Python environment: ${String(error)}`);
    }
  }

  async function refreshRustEnvironment(root: string | undefined = workspaceRoot()) {
    if (!root) {
      setRustEnvironment(null);
      return;
    }

    try {
      const status = await invokeCommand<RustEnvironmentStatus>(
        "inspect_rust_environment",
        { root }
      );
      setRustEnvironment(status);
    } catch (error) {
      setStatus(`Could not inspect Rust toolchains: ${String(error)}`);
    }
  }

  async function refreshCFamilyEnvironment(root: string | undefined = workspaceRoot()) {
    if (!root) {
      setCFamilyEnvironment(null);
      return;
    }

    try {
      const status = await invokeCommand<CFamilyEnvironmentStatus>(
        "inspect_c_family_environment",
        { root }
      );
      setCFamilyEnvironment(status);
    } catch (error) {
      setStatus(`Could not inspect C/C++/CUDA toolchains: ${String(error)}`);
    }
  }

  async function requestEditorHover(
    request: EditorHoverRequest
  ): Promise<EditorHoverItem | null> {
    const root = workspaceRoot();
    const language = activeDocument()?.language;
    if (
      !root ||
      !activeDocument() ||
      !["python", "rust", "c", "cpp", "cuda-cpp"].includes(language ?? "")
    ) {
      return null;
    }

    try {
      return await invokeCommand<EditorHoverItem | null>("request_editor_hover", {
        request: {
          root,
          filePath: request.path,
          source: request.content,
          line: request.line,
          column: request.column,
        },
      });
    } catch {
      return null;
    }
  }

  function readPersistedWorkspaceSnapshot(root: string) {
    const state = loadPersistedAppState();
    if (state.workspaces[root]) {
      return state.workspaces[root];
    }

    const matchedKey = Object.keys(state.workspaces).find(
      (key) => normalizePathForCompare(key) === normalizePathForCompare(root)
    );

    return matchedKey ? state.workspaces[matchedKey] : null;
  }

  function seedCachedDirectoryCache(root: string, snapshot: PersistedWorkspaceSnapshot | null) {
    if (!snapshot) {
      return;
    }

    const cachedPaths = dedupePaths([
      root,
      ...snapshot.expandedDirectories,
      ...Object.keys(snapshot.directoryCache),
    ]).filter((path) => isPathInsideRoot(path, root));

    for (const path of cachedPaths) {
      const entries = snapshot.directoryCache[path];
      if (!entries?.length) {
        continue;
      }

      setDirectories(path, {
        entries,
        loaded: true,
        loading: false,
        error: undefined,
      });
    }
  }

  function seedCachedActiveDocument(
    root: string,
    snapshot: PersistedWorkspaceSnapshot | null
  ) {
    const cachedDocument = snapshot?.activeDocumentSnapshot;
    if (!cachedDocument || !isPathInsideRoot(cachedDocument.path, root)) {
      return;
    }

    setDocuments(cachedDocument.path, {
      ...cachedDocument,
      savedContent: cachedDocument.content,
      dirty: false,
      diagnostics: [],
      installEvents: [],
    });
    setOpenTabs([cachedDocument.path]);
    setActiveTab(cachedDocument.path);
    setJumpToLine(null);
  }

  function buildPersistedDirectoryCache(root: string) {
    const relevantPaths = dedupePaths([
      root,
      ...Array.from(expandedDirectories()).filter((path) => isPathInsideRoot(path, root)),
    ]).slice(0, MAX_CACHED_DIRECTORY_COUNT);

    const cacheEntries = relevantPaths.flatMap((path) => {
      const directory = directories[path];
      if (!directory?.loaded || !directory.entries.length) {
        return [];
      }

      return [
        [
          path,
          directory.entries
            .slice(0, MAX_CACHED_DIRECTORY_ENTRIES)
            .map((entry) => ({
              name: entry.name,
              path: entry.path,
              isDir: entry.isDir,
              size: entry.size ?? null,
            })),
        ] as const,
      ];
    });

    return Object.fromEntries(cacheEntries);
  }

  function buildPersistedDocumentSnapshot(root: string) {
    const document = activeDocument();
    if (!document || !isPathInsideRoot(document.path, root)) {
      return null;
    }

    const content = (document.dirty ? document.savedContent : document.content).slice(
      0,
      MAX_CACHED_DOCUMENT_CHARS
    );

    return {
      path: document.path,
      name: document.name,
      language: document.language,
      content,
    } satisfies PersistedDocumentSnapshot;
  }

  function persistWorkspaceSession(root = workspaceRoot()) {
    if (!root || isHydratingWorkspaceState()) {
      return;
    }

    const state = loadPersistedAppState();
    state.lastWorkspace = root;
    state.workspaces[root] = {
      activeTab:
        activeTab() && isPathInsideRoot(activeTab()!, root) ? activeTab() : null,
      expandedDirectories: dedupePaths([
        root,
        ...Array.from(expandedDirectories()).filter((path) => isPathInsideRoot(path, root)),
      ]),
      directoryCache: buildPersistedDirectoryCache(root),
      activeDocumentSnapshot: buildPersistedDocumentSnapshot(root),
    };
    savePersistedAppState(state);
  }

  async function restoreWorkspaceSession(root: string) {
    const snapshot = readPersistedWorkspaceSnapshot(root);
    if (!snapshot) {
      return { found: false, restoredFile: false, wantedFile: false };
    }

    const expanded = dedupePaths([
      root,
      ...snapshot.expandedDirectories.filter((path) => isPathInsideRoot(path, root)),
    ]);
    setExpandedDirectories(new Set(expanded));

    const nestedDirectories = expanded.filter((path) => path !== root);
    window.setTimeout(() => {
      nestedDirectories.forEach((path, index) => {
        window.setTimeout(() => {
          if (workspaceRoot() !== root) {
            return;
          }
          void loadDirectory(path).catch(() => {
            // Ignore stale or missing folders during restore.
          });
        }, index * 36);
      });
    }, 24);

    const restoredPath =
      snapshot.activeTab && isPathInsideRoot(snapshot.activeTab, root)
        ? snapshot.activeTab
        : null;

    if (!restoredPath) {
      return { found: true, restoredFile: false, wantedFile: false };
    }

    try {
      await openFile(
        restoredPath,
        true,
        Boolean(snapshot.activeDocumentSnapshot?.path === restoredPath)
      );
      return { found: true, restoredFile: true, wantedFile: true };
    } catch {
      return { found: true, restoredFile: false, wantedFile: true };
    }
  }

  async function loadDirectory(path: string, force = false): Promise<DirectoryListing> {
    const existing = directories[path];
    if (!force && existing?.loaded) {
      return { path, entries: existing.entries };
    }

    setDirectories(path, {
      entries: existing?.entries ?? [],
      loaded: false,
      loading: true,
      error: undefined,
    });

    try {
      const listing = await invokeCommand<DirectoryListing>("list_directory", { path });
      setDirectories(path, {
        entries: listing.entries,
        loaded: true,
        loading: false,
        error: undefined,
      });
      persistWorkspaceSession();
      return listing;
    } catch (error) {
      setDirectories(path, {
        entries: [],
        loaded: false,
        loading: false,
        error: String(error),
      });
      throw error;
    }
  }

  async function openFile(path: string, silent = false, forceReload = false) {
    if (documents[path] && !forceReload) {
      setActiveTab(path);
      setJumpToLine(null);
      return;
    }

    const document = await invokeCommand<FileDocument>("read_file", { path });
    setDocuments(path, {
      ...document,
      savedContent: document.content,
      dirty: false,
      diagnostics: [],
      installEvents: [],
    });
    setOpenTabs((tabs) => (tabs.includes(path) ? tabs : [...tabs, path]));
    setActiveTab(path);
    setJumpToLine(null);

    if (!silent) {
      setStatus(`Opened ${document.name}.`);
    }

    persistWorkspaceSession();
  }

  function suggestNewFilePath() {
    const root = workspaceRoot();
    if (!root) {
      return "";
    }

    const activePath = activeTab();
    const currentDirectory =
      activePath && isPathInsideRoot(activePath, root) ? dirname(activePath) : root;
    const currentRelativeDirectory =
      currentDirectory && isPathInsideRoot(currentDirectory, root)
        ? relativePath(root, currentDirectory)
        : "";
    const currentExtension = activePath?.split(".").pop()?.toLowerCase();
    const suggestedName = currentExtension
      ? `new_file.${currentExtension}`
      : "new_file.ts";
    return currentRelativeDirectory
      ? `${currentRelativeDirectory.replace(/\\/g, "/")}/${suggestedName}`
      : suggestedName;
  }

  function beginCreateNewFile() {
    if (!workspaceRoot()) {
      setStatus("Open a workspace before creating a new file.");
      return;
    }

    setNewFileDraft(suggestNewFilePath());
    setIsCreatingFile(true);
    queueMicrotask(() => {
      newFileInputRef?.focus();
      const length = newFileInputRef?.value.length ?? 0;
      newFileInputRef?.setSelectionRange(length, length);
    });
  }

  function cancelCreateNewFile() {
    setIsCreatingFile(false);
    setNewFileDraft("");
  }

  async function submitCreateNewFile() {
    const root = workspaceRoot();
    if (!root) {
      setStatus("Open a workspace before creating a new file.");
      return;
    }

    const nextPath = newFileDraft().trim().replace(/\\/g, "/");
    if (!nextPath) {
      setStatus("New file creation was cancelled because the path was empty.");
      return;
    }

    try {
      const document = await invokeCommand<FileDocument>("create_file", {
        request: {
          root,
          relativePath: nextPath,
        },
      });

      setDocuments(document.path, {
        ...document,
        savedContent: document.content,
        dirty: false,
        diagnostics: [],
        installEvents: [],
      });
      setOpenTabs((tabs) => (tabs.includes(document.path) ? tabs : [...tabs, document.path]));
      setActiveTab(document.path);
      setJumpToLine(null);

      const parentDirectories = dedupePaths([
        root,
        ...dirname(document.path)
          .replace(root.replace(/[\\/]+$/, ""), "")
          .split(/[/\\]/)
          .filter(Boolean)
          .reduce<string[]>((paths, segment) => {
            const previous = paths.length ? paths[paths.length - 1] : root;
            paths.push(joinPath(previous, segment));
            return paths;
          }, []),
      ]);

      setExpandedDirectories((current) => {
        const next = new Set(current);
        parentDirectories.forEach((path) => next.add(path));
        return next;
      });

      for (const directoryPath of parentDirectories) {
        await loadDirectory(directoryPath, true);
      }

      persistWorkspaceSession(root);
      cancelCreateNewFile();
      setStatus(`Created ${relativePath(root, document.path) || document.name}.`);
    } catch (error) {
      setStatus(`Could not create a new file: ${String(error)}`);
    }
  }

  async function openBestInitialFile(root: string) {
    const preferredPaths = [
      joinPath(root, "src", "App.tsx"),
      joinPath(root, "src", "main.rs"),
      joinPath(root, "src", "lib.rs"),
      joinPath(root, "src-tauri", "src", "lib.rs"),
      joinPath(root, "src", "main.py"),
      joinPath(root, "README.md"),
    ];

    for (const path of preferredPaths) {
      try {
        await openFile(path, true);
        return;
      } catch {
        continue;
      }
    }

    try {
      const rootListing = await loadDirectory(root);
      const firstFile = rootListing.entries.find((entry) => !entry.isDir);
      if (firstFile) {
        await openFile(firstFile.path, true);
        return;
      }

      const firstDirectory = rootListing.entries.find((entry) => entry.isDir);
      if (!firstDirectory) {
        return;
      }

      const nested = await loadDirectory(firstDirectory.path);
      const nestedFile = nested.entries.find((entry) => !entry.isDir);
      if (nestedFile) {
        setExpandedDirectories((current) => {
          const next = new Set(current);
          next.add(firstDirectory.path);
          return next;
        });
        await openFile(nestedFile.path, true);
      }
    } catch {
      // Leave the editor empty if no obvious file is available.
    }
  }

  async function openWorkspace(path: string) {
    const trimmed = sanitizeStoredPath(path);
    if (!trimmed) {
      return false;
    }

    const cachedSnapshot = readPersistedWorkspaceSnapshot(trimmed);
    const cachedExpanded = cachedSnapshot
      ? dedupePaths([
          trimmed,
          ...cachedSnapshot.expandedDirectories.filter((value) =>
            isPathInsideRoot(value, trimmed)
          ),
        ])
      : [trimmed];

    setIsOpeningWorkspace(true);
    setIsHydratingWorkspaceState(true);
    setStatus(`Scanning ${trimmed}...`);
    setWorkspaceInput(trimmed);
    setCompactContext("");
    setVenvPathHint("");
    setSymbols([]);
    setJumpToLine(null);
    setPrepareOutcome(null);
    setRustToolingOutcome(null);
    setPythonToolingOutcome(null);
    setRunningRustToolingAction(null);
    setRunningPythonToolingAction(null);
    setTerminalEntries([]);
    setTerminalInput("");
    setTerminalCwd(trimmed);
    setOpenTabs([]);
    setActiveTab(null);
    setDirectories(reconcile({}));
    setDocuments(reconcile({}));
    setWorkspaceRoot(trimmed);
    setExpandedDirectories(new Set(cachedExpanded));
    seedCachedDirectoryCache(trimmed, cachedSnapshot);
    seedCachedActiveDocument(trimmed, cachedSnapshot);
    if (!cachedSnapshot?.directoryCache[trimmed]?.length) {
      setDirectories(trimmed, {
        entries: [],
        loaded: false,
        loading: true,
        error: undefined,
      });
    }

    try {
      const listing = await loadDirectory(trimmed, true);
      const root = listing.path || trimmed;
      const restoredSnapshot =
        root === trimmed ? cachedSnapshot : readPersistedWorkspaceSnapshot(root) ?? cachedSnapshot;
      setWorkspaceRoot(root);
      setWorkspaceInput(root);
      if (root !== trimmed) {
        seedCachedDirectoryCache(root, restoredSnapshot);
        seedCachedActiveDocument(root, restoredSnapshot);
        setDirectories(
          root,
          directories[trimmed] ?? {
            entries: listing.entries,
            loaded: true,
            loading: false,
            error: undefined,
          }
        );
      }
      setTerminalCwd(root);
      resetChat(root);
      const restored = await restoreWorkspaceSession(root);
      if (!restored.found) {
        setExpandedDirectories(new Set([root]));
      }
      setStatus(
        restored.restoredFile
          ? `Restored ${basename(root)}. Finishing background setup...`
          : `Workspace ready: ${basename(root)}. Finishing background setup...`
      );

      window.setTimeout(() => {
        void (async () => {
          try {
            await resetCodexBackend(root);
            await resetGeminiBackend(root);
          } catch {
            // Background session reset failures are reported by the helpers.
          }
        })();
      }, 0);

      window.setTimeout(() => {
        void (async () => {
          if (workspaceRoot() !== root) {
            return;
          }

          if (!restored.found || (restored.wantedFile && !restored.restoredFile)) {
            await openBestInitialFile(root);
          }

          if (workspaceRoot() === root) {
            setStatus(`Workspace ready: ${basename(root)}.`);
          }
        })();
      }, 280);

      window.setTimeout(() => {
        void (async () => {
          if (workspaceRoot() !== root) {
            return;
          }
          await Promise.all([
            refreshPythonEnvironment(root),
            refreshRustEnvironment(root),
            refreshCFamilyEnvironment(root),
          ]);
        })();
      }, 720);
      return true;
    } catch (error) {
      setWorkspaceRoot("");
      setExpandedDirectories(new Set<string>());
      setStatus(`Could not open workspace: ${String(error)}`);
      return false;
    } finally {
      setIsOpeningWorkspace(false);
      setIsHydratingWorkspaceState(false);
    }
  }

  async function browseWorkspace() {
    const selected = await invokeCommand<string | null>("pick_workspace_directory");
    if (selected) {
      setWorkspaceInput(selected);
      await openWorkspace(selected);
    }
  }

  async function browseServiceAccountFile() {
    const selected = await invokeCommand<string | null>("pick_service_account_file");
    if (selected) {
      setCredentialsForm("googleApplicationCredentials", selected);
    }
  }

  async function saveActiveFile(path = activeTab()) {
    if (!path) {
      return;
    }

    const document = documents[path];
    if (!document) {
      return;
    }

    await invokeCommand("save_file", {
      request: {
        path: document.path,
        content: document.content,
      },
    });

    setDocuments(path, "savedContent", document.content);
    setDocuments(path, "dirty", false);
    setStatus(`Saved ${document.name}.`);
  }

  async function refreshCompactContext() {
    if (!workspaceRoot()) {
      return;
    }

    setIsRefreshingContext(true);
    try {
      const payload = await invokeCommand<CompactContextPayload>(
        "build_compact_context",
        {
          request: {
            root: workspaceRoot(),
            currentFile: activeDocument()?.path,
            content: activeDocument()?.content,
          },
        }
      );
      setCompactContext(payload.context);
    } catch (error) {
      setCompactContext(String(error));
    } finally {
      setIsRefreshingContext(false);
    }
  }

  async function saveCredentials() {
    setIsSavingCredentials(true);
    try {
      const payload = await invokeCommand<AgentHealthPayload>(
        "save_agent_credentials",
        {
          request: {
            openaiApiKey: credentialsForm.openaiApiKey || undefined,
            geminiApiKey: credentialsForm.geminiApiKey || undefined,
            googleApiKey: credentialsForm.googleApiKey || undefined,
            googleCloudProject: credentialsForm.googleCloudProject || undefined,
            googleCloudLocation: credentialsForm.googleCloudLocation || undefined,
            googleApplicationCredentials:
              credentialsForm.googleApplicationCredentials || undefined,
            anthropicApiKey: credentialsForm.anthropicApiKey || undefined,
            kiloApiKey: credentialsForm.kiloApiKey || undefined,
          },
        }
      );

      setAgentHealth(payload);
      syncCredentialForm(payload.credentials);
      setCredentialsForm("openaiApiKey", "");
      setCredentialsForm("geminiApiKey", "");
      setCredentialsForm("googleApiKey", "");
      setCredentialsForm("anthropicApiKey", "");
      setCredentialsForm("kiloApiKey", "");
      setStatus("Updated local agent credentials for Hematite.");
    } catch (error) {
      setStatus(`Could not save credentials: ${String(error)}`);
    } finally {
      setIsSavingCredentials(false);
    }
  }

  async function launchLogin(agent: AgentDefinition) {
    setLaunchingAgentId(agent.id);
    try {
      const message = await invokeCommand<string>("launch_agent_login", {
        request: { agentId: agent.id },
      });
      setStatus(message);
    } catch (error) {
      setStatus(`Could not start ${agent.label} login: ${String(error)}`);
    } finally {
      setLaunchingAgentId(null);
    }
  }

  async function preparePythonEnvironment() {
    if (!workspaceRoot()) {
      return;
    }

    setIsPreparingPythonEnvironment(true);
    try {
      const outcome = await invokeCommand<ProcessOutcome>(
        "prepare_python_environment",
        { root: workspaceRoot() }
      );
      setPrepareOutcome(outcome);
      await refreshPythonEnvironment(workspaceRoot());
      setStatus(
        outcome.success
          ? `Finished ${outcome.command}.`
          : `${outcome.command} returned a non-zero exit status.`
      );
    } catch (error) {
      setStatus(`Could not prepare Python environment: ${String(error)}`);
    } finally {
      setIsPreparingPythonEnvironment(false);
    }
  }

  async function runTerminalCommand() {
    const command = terminalInput().trim();
    if (!command) {
      setStatus("Write a terminal command before running it.");
      return;
    }

    if (command === "clear" || command === "cls") {
      setTerminalEntries([]);
      setTerminalInput("");
      setStatus("Cleared terminal output.");
      return;
    }

    const cwd = terminalCwd() || workspaceRoot() || workspaceInput();
    setIsRunningTerminal(true);
    setStatus(`Running terminal command: ${command}`);

    try {
      const response = await invokeCommand<TerminalCommandResponse>(
        "execute_terminal_command",
        {
          request: {
            command,
            cwd: cwd || undefined,
          },
        }
      );

      setTerminalEntries((entries) => [
        ...entries,
        {
          id: makeId(),
          command: response.command,
          stdout: response.stdout,
          stderr: response.stderr,
          cwd: response.cwd,
          success: response.success,
          timestamp: formatTime(),
        },
      ]);
      setTerminalCwd(response.cwd || cwd);
      setTerminalInput("");
      setStatus(
        response.success
          ? "Terminal command finished."
          : "Terminal command returned a non-zero exit status."
      );
    } catch (error) {
      const message = String(error);
      setTerminalEntries((entries) => [
        ...entries,
        {
          id: makeId(),
          command,
          stdout: "",
          stderr: message,
          cwd,
          success: false,
          timestamp: formatTime(),
        },
      ]);
      setStatus(`Could not run terminal command: ${message}`);
    } finally {
      setIsRunningTerminal(false);
    }
  }

  function handleCodexEvent(event: CodexFrontendEvent) {
    switch (event.kind) {
      case "agentMessageDelta": {
        const messageId = activeCodexAssistantMessageId();
        if (!messageId) {
          return;
        }
        updateChatMessage(messageId, (message) => {
          const initial = message.content === `${CODEX_AGENT_LABEL} is thinking...`;
          return {
            ...message,
            content: initial ? event.delta : `${message.content}${event.delta}`,
            status: "running",
          };
        });
        break;
      }
      case "agentMessageCompleted": {
        const messageId = activeCodexAssistantMessageId();
        if (!messageId) {
          return;
        }
        updateChatMessage(messageId, (message) => ({
          ...message,
          content: event.text || message.content,
        }));
        break;
      }
      case "approvalRequested": {
        const approval: AgentApproval = {
          requestId: event.requestId,
          agentId: "codex",
          approvalType: event.approvalType,
          reason: event.reason,
          command: event.command,
          cwd: event.cwd,
          grantRoot: event.grantRoot,
          permissions: event.permissions,
          choices: event.choices,
          state: "pending",
        };

        setChatMessages((messages) => [
          ...messages,
          {
            id: makeId(),
            role: "system",
            content: approvalTitle(approval),
            timestamp: formatTime(),
            agentLabel: "Hematite",
            status: "complete",
            approval,
          },
        ]);
        setStatus(`${CODEX_AGENT_LABEL} is waiting for approval.`);
        break;
      }
      case "approvalResolved":
        updateApprovalState(event.requestId, "resolved");
        break;
      case "turnCompleted": {
        const activeTurnId = activeCodexTurnId();
        if (activeTurnId && event.turnId && activeTurnId !== event.turnId) {
          return;
        }
        const messageId = activeCodexAssistantMessageId();
        if (messageId) {
          updateChatMessage(messageId, (message) => ({
            ...message,
            content:
              message.content === `${CODEX_AGENT_LABEL} is thinking...`
                ? event.success
                  ? `${CODEX_AGENT_LABEL} finished without textual output.`
                  : event.error || `${CODEX_AGENT_LABEL} ended without a response.`
                : message.content,
            status: event.success ? "complete" : "error",
            stderr: event.error ?? message.stderr,
          }));
        }
        setActiveCodexAssistantMessageId(null);
        setActiveCodexTurnId(null);
        setIsRunningAgent(false);
        setStatus(
          event.success
            ? `${CODEX_AGENT_LABEL} finished successfully.`
            : event.error || `${CODEX_AGENT_LABEL} returned an error.`
        );
        break;
      }
      case "error":
        setStatus(event.message);
        if (isRunningAgent() && activeCodexAssistantMessageId()) {
          const messageId = activeCodexAssistantMessageId()!;
          updateChatMessage(messageId, (message) => ({
            ...message,
            content:
              message.content === `${CODEX_AGENT_LABEL} is thinking...`
                ? event.message
                : message.content,
            status: "error",
            stderr: event.message,
          }));
          setActiveCodexAssistantMessageId(null);
          setActiveCodexTurnId(null);
          setIsRunningAgent(false);
        }
        break;
    }
  }

  function handleGeminiEvent(event: GeminiFrontendEvent) {
    switch (event.kind) {
      case "agentMessageDelta": {
        if (!activeGeminiSessionId()) {
          setActiveGeminiSessionId(event.sessionId);
        }
        const messageId = activeGeminiAssistantMessageId();
        if (!messageId) {
          return;
        }
        updateChatMessage(messageId, (message) => {
          const initial = message.content === `${GEMINI_AGENT_LABEL} is thinking...`;
          return {
            ...message,
            content: initial ? event.delta : `${message.content}${event.delta}`,
            status: "running",
          };
        });
        break;
      }
      case "approvalRequested": {
        if (!activeGeminiSessionId()) {
          setActiveGeminiSessionId(event.sessionId);
        }
        const approval: AgentApproval = {
          requestId: event.requestId,
          agentId: "gemini",
          approvalType: event.toolKind ?? "tool",
          title: event.title,
          reason: event.toolKind ? `Gemini wants to use a ${event.toolKind} tool.` : undefined,
          command: event.command,
          locations: event.locations,
          choices: event.choices,
          state: "pending",
        };

        setChatMessages((messages) => [
          ...messages,
          {
            id: makeId(),
            role: "system",
            content: approvalTitle(approval),
            timestamp: formatTime(),
            agentLabel: "Hematite",
            status: "complete",
            approval,
          },
        ]);
        setStatus(`${GEMINI_AGENT_LABEL} is waiting for approval.`);
        break;
      }
      case "approvalResolved":
        updateApprovalState(event.requestId, "resolved");
        break;
      case "promptCompleted": {
        const activeSessionId = activeGeminiSessionId();
        if (activeSessionId && event.sessionId !== activeSessionId) {
          return;
        }
        const messageId = activeGeminiAssistantMessageId();
        if (messageId) {
          updateChatMessage(messageId, (message) => ({
            ...message,
            content:
              message.content === `${GEMINI_AGENT_LABEL} is thinking...`
                ? event.success
                  ? `${GEMINI_AGENT_LABEL} finished without textual output.`
                  : event.error || `${GEMINI_AGENT_LABEL} ended without a response.`
                : message.content,
            status: event.success ? "complete" : "error",
            stderr: event.error ?? message.stderr,
          }));
        }
        setActiveGeminiAssistantMessageId(null);
        setActiveGeminiSessionId(null);
        setIsRunningAgent(false);
        setStatus(
          event.success
            ? `${GEMINI_AGENT_LABEL} finished successfully.`
            : event.error || `${GEMINI_AGENT_LABEL} returned an error.`
        );
        break;
      }
      case "error":
        setStatus(event.message);
        if (isRunningAgent() && activeGeminiAssistantMessageId()) {
          const messageId = activeGeminiAssistantMessageId()!;
          updateChatMessage(messageId, (message) => ({
            ...message,
            content:
              message.content === `${GEMINI_AGENT_LABEL} is thinking...`
                ? event.message
                : message.content,
            status: "error",
            stderr: event.message,
          }));
          setActiveGeminiAssistantMessageId(null);
          setActiveGeminiSessionId(null);
          setIsRunningAgent(false);
        }
        break;
    }
  }

  async function respondToApproval(
    approval: AgentApproval,
    optionId: string
  ) {
    const agentLabel = approval.agentId === "codex" ? CODEX_AGENT_LABEL : GEMINI_AGENT_LABEL;
    updateApprovalState(approval.requestId, "submitted");
    try {
      if (approval.agentId === "codex") {
        await invokeCommand("respond_to_codex_approval", {
          request: {
            requestId: approval.requestId,
            decision: optionId,
          },
        });
      } else {
        await invokeCommand("respond_to_gemini_approval", {
          request: {
            requestId: approval.requestId,
            optionId,
          },
        });
      }
      updateApprovalState(approval.requestId, "resolved");
      setStatus(`Sent the approval response back to ${agentLabel}.`);
    } catch (error) {
      updateApprovalState(approval.requestId, "pending");
      setStatus(`Could not send the approval response: ${String(error)}`);
    }
  }

  async function runCliAgentChat() {
    if (!workspaceRoot()) {
      setStatus("Open a workspace before starting an agent chat.");
      return;
    }

    const agent = selectedAgent();
    const status = selectedAgentStatus();
    if (!status?.available) {
      setStatus(`${agent.label} CLI is not available on PATH.`);
      return;
    }
    if (status.authState !== "ready") {
      setStatus(`${agent.label} is not ready yet. Finish login or add credentials first.`);
      return;
    }

    const content = chatDraft().trim();
    if (!content) {
      setStatus("Write a message before running a coding agent.");
      return;
    }

    const prompt = buildConversationPrompt(chatMessages(), content);
    const pendingMessageId = makeId();

    setChatMessages((messages) => [
      ...messages,
      {
        id: makeId(),
        role: "user",
        content,
        timestamp: formatTime(),
        status: "complete",
      },
      {
        id: pendingMessageId,
        role: "assistant",
        content: `${agent.label} is thinking...`,
        timestamp: formatTime(),
        agentId: agent.id,
        agentLabel: agent.label,
        status: "running",
      },
    ]);
    setChatDraft("");
    setIsRunningAgent(true);
    setStatus(`Running ${agent.label}...`);

    try {
      const response = await invokeCommand<AgentRunResponse>("run_agent", {
        request: {
          root: workspaceRoot(),
          binary: agent.binary,
          args: agent.args,
          stdinPrompt: agent.promptMode === "stdin",
          prompt,
          includeCompactContext: includeCompactContext(),
          currentFile: activeDocument()?.path,
          content: activeDocument()?.content,
          model: selectedAgentModel()?.id || undefined,
        },
      });

      if (response.context) {
        setCompactContext(response.context);
      }

      updateChatMessage(pendingMessageId, (message) => ({
        ...message,
        content:
          response.stdout ||
          (response.success
            ? `${agent.label} finished without textual output. Open details for the command trace.`
            : response.stderr || `${agent.label} returned a non-zero exit status.`),
        timestamp: formatTime(),
        status: response.success ? "complete" : "error",
        command: response.command.join(" "),
        prompt: response.prompt,
        stderr: response.stderr || undefined,
      }));

      setStatus(
        response.success
          ? `${agent.label} finished successfully.`
          : `${agent.label} returned a non-zero exit status.`
      );
    } catch (error) {
      const message = String(error);
      updateChatMessage(pendingMessageId, (chatMessage) => ({
        ...chatMessage,
        content: message,
        timestamp: formatTime(),
        status: "error",
        stderr: message,
      }));
      setStatus(`Failed to launch ${agent.label}.`);
    } finally {
      setIsRunningAgent(false);
    }
  }

  async function runCodexChat() {
    if (!workspaceRoot()) {
      setStatus("Open a workspace before starting an agent chat.");
      return;
    }

    const status = selectedAgentStatus();
    if (!status?.available) {
      setStatus("OpenAI Codex CLI is not available on PATH.");
      return;
    }
    if (status.authState !== "ready") {
      setStatus("OpenAI Codex is not ready yet. Finish login or add credentials first.");
      return;
    }

    const content = chatDraft().trim();
    if (!content) {
      setStatus("Write a message before running a coding agent.");
      return;
    }

    const pendingMessageId = makeId();
    setChatMessages((messages) => [
      ...messages,
      {
        id: makeId(),
        role: "user",
        content,
        timestamp: formatTime(),
        status: "complete",
      },
      {
        id: pendingMessageId,
        role: "assistant",
        content: `${CODEX_AGENT_LABEL} is thinking...`,
        timestamp: formatTime(),
        agentId: "codex",
        agentLabel: CODEX_AGENT_LABEL,
        status: "running",
      },
    ]);
    setChatDraft("");
    setIsRunningAgent(true);
    setActiveCodexAssistantMessageId(pendingMessageId);
    setActiveCodexTurnId(null);
    setStatus(`Running ${selectedAgent().label}...`);

    try {
      const response = await invokeCommand<CodexTurnResponse>("start_codex_turn", {
        request: {
          root: workspaceRoot(),
          prompt: content,
          includeCompactContext: includeCompactContext(),
          currentFile: activeDocument()?.path,
          content: activeDocument()?.content,
          model: selectedAgentModel()?.id || undefined,
        },
      });

      if (response.context) {
        setCompactContext(response.context);
      }

      setActiveCodexTurnId(response.turnId);
      updateChatMessage(pendingMessageId, (message) => ({
        ...message,
        prompt: response.prompt,
      }));
    } catch (error) {
      const message = String(error);
      updateChatMessage(pendingMessageId, (chatMessage) => ({
        ...chatMessage,
        content: message,
        timestamp: formatTime(),
        status: "error",
        stderr: message,
      }));
      setActiveCodexAssistantMessageId(null);
      setActiveCodexTurnId(null);
      setIsRunningAgent(false);
      setStatus("Failed to launch OpenAI Codex.");
    }
  }

  async function runGeminiChat() {
    if (!workspaceRoot()) {
      setStatus("Open a workspace before starting an agent chat.");
      return;
    }

    const status = selectedAgentStatus();
    if (!status?.available) {
      setStatus("Gemini CLI is not available on PATH.");
      return;
    }
    if (status.authState !== "ready") {
      setStatus("Gemini CLI is not ready yet. Finish login or add credentials first.");
      return;
    }

    const content = chatDraft().trim();
    if (!content) {
      setStatus("Write a message before running a coding agent.");
      return;
    }

    const pendingMessageId = makeId();
    setChatMessages((messages) => [
      ...messages,
      {
        id: makeId(),
        role: "user",
        content,
        timestamp: formatTime(),
        status: "complete",
      },
      {
        id: pendingMessageId,
        role: "assistant",
        content: `${GEMINI_AGENT_LABEL} is thinking...`,
        timestamp: formatTime(),
        agentId: "gemini",
        agentLabel: GEMINI_AGENT_LABEL,
        status: "running",
      },
    ]);
    setChatDraft("");
    setIsRunningAgent(true);
    setActiveGeminiAssistantMessageId(pendingMessageId);
    setActiveGeminiSessionId(null);
    setStatus(`Running ${GEMINI_AGENT_LABEL}...`);

    try {
      const response = await invokeCommand<GeminiTurnResponse>("start_gemini_turn", {
        request: {
          root: workspaceRoot(),
          prompt: content,
          includeCompactContext: includeCompactContext(),
          currentFile: activeDocument()?.path,
          content: activeDocument()?.content,
          model: selectedAgentModel()?.id || undefined,
        },
      });

      if (response.context) {
        setCompactContext(response.context);
      }

      if (activeGeminiAssistantMessageId() === pendingMessageId) {
        setActiveGeminiSessionId(response.sessionId);
        updateChatMessage(pendingMessageId, (message) => ({
          ...message,
          prompt: response.prompt,
        }));
      }
    } catch (error) {
      const message = String(error);
      updateChatMessage(pendingMessageId, (chatMessage) => ({
        ...chatMessage,
        content: message,
        timestamp: formatTime(),
        status: "error",
        stderr: message,
      }));
      setActiveGeminiAssistantMessageId(null);
      setActiveGeminiSessionId(null);
      setIsRunningAgent(false);
      setStatus("Failed to launch Gemini CLI.");
    }
  }

  async function runAgentChat() {
    if (selectedAgent().id === "codex") {
      await runCodexChat();
      return;
    }

    if (selectedAgent().id === "gemini") {
      await runGeminiChat();
      return;
    }

    await runCliAgentChat();
  }

  async function installMissingPackages() {
    const document = activeDocument();
    if (!workspaceRoot() || !document || document.language !== "python") {
      return;
    }

    setIsInstallingMissingPackages(true);
    setStatus(
      `Installing ${missingImports().length} missing Python package${
        missingImports().length === 1 ? "" : "s"
      } with uv...`
    );

    try {
      const response = await invokeCommand<PythonImportResponse>(
        "install_missing_python_imports",
        {
          request: {
            root: workspaceRoot(),
            filePath: document.path,
            source: document.content,
            autoInstall: true,
          },
        }
      );

      if (!documents[document.path]) {
        return;
      }

      setDocuments(document.path, "diagnostics", response.diagnostics);
      setDocuments(document.path, "installEvents", response.events);
      if (response.environmentPath) {
        setVenvPathHint(response.environmentPath);
      }

      await refreshPythonEnvironment(workspaceRoot());

      const successCount = response.events.filter((event) => event.success).length;
      const failedCount = response.events.filter((event) => !event.success).length;

      if (response.events.length === 0) {
        setStatus("No installable missing imports were found in the active Python file.");
      } else if (failedCount === 0) {
        setStatus(
          `Installed ${successCount} Python package${successCount === 1 ? "" : "s"} with uv.`
        );
      } else {
        setStatus(
          `uv installed ${successCount} package${
            successCount === 1 ? "" : "s"
          } and left ${failedCount} unresolved.`
        );
      }
    } catch (error) {
      setStatus(`Could not install missing Python packages: ${String(error)}`);
    } finally {
      setIsInstallingMissingPackages(false);
    }
  }

  async function runPythonTooling(action: PythonToolingAction) {
    const document = activeDocument();
    const root = workspaceRoot();
    if (!root || !document || document.language !== "python") {
      setStatus("Open a Python file before running Ruff or ty.");
      return;
    }

    setRunningPythonToolingAction(action);
    setStatus("Running Python tooling...");

    try {
      if (document.dirty) {
        await saveActiveFile(document.path);
      }

      const outcome = await invokeCommand<ProcessOutcome>("run_python_tooling_action", {
        request: {
          root,
          filePath: document.path,
          action,
        },
      });
      setPythonToolingOutcome(outcome);

      if (
        outcome.success &&
        (action === "format" || action === "fixAll" || action === "organizeImports")
      ) {
        const refreshed = await invokeCommand<FileDocument>("read_file", { path: document.path });
        if (documents[document.path]) {
          setDocuments(document.path, {
            ...documents[document.path],
            ...refreshed,
            savedContent: refreshed.content,
            dirty: false,
            diagnostics: [],
            installEvents: [],
          });
        }
      }

      if (documents[document.path]) {
        const response = await invokeCommand<PythonImportResponse>("analyze_python_imports", {
          request: {
            root,
            filePath: document.path,
            source: documents[document.path].content,
            autoInstall: false,
          },
        });
        if (documents[document.path]) {
          setDocuments(document.path, "diagnostics", response.diagnostics);
          setDocuments(document.path, "installEvents", response.events);
        }
      }

      setStatus(
        outcome.success
          ? `Finished ${outcome.command}.`
          : `${outcome.command} returned a non-zero exit status.`
      );
    } catch (error) {
      setStatus(`Could not run Python tooling: ${String(error)}`);
    } finally {
      setRunningPythonToolingAction(null);
    }
  }

  async function runRustTooling(action: RustToolingAction) {
    const document = activeDocument();
    const root = workspaceRoot();
    if (!root || !document || document.language !== "rust") {
      setStatus("Open a Rust file before running Cargo or rustfmt.");
      return;
    }

    setRunningRustToolingAction(action);
    setStatus("Running Rust tooling...");

    try {
      if (document.dirty) {
        await saveActiveFile(document.path);
      }

      const outcome = await invokeCommand<ProcessOutcome>("run_rust_tooling_action", {
        request: {
          root,
          filePath: document.path,
          action,
        },
      });
      setRustToolingOutcome(outcome);

      if (outcome.success && action === "format") {
        const refreshed = await invokeCommand<FileDocument>("read_file", { path: document.path });
        if (documents[document.path]) {
          setDocuments(document.path, {
            ...documents[document.path],
            ...refreshed,
            savedContent: refreshed.content,
            dirty: false,
            diagnostics: documents[document.path].diagnostics,
            installEvents: documents[document.path].installEvents,
          });
        }
      }

      if (documents[document.path] && outcome.diagnostics) {
        setDocuments(document.path, "diagnostics", outcome.diagnostics);
      }

      setStatus(
        outcome.success
          ? `Finished ${outcome.command}.`
          : `${outcome.command} returned a non-zero exit status.`
      );
    } catch (error) {
      setStatus(`Could not run Rust tooling: ${String(error)}`);
    } finally {
      setRunningRustToolingAction(null);
    }
  }

  function closeTab(path: string) {
    const nextTabs = openTabs().filter((tab) => tab !== path);
    setOpenTabs(nextTabs);
    if (activeTab() === path) {
      setActiveTab(nextTabs[nextTabs.length - 1] ?? null);
    }
  }

  function toggleDirectory(entry: FileEntry) {
    if (!entry.isDir) {
      return;
    }

    setExpandedDirectories((current) => {
      const next = new Set(current);
      if (next.has(entry.path)) {
        next.delete(entry.path);
      } else {
        next.add(entry.path);
        void loadDirectory(entry.path);
      }
      return next;
    });
  }

  function handleMenuAction(id: string) {
    switch (id) {
      case "file.new_file":
        beginCreateNewFile();
        break;
      case "file.open_folder":
        void browseWorkspace();
        break;
      case "file.save":
        if (activeTab()) {
          void saveActiveFile(activeTab());
        }
        break;
      case "file.close_tab":
        if (activeTab()) {
          closeTab(activeTab()!);
        }
        break;
      case "file.new_chat":
        setUtilityTab("chat");
        void startNewChat();
        break;
      case "view.focus_chat":
        setUtilityTab("chat");
        break;
      case "view.focus_access":
        setUtilityTab("access");
        break;
      case "view.focus_project":
        setUtilityTab("project");
        break;
      case "view.focus_problems":
        setUtilityTab("problems");
        break;
      case "view.focus_outline":
        setUtilityTab("outline");
        break;
      case "view.refresh_context":
        void refreshCompactContext();
        break;
      case "view.toggle_terminal":
        toggleTerminal();
        break;
      case "help.refresh_agents":
        void refreshAgentHealth();
        break;
      default:
        break;
    }
  }

  createEffect(() => {
    if (!activeDocument()) {
      setCursorPosition({ line: 1, column: 1 });
    }
  });

  createEffect(() => {
    const document = activeDocument();
    if (!document) {
      setSymbols([]);
      setEditorSemantics(EMPTY_EDITOR_SEMANTICS);
      return;
    }
    const path = document.path;
    const content = document.content;

    const timeout = window.setTimeout(async () => {
      try {
        const nextSymbols = await invokeCommand<SymbolEntry[]>("extract_symbols", {
          path,
          content,
        });
        if (activeTab() === path) {
          setSymbols(nextSymbols);
        }
      } catch {
        if (activeTab() === path) {
          setSymbols([]);
        }
      }
    }, 120);

    onCleanup(() => window.clearTimeout(timeout));
  });

  createEffect(() => {
    const document = activeDocument();
    if (!document) {
      setEditorSemantics(EMPTY_EDITOR_SEMANTICS);
      return;
    }
    const path = document.path;
    const content = document.content;

    const timeout = window.setTimeout(async () => {
      try {
        const nextSemantics = await invokeCommand<EditorSemanticsPayload>(
          "analyze_editor_semantics",
          {
            path,
            content,
          }
        );

        if (activeTab() === path) {
          setEditorSemantics(nextSemantics);
        }
      } catch {
        if (activeTab() === path) {
          setEditorSemantics(EMPTY_EDITOR_SEMANTICS);
        }
      }
    }, document.language === "python" ? 620 : 170);

    onCleanup(() => window.clearTimeout(timeout));
  });

  createEffect(() => {
    const document = activeDocument();
    const root = workspaceRoot();

    if (!document || document.language !== "python" || !root) {
      if (document && document.language !== "python") {
        setDocuments(document.path, "diagnostics", []);
        setDocuments(document.path, "installEvents", []);
      }
      return;
    }

    const timeout = window.setTimeout(async () => {
      try {
        const response = await invokeCommand<PythonImportResponse>(
          "analyze_python_imports",
          {
            request: {
              root,
              filePath: document.path,
              source: document.content,
              autoInstall: false,
            },
          }
        );

        if (activeTab() !== document.path || !documents[document.path]) {
          return;
        }

        setDocuments(document.path, "diagnostics", response.diagnostics);
        setDocuments(document.path, "installEvents", response.events);
        if (response.environmentPath) {
          setVenvPathHint(response.environmentPath);
        }
      } catch (error) {
        if (activeTab() === document.path && documents[document.path]) {
          setDocuments(document.path, "diagnostics", [
            {
              module: "uv",
              from: 0,
              to: 0,
              line: 0,
              column: 0,
              severity: "warning",
              message: String(error),
            },
          ]);
        }
      }
    }, 1000);

    onCleanup(() => window.clearTimeout(timeout));
  });

  createEffect(() => {
    chatMessages();
    queueMicrotask(() => {
      chatTimelineRef?.scrollTo({
        top: chatTimelineRef.scrollHeight,
        behavior: "smooth",
      });
    });
  });

  createEffect(() => {
    terminalEntries();
    queueMicrotask(() => {
      terminalLogRef?.scrollTo({
        top: terminalLogRef.scrollHeight,
        behavior: "smooth",
      });
    });
  });

  createEffect(() => {
    const root = workspaceRoot();
    const document = activeDocument();
    const path = document?.path;
    document?.content;
    document?.savedContent;
    const dirty = document?.dirty;

    if (!root || !path || isHydratingWorkspaceState()) {
      return;
    }

    const timeout = window.setTimeout(() => {
      persistWorkspaceSession(root);
    }, dirty ? 420 : 180);

    onCleanup(() => window.clearTimeout(timeout));
  });

  createEffect(() => {
    const root = workspaceRoot();
    activeTab();
    expandedDirectories();

    if (!root || isHydratingWorkspaceState()) {
      return;
    }

    persistWorkspaceSession(root);
  });

  onMount(async () => {
    const unlisten = await listen<MenuEventPayload>("hematite://menu", (event) => {
      handleMenuAction(event.payload.id);
    });
    onCleanup(() => void unlisten());

    const unlistenCodex = await listen<CodexFrontendEvent>("hematite://codex", (event) => {
      handleCodexEvent(event.payload);
    });
    onCleanup(() => void unlistenCodex());

    const unlistenGemini = await listen<GeminiFrontendEvent>("hematite://gemini", (event) => {
      handleGeminiEvent(event.payload);
    });
    onCleanup(() => void unlistenGemini());

    try {
      const storedState = await invokeCommand<string | null>("load_ui_state");
      if (storedState) {
        window.localStorage.setItem(PERSISTED_APP_STATE_KEY, storedState);
      }
    } catch {
      // Fall back to localStorage-only restore if backend persistence is unavailable.
    }

    const payload = await invokeCommand<BootstrapPayload>("bootstrap");
    const persisted = loadPersistedAppState();
    const initialWorkspace = persisted.lastWorkspace.trim() || payload.defaultRoot;
    setWorkspaceInput(initialWorkspace);
    setStatus(`Ready. Opening ${basename(initialWorkspace)}...`);

    const editorPreloadHandle = window.setTimeout(() => {
      void import("./components/CodeEditor");
    }, 120);

    const bootHandle = window.setTimeout(() => {
      void (async () => {
        const opened = await openWorkspace(initialWorkspace);
        if (!opened && initialWorkspace !== payload.defaultRoot) {
          setWorkspaceInput(payload.defaultRoot);
          setStatus(`Falling back to ${basename(payload.defaultRoot)}...`);
          await openWorkspace(payload.defaultRoot);
        }
      })();
    }, 90);

    const toolHandle = window.setTimeout(() => {
      void refreshToolStatuses();
    }, 650);

    const refreshHandle = window.setTimeout(() => {
      void refreshAgentHealth().catch((error) => {
        setStatus(`Could not refresh agent access: ${String(error)}`);
      });
    }, 1350);

    onCleanup(() => window.clearTimeout(editorPreloadHandle));
    onCleanup(() => window.clearTimeout(bootHandle));
    onCleanup(() => window.clearTimeout(toolHandle));
    onCleanup(() => window.clearTimeout(refreshHandle));
  });

  const renderTree = (entry: FileEntry, depth = 0) => (
    <TreeNode
      entry={entry}
      depth={depth}
      activePath={activeTab()}
      isExpanded={expandedDirectories().has(entry.path)}
      directoryState={directories[entry.path]}
      onToggleDirectory={toggleDirectory}
      onOpenFile={(path) => void openFile(path)}
    >
      <For each={directories[entry.path]?.entries ?? []}>
        {(child) => renderTree(child, depth + 1)}
      </For>
    </TreeNode>
  );

  return (
    <main
      ref={shellRef}
      class={`workbench-shell${isTerminalVisible() ? "" : " terminal-hidden"}`}
      style={layoutStyle()}
    >
      <header class="commandbar">
        <div class="brand-cluster">
          <div class="brand-mark">H</div>
          <div class="brand-copy">
            <div class="brand-name">Hematite</div>
            <div class="brand-subtitle">
              Lightweight desktop IDE with agent access, tree-sitter context, and
              uv-backed Python management
            </div>
          </div>
        </div>

        <div class="workspace-command-row">
          <label class="path-field">
            <span>Workspace</span>
            <input
              value={workspaceInput()}
              placeholder="Open a folder"
              onInput={(event) => setWorkspaceInput(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  void openWorkspace(workspaceInput());
                }
              }}
            />
          </label>

          <div class="command-actions">
            <button type="button" class="command-button" onClick={() => void browseWorkspace()}>
              Open Folder...
            </button>
            <button
              type="button"
              class="command-button"
              disabled={isOpeningWorkspace()}
              onClick={() => void openWorkspace(workspaceInput())}
            >
              {isOpeningWorkspace() ? "Scanning..." : "Open"}
            </button>
            <button
              type="button"
              class="command-button"
              disabled={!activeDocument() || !activeDocument()!.dirty}
              onClick={() => void saveActiveFile()}
            >
              Save
            </button>
            <button
              type="button"
              class="command-button accent"
              disabled={isRefreshingContext()}
              onClick={() => void refreshCompactContext()}
            >
              {isRefreshingContext() ? "Compacting..." : "Refresh Context"}
            </button>
            <button
              type="button"
              class={`command-button${isTerminalVisible() ? " accent" : ""}`}
              onClick={() => toggleTerminal()}
            >
              {isTerminalVisible() ? "Hide Terminal" : "Show Terminal"}
            </button>
          </div>
        </div>

        <div class="status-pills">
          <For each={tools()}>
            {(tool) => (
              <div class={`pill${tool.available ? " available" : ""}`}>
                <span class="pill-dot" />
                <span>{tool.label}</span>
              </div>
            )}
          </For>
          <div class={`pill${missingImports().length ? " available" : ""}`}>
            <span class="pill-dot" />
            <span>{pythonInstallModeLabel()}</span>
          </div>
        </div>
      </header>

      <div ref={gridRef} class="workbench-grid">
        <nav class="activity-bar" aria-label="Primary workbench areas">
          <button type="button" class="activity-item active" title="Explorer">
            EX
          </button>
          <button
            type="button"
            class={`activity-item${utilityTab() === "chat" ? " active" : ""}`}
            title="Agent Workflows"
            onClick={() => setUtilityTab("chat")}
          >
            AI
          </button>
          <button
            type="button"
            class={`activity-item${utilityTab() === "access" ? " active" : ""}`}
            title="Agent Access"
            onClick={() => setUtilityTab("access")}
          >
            AC
          </button>
          <button
            type="button"
            class={`activity-item${utilityTab() === "project" ? " active" : ""}`}
            title="Project Automation"
            onClick={() => setUtilityTab("project")}
          >
            PK
          </button>
          <button
            type="button"
            class={`activity-item${utilityTab() === "problems" ? " active" : ""}`}
            title="Problems"
            onClick={() => setUtilityTab("problems")}
          >
            PR
            <Show when={activeDiagnosticStats().total}>
              <span class={`activity-badge ${diagnosticBadgeTone(activeDiagnosticStats())}`}>
                {activeDiagnosticStats().total}
              </span>
            </Show>
          </button>
          <button
            type="button"
            class={`activity-item${utilityTab() === "outline" ? " active" : ""}`}
            title="Outline"
            onClick={() => setUtilityTab("outline")}
          >
            OL
          </button>
          <button
            type="button"
            class={`activity-item${isTerminalVisible() ? " active" : ""}`}
            title="Terminal"
            onClick={() => toggleTerminal()}
          >
            &gt;_
          </button>
        </nav>

        <aside class="explorer-pane">
          <div class="pane-header">
            <div>
              <div class="pane-title">Explorer</div>
              <div class="pane-caption">{basename(workspaceRoot()) || "Hematite"}</div>
            </div>
            <Show
              when={isCreatingFile()}
              fallback={
                <div class="inline-button-row">
                  <button type="button" class="pane-button" onClick={() => beginCreateNewFile()}>
                    New File
                  </button>
                  <button type="button" class="pane-button" onClick={() => void browseWorkspace()}>
                    Browse
                  </button>
                </div>
              }
            >
              <div class="explorer-create-inline">
                <input
                  ref={newFileInputRef}
                  type="text"
                  name="newFilePath"
                  autocomplete="off"
                  spellcheck={false}
                  value={newFileDraft()}
                  aria-label="New file path"
                  placeholder="src/new_file.ts"
                  onInput={(event) => setNewFileDraft(event.currentTarget.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      void submitCreateNewFile();
                    } else if (event.key === "Escape") {
                      event.preventDefault();
                      cancelCreateNewFile();
                    }
                  }}
                />
                <div class="explorer-create-actions">
                  <button
                    type="button"
                    class="pane-button pane-button-compact active"
                    onClick={() => void submitCreateNewFile()}
                  >
                    Create
                  </button>
                  <button
                    type="button"
                    class="pane-button pane-button-compact"
                    onClick={() => cancelCreateNewFile()}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            </Show>
          </div>

          <div class="pane-note">
            Hidden by default: <code>node_modules</code>, <code>target</code>,{" "}
            <code>.venv</code>, <code>dist</code>
          </div>

          <div class="explorer-scroll">
            <Show when={workspaceRoot()} fallback={<div class="empty-note">Open a workspace to begin.</div>}>
              <Show
                when={explorerEntries().length}
                fallback={<div class="empty-note">No visible files in this directory.</div>}
              >
                <ul class="tree-root">
                  <For each={explorerEntries()}>
                    {(entry) => renderTree(entry)}
                  </For>
                </ul>
              </Show>
            </Show>
          </div>
        </aside>

        <section class="editor-pane">
          <div class="tabs-bar">
            <div class="tabs">
              <For each={openTabs()}>
                {(path) => {
                  const tabStats = createMemo(() =>
                    summarizeDiagnostics(documents[path]?.diagnostics ?? [])
                  );

                  return (
                    <div class={`tab${activeTab() === path ? " active" : ""}`}>
                      <button type="button" class="tab-open" onClick={() => setActiveTab(path)}>
                        <span class="tab-name">{basename(path)}</span>
                        <Show when={documents[path]?.dirty}>
                          <span class="tab-dirty">*</span>
                        </Show>
                        <Show when={tabStats().total}>
                          <span
                            class={`tab-diagnostic-badge ${diagnosticBadgeTone(tabStats())}`}
                            title={`${tabStats().errors} error(s), ${tabStats().warnings} warning(s)`}
                          >
                            {tabStats().errors || tabStats().warnings || tabStats().info}
                          </span>
                        </Show>
                      </button>
                      <button type="button" class="tab-close" onClick={() => closeTab(path)}>
                        x
                      </button>
                    </div>
                  );
                }}
              </For>
            </div>

            <div class="editor-meta">
              <Show when={activeDocument()}>
                <span>{activeDocument()?.language}</span>
                <span>{activeDocument()?.dirty ? "unsaved" : "saved"}</span>
                <button
                  type="button"
                  class={`editor-tool-button${lineWrapping() ? " active" : ""}`}
                  onClick={() => setLineWrapping((value) => !value)}
                >
                  Wrap
                </button>
                <button
                  type="button"
                  class={`editor-tool-button ${diagnosticBadgeTone(activeDiagnosticStats())}`}
                  onClick={() => setUtilityTab("problems")}
                >
                  {activeDiagnosticStats().total
                    ? `${activeDiagnosticStats().total} Problems`
                    : "No Problems"}
                </button>
              </Show>
            </div>
          </div>

          <div class="editor-breadcrumb">
            <Show when={activeDocument()} fallback={<span>No file selected</span>}>
              <span class="breadcrumb-path">{activeDocument()?.path}</span>
              <div class="editor-inline-actions">
                <span>
                  Ln {cursorPosition().line}, Col {cursorPosition().column}
                </span>
                <Show when={activeDiagnosticSources().length}>
                  <For each={activeDiagnosticSources()}>
                    {(source) => (
                      <span class="source-chip">
                        {source.label} {source.count}
                      </span>
                    )}
                  </For>
                </Show>
              </div>
            </Show>
          </div>

          <Show
            when={activeDocument()}
            fallback={
              <div class="empty-editor">
                <div class="empty-editor-title">Pick a file to start editing</div>
                <div class="empty-editor-copy">
                  Hematite keeps the scrolling local to each pane, so the editor,
                  explorer, and utility panel all stay tidy and independent.
                </div>
              </div>
            }
          >
            <div class="editor-frame">
              <Suspense
                fallback={
                  <div class="empty-editor">
                    <div class="empty-editor-title">Loading editor...</div>
                    <div class="empty-editor-copy">
                      The editor runtime is split out to keep startup light.
                    </div>
                  </div>
                }
              >
                <LazyCodeEditor
                  value={activeDocument()!.content}
                  path={activeDocument()!.path}
                  diagnostics={activeCodeMirrorDiagnostics()}
                  semanticTokens={editorSemantics().tokens}
                  lineWrapping={lineWrapping()}
                  onHover={requestEditorHover}
                  jumpToLine={jumpToLine()}
                  onChange={(value) => {
                    const path = activeTab();
                    if (!path || !documents[path]) {
                      return;
                    }
                    setDocuments(path, "content", value);
                    setDocuments(path, "dirty", value !== documents[path].savedContent);
                  }}
                  onCursorChange={(line, column) => setCursorPosition({ line, column })}
                  onSave={() => void saveActiveFile()}
                />
              </Suspense>
            </div>
          </Show>
        </section>

        <aside class="utility-pane">
          <div class="utility-tabs">
            <button
              type="button"
              class={`utility-tab${utilityTab() === "chat" ? " active" : ""}`}
              onClick={() => setUtilityTab("chat")}
            >
              Agents
            </button>
            <button
              type="button"
              class={`utility-tab${utilityTab() === "access" ? " active" : ""}`}
              onClick={() => setUtilityTab("access")}
            >
              Access
            </button>
            <button
              type="button"
              class={`utility-tab${utilityTab() === "project" ? " active" : ""}`}
              onClick={() => setUtilityTab("project")}
            >
              Project
            </button>
            <button
              type="button"
              class={`utility-tab${utilityTab() === "problems" ? " active" : ""}`}
              onClick={() => setUtilityTab("problems")}
            >
              Problems
              <Show when={activeDiagnosticStats().total}>
                <span class={`utility-tab-badge ${diagnosticBadgeTone(activeDiagnosticStats())}`}>
                  {activeDiagnosticStats().total}
                </span>
              </Show>
            </button>
            <button
              type="button"
              class={`utility-tab${utilityTab() === "outline" ? " active" : ""}`}
              onClick={() => setUtilityTab("outline")}
            >
              Outline
            </button>
          </div>

          <div class={`utility-scroll${utilityTab() === "chat" ? " chat-mode" : ""}`}>
            <Show when={utilityTab() === "access"}>
              <section class="panel-section no-divider">
                <div class="pane-header compact">
                  <div>
                    <div class="pane-title">Agent Access</div>
                    <div class="pane-caption">
                      Check CLI availability, login state, and local credentials
                    </div>
                  </div>
                  <button
                    type="button"
                    class="pane-button"
                    onClick={() => void refreshAgentHealth()}
                  >
                    Refresh
                  </button>
                </div>

                <div class="agent-card-list">
                  <For each={AGENTS}>
                    {(agent) => {
                      const status = createMemo(() =>
                        agentHealth()?.agents.find((entry) => entry.id === agent.id)
                      );

                      return (
                        <article class="agent-card compact">
                          <div class="agent-card-head">
                            <div>
                              <div class="agent-card-title">{agent.label}</div>
                              <div class="agent-card-copy">{agent.description}</div>
                            </div>
                            <span class={`state-badge ${badgeTone(status()?.authState)}`}>
                              {status()?.authState ?? "checking"}
                            </span>
                          </div>

                          <div class="agent-card-meta">
                            <span>{status()?.summary ?? "Checking access..."}</span>
                            <Show when={status()?.authSource}>
                              <span>Source: {status()?.authSource}</span>
                            </Show>
                          </div>

                          <Show when={agent.models?.length}>
                            <label class="model-select-field card-model-select">
                              <span>Model</span>
                              <select
                                value={
                                  selectedAgentModels[agent.id] ??
                                  defaultAgentModel(agent)?.id ??
                                  ""
                                }
                                onChange={(event) =>
                                  setSelectedAgentModels(
                                    agent.id,
                                    event.currentTarget.value
                                  )
                                }
                              >
                                <For each={agent.models ?? []}>
                                  {(model) => <option value={model.id}>{model.label}</option>}
                                </For>
                              </select>
                            </label>
                          </Show>

                          <div class="agent-card-actions">
                            <button
                              type="button"
                              class={`pane-button${
                                selectedAgentId() === agent.id ? " active" : ""
                              }`}
                              onClick={() => {
                                setSelectedAgentId(agent.id);
                                setStatus(`Selected ${agent.label}.`);
                              }}
                            >
                              {selectedAgentId() === agent.id ? "Selected" : "Use"}
                            </button>
                            <button
                              type="button"
                              class="pane-button"
                              disabled={!status()?.available || launchingAgentId() === agent.id}
                              onClick={() => void launchLogin(agent)}
                            >
                              {launchingAgentId() === agent.id ? "Launching..." : "Open login"}
                            </button>
                          </div>
                        </article>
                      );
                    }}
                  </For>
                </div>

                <details
                  class="settings-disclosure"
                  open={selectedAgentStatus()?.authState !== "ready"}
                >
                  <summary>Credentials and local environment overrides</summary>
                  <div class="settings-disclosure-body">
                    <div class="disclosure-copy">
                      Leave fields blank to keep using whatever is already in your
                      shell environment. Hematite stores values locally and injects
                      them into agent and terminal runs when needed.
                    </div>

                    <div class="form-grid">
                      <label class="form-field">
                        <span>OpenAI API key</span>
                        <input
                          value={credentialsForm.openaiApiKey}
                          placeholder={
                            agentHealth()?.credentials.hasOpenaiApiKey
                              ? "Already stored or provided via environment"
                              : "sk-..."
                          }
                          onInput={(event) =>
                            setCredentialsForm("openaiApiKey", event.currentTarget.value)
                          }
                        />
                      </label>

                      <label class="form-field">
                        <span>Gemini API key</span>
                        <input
                          value={credentialsForm.geminiApiKey}
                          placeholder={
                            agentHealth()?.credentials.hasGeminiApiKey
                              ? "Already stored or provided via environment"
                              : "AIza..."
                          }
                          onInput={(event) =>
                            setCredentialsForm("geminiApiKey", event.currentTarget.value)
                          }
                        />
                      </label>

                      <label class="form-field">
                        <span>Google API key</span>
                        <input
                          value={credentialsForm.googleApiKey}
                          placeholder={
                            agentHealth()?.credentials.hasGoogleApiKey
                              ? "Already stored or provided via environment"
                              : "Vertex or express mode key"
                          }
                          onInput={(event) =>
                            setCredentialsForm("googleApiKey", event.currentTarget.value)
                          }
                        />
                      </label>

                      <label class="form-field">
                        <span>Google Cloud project</span>
                        <input
                          value={credentialsForm.googleCloudProject}
                          onInput={(event) =>
                            setCredentialsForm("googleCloudProject", event.currentTarget.value)
                          }
                        />
                      </label>

                      <label class="form-field">
                        <span>Google Cloud location</span>
                        <input
                          value={credentialsForm.googleCloudLocation}
                          onInput={(event) =>
                            setCredentialsForm("googleCloudLocation", event.currentTarget.value)
                          }
                        />
                      </label>

                      <div class="form-field">
                        <span>Google service account JSON</span>
                        <div class="inline-input-row">
                          <input
                            value={credentialsForm.googleApplicationCredentials}
                            onInput={(event) =>
                              setCredentialsForm(
                                "googleApplicationCredentials",
                                event.currentTarget.value
                              )
                            }
                          />
                          <button
                            type="button"
                            class="pane-button"
                            onClick={() => void browseServiceAccountFile()}
                          >
                            Browse
                          </button>
                        </div>
                      </div>

                      <label class="form-field">
                        <span>Anthropic API key</span>
                        <input
                          value={credentialsForm.anthropicApiKey}
                          placeholder={
                            agentHealth()?.credentials.hasAnthropicApiKey
                              ? "Already stored or provided via environment"
                              : "sk-ant-..."
                          }
                          onInput={(event) =>
                            setCredentialsForm("anthropicApiKey", event.currentTarget.value)
                          }
                        />
                      </label>

                      <label class="form-field">
                        <span>Kilo API key</span>
                        <input
                          value={credentialsForm.kiloApiKey}
                          placeholder={
                            agentHealth()?.credentials.hasKiloApiKey
                              ? "Already stored or provided via environment"
                              : "KILO_API_KEY"
                          }
                          onInput={(event) =>
                            setCredentialsForm("kiloApiKey", event.currentTarget.value)
                          }
                        />
                      </label>
                    </div>

                    <button
                      type="button"
                      class="command-button accent"
                      disabled={isSavingCredentials()}
                      onClick={() => void saveCredentials()}
                    >
                      {isSavingCredentials() ? "Saving..." : "Save access settings"}
                    </button>
                  </div>
                </details>
              </section>
            </Show>

            <Show when={utilityTab() === "chat"}>
              <div class="chat-shell">
                <div class="chat-toolbar">
                  <div>
                    <div class="pane-title">Agent Workflows</div>
                    <div class="pane-caption">
                      On-demand task handoff for Codex, Gemini, Claude, and Kilo
                    </div>
                  </div>
                  <button
                    type="button"
                    class="pane-button"
                    disabled={isRunningAgent()}
                    onClick={() => void startNewChat()}
                  >
                    New task
                  </button>
                </div>

                <div class="agent-switcher">
                  <For each={AGENTS}>
                    {(agent) => {
                      const status = createMemo(() =>
                        agentHealth()?.agents.find((entry) => entry.id === agent.id)
                      );

                      return (
                        <button
                          type="button"
                          class={`agent-chip${
                            selectedAgentId() === agent.id ? " active" : ""
                          }`}
                          onClick={() => {
                            setSelectedAgentId(agent.id);
                            setStatus(`Selected ${agent.label}.`);
                          }}
                        >
                          <span class="agent-chip-name">{agent.label}</span>
                          <span class={`agent-chip-state ${badgeTone(status()?.authState)}`} />
                        </button>
                      );
                    }}
                  </For>
                </div>

                <Show when={selectedAgent().models?.length}>
                  <div class="agent-model-bar">
                    <label class="model-select-field prominent">
                      <span>Model</span>
                      <select
                        value={selectedAgentModel()?.id ?? ""}
                        onChange={(event) =>
                          setSelectedAgentModels(
                            selectedAgent().id,
                            event.currentTarget.value
                          )
                        }
                      >
                        <For each={selectedAgent().models ?? []}>
                          {(model) => <option value={model.id}>{model.label}</option>}
                        </For>
                      </select>
                    </label>
                  </div>
                </Show>

                <div class="chat-stream">
                  <Show when={selectedAgentStatus()?.authState !== "ready"}>
                    <div class="chat-notice">
                      <div class="chat-notice-copy">
                        {selectedAgent().label} is not fully ready yet. Finish CLI login
                        or add credentials in Access before sending a prompt.
                      </div>
                      <button
                        type="button"
                        class="pane-button"
                        onClick={() => setUtilityTab("access")}
                      >
                        Open Access
                      </button>
                    </div>
                  </Show>

                  <div ref={chatTimelineRef} class="chat-timeline">
                    <For each={chatMessages()}>
                      {(message) => (
                        <article
                          class={`chat-row${message.role === "system" ? " is-system" : ""}${
                            message.role === "user" ? " is-user" : ""
                          }${message.role === "assistant" ? " is-assistant" : ""}${
                            message.status === "running" ? " pending" : ""
                          }${message.status === "error" ? " error" : ""}`}
                        >
                          <Show when={message.role !== "user"}>
                            <div class="chat-avatar">
                              {(message.agentLabel ?? selectedAgent().label).slice(0, 1)}
                            </div>
                          </Show>

                          <div class="chat-entry">
                            <div class="chat-meta">
                              <span class="chat-author">
                                {message.role === "user"
                                  ? "You"
                                  : message.agentLabel ?? selectedAgent().label}
                              </span>
                              <span class="chat-time">{message.timestamp}</span>
                            </div>

                            <div class="chat-bubble">
                              <div class="chat-content">{message.content}</div>

                              <Show when={message.approval}>
                                {(approvalAccessor) => {
                                  const approval = approvalAccessor();

                                  return (
                                    <div class="approval-card">
                                      <div class="approval-meta">
                                        <span class="approval-kind">
                                          {approvalTitle(approval)}
                                        </span>
                                        <span
                                          class={`state-badge ${badgeTone(
                                            approval.state === "resolved"
                                              ? "ready"
                                              : approval.state === "submitted"
                                                ? "partial"
                                                : "warning"
                                          )}`}
                                        >
                                          {approvalStateLabel(approval.state)}
                                        </span>
                                      </div>

                                      <Show when={approval.reason}>
                                        <div class="approval-copy">{approval.reason}</div>
                                      </Show>

                                      <Show when={approval.command}>
                                        <div class="approval-detail">
                                          <div class="chat-detail-label">Command</div>
                                          <pre class="terminal-output compact">
                                            {approval.command}
                                          </pre>
                                        </div>
                                      </Show>

                                      <Show when={approval.cwd}>
                                        <div class="approval-copy subtle">
                                          Working directory: {approval.cwd}
                                        </div>
                                      </Show>

                                      <Show when={approval.grantRoot}>
                                        <div class="approval-copy subtle">
                                          Requested root: {approval.grantRoot}
                                        </div>
                                      </Show>

                                      <Show when={approval.locations?.length}>
                                        <div class="approval-copy subtle">
                                          Paths:{" "}
                                          {approval.locations
                                            ?.map((location) =>
                                              location.line
                                                ? `${location.path}:${location.line}`
                                                : location.path
                                            )
                                            .join(", ")}
                                        </div>
                                      </Show>

                                      <Show when={approval.permissions}>
                                        {(permissionsAccessor) => {
                                          const permissions = permissionsAccessor();
                                          return (
                                            <div class="approval-permissions">
                                              <Show
                                                when={
                                                  permissions.networkEnabled !== null &&
                                                  permissions.networkEnabled !== undefined
                                                }
                                              >
                                                <div class="approval-copy subtle">
                                                  Network access:{" "}
                                                  {permissions.networkEnabled ? "requested" : "off"}
                                                </div>
                                              </Show>

                                              <Show when={permissions.readRoots.length}>
                                                <div class="approval-copy subtle">
                                                  Read roots: {permissions.readRoots.join(", ")}
                                                </div>
                                              </Show>

                                              <Show when={permissions.writeRoots.length}>
                                                <div class="approval-copy subtle">
                                                  Write roots: {permissions.writeRoots.join(", ")}
                                                </div>
                                              </Show>
                                            </div>
                                          );
                                        }}
                                      </Show>

                                      <div class="approval-actions">
                                        <For each={approval.choices}>
                                          {(choice) => (
                                            <button
                                              type="button"
                                              class={`command-button${
                                                choice.id.includes("deny") ||
                                                choice.id.includes("reject") ||
                                                choice.id.includes("cancel")
                                                  ? ""
                                                  : " accent"
                                              }`}
                                              disabled={approval.state !== "pending"}
                                              onClick={() =>
                                                void respondToApproval(approval, choice.id)
                                              }
                                            >
                                              {choice.label}
                                            </button>
                                          )}
                                        </For>
                                      </div>
                                    </div>
                                  );
                                }}
                              </Show>

                              <Show when={message.command || message.prompt || message.stderr}>
                                <details class="chat-details">
                                  <summary>Details</summary>

                                  <Show when={message.command}>
                                    <div class="chat-detail-label">Command</div>
                                    <pre class="terminal-output compact">{message.command}</pre>
                                  </Show>

                                  <Show when={message.prompt}>
                                    <div class="chat-detail-label">Prompt sent</div>
                                    <pre class="terminal-output compact">{message.prompt}</pre>
                                  </Show>

                                  <Show when={message.stderr}>
                                    <div class="chat-detail-label error">stderr</div>
                                    <pre class="terminal-output compact error">
                                      {message.stderr}
                                    </pre>
                                  </Show>
                                </details>
                              </Show>
                            </div>
                          </div>
                        </article>
                      )}
                    </For>
                  </div>

                  <div class="quick-prompts">
                    <For each={QUICK_PROMPTS}>
                      {(prompt) => (
                        <button
                          type="button"
                          class="quick-prompt"
                          onClick={() => {
                            setChatDraft(prompt);
                            chatInputRef?.focus();
                          }}
                        >
                          {prompt}
                        </button>
                      )}
                    </For>
                  </div>

                  <div class="chat-composer">
                    <label class="form-field">
                      <span>Message</span>
                      <textarea
                        ref={chatInputRef}
                        value={chatDraft()}
                        placeholder={`Ask ${selectedAgent().label} to review code, explain a file, or suggest the next step...`}
                        onInput={(event) => setChatDraft(event.currentTarget.value)}
                        onKeyDown={(event) => {
                          if (event.key === "Enter" && !event.shiftKey) {
                            event.preventDefault();
                            void runAgentChat();
                          }
                        }}
                      />
                    </label>

                    <div class="chat-composer-footer">
                      <div class="chat-readiness">
                        <span class={`state-badge ${badgeTone(selectedAgentStatus()?.authState)}`}>
                          {selectedAgentStatus()?.authState ?? "checking"}
                        </span>
                        <span class="chat-status-copy">
                          {pendingApprovalCount() > 0
                            ? `${pendingApprovalCount()} approval request${
                                pendingApprovalCount() === 1 ? "" : "s"
                              } waiting`
                            : selectedAgentStatus()?.authState === "ready"
                              ? `${selectedAgent().label} ready`
                            : selectedAgentStatus()?.authState === "partial"
                              ? `${selectedAgent().label} setup incomplete`
                              : selectedAgentStatus()?.authState === "unavailable"
                                ? `${selectedAgent().label} unavailable`
                                : `${selectedAgent().label} needs access`}
                        </span>
                      </div>

                      <div class="chat-actions">
                        <label class="toggle-row compact">
                          <input
                            type="checkbox"
                            checked={includeCompactContext()}
                            onChange={(event) =>
                              setIncludeCompactContext(event.currentTarget.checked)
                            }
                          />
                          <span>Context</span>
                        </label>

                        <button
                          type="button"
                          class="command-button accent"
                          disabled={
                            isRunningAgent() || selectedAgentStatus()?.authState !== "ready"
                          }
                          onClick={() => void runAgentChat()}
                        >
                          {isRunningAgent() ? "Sending..." : "Send"}
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </Show>

            <Show when={utilityTab() === "project"}>
              <section class="panel-section no-divider">
                <div class="pane-header compact">
                  <div>
                    <div class="pane-title">Project Automation</div>
                    <div class="pane-caption">
                      Python, Rust, C/C++/CUDA, toolchains, and explicit project actions
                    </div>
                  </div>
                  <button
                    type="button"
                    class="pane-button"
                    onClick={() =>
                      void Promise.all([
                        refreshPythonEnvironment(),
                        refreshRustEnvironment(),
                        refreshCFamilyEnvironment(),
                      ])
                    }
                  >
                    Refresh
                  </button>
                </div>

                <article class="status-card">
                  <div class="status-card-title">Workspace</div>
                  <div class="status-card-copy text-wrap">
                    {workspaceRoot() || "No workspace selected"}
                  </div>
                </article>

                <article class="status-card">
                  <div class="status-card-head">
                    <div class="status-card-title">C/C++/CUDA toolchains</div>
                    <span
                      class={`state-badge ${
                        cFamilyEnvironment()?.compileCommandsExists ? "ready" : "warning"
                      }`}
                    >
                      {cFamilyEnvironment()?.compileCommandsExists
                        ? "compile DB"
                        : "parser fallback"}
                    </span>
                  </div>

                  <div class="status-card-copy">{cFamilyEnvironment()?.summary}</div>

                  <div class="status-row-list">
                    <div class="status-row">
                      <span>C-family root</span>
                      <strong>{cFamilyEnvironment()?.root || "-"}</strong>
                    </div>
                    <div class="status-row">
                      <span>compile_commands.json</span>
                      <strong>
                        {cFamilyEnvironment()?.compileCommandsExists ? "Yes" : "No"}
                      </strong>
                    </div>
                    <div class="status-row">
                      <span>CMakeLists.txt</span>
                      <strong>{cFamilyEnvironment()?.cmakeListsExists ? "Yes" : "No"}</strong>
                    </div>
                    <div class="status-row">
                      <span>Recommended</span>
                      <strong>{cFamilyEnvironment()?.recommendedCommand ?? "-"}</strong>
                    </div>
                  </div>

                  <div class="subsection-title">C-family tools</div>
                  <div class="tool-status-grid">
                    <For each={cFamilyToolStatus()}>
                      {(tool) => (
                        <div class={`pill${tool.available ? " available" : ""}`}>
                          <span class="pill-dot" />
                          <span>{tool.label}</span>
                        </div>
                      )}
                    </For>
                  </div>
                </article>

                <article class="status-card">
                  <div class="status-card-head">
                    <div class="status-card-title">C/C++/CUDA workflow</div>
                    <span class={`state-badge ${isCFamilyDocumentActive() ? "ready" : "muted"}`}>
                      {isCFamilyDocumentActive() ? "active" : "idle"}
                    </span>
                  </div>

                  <div class="status-card-copy">{cFamilyManagementSummary()}</div>

                  <div class="status-row-list">
                    <div class="status-row">
                      <span>Editor</span>
                      <strong>Parser hover, semantic tokens, and outline</strong>
                    </div>
                    <div class="status-row">
                      <span>Language service</span>
                      <strong>
                        {cFamilyEnvironment()?.compileCommandsExists &&
                        cFamilyEnvironment()?.clangdAvailable
                          ? "clangd metadata detected"
                          : "clangd integration pending compile database"}
                      </strong>
                    </div>
                    <div class="status-row">
                      <span>CUDA</span>
                      <strong>
                        {cFamilyEnvironment()?.nvccAvailable
                          ? "NVCC detected"
                          : "Install CUDA Toolkit for nvcc"}
                      </strong>
                    </div>
                  </div>
                </article>

                <article class="status-card">
                  <div class="status-card-head">
                    <div class="status-card-title">Rust toolchains</div>
                    <span
                      class={`state-badge ${
                        rustEnvironment()?.cargoTomlExists ? "ready" : "warning"
                      }`}
                    >
                      {rustEnvironment()?.cargoTomlExists ? "Cargo ready" : "No Cargo.toml"}
                    </span>
                  </div>

                  <div class="status-card-copy">{rustEnvironment()?.summary}</div>

                  <div class="status-row-list">
                    <div class="status-row">
                      <span>Rust root</span>
                      <strong>{rustEnvironment()?.root || "-"}</strong>
                    </div>
                    <div class="status-row">
                      <span>Active toolchain</span>
                      <strong>{rustEnvironment()?.activeToolchain || "Not reported"}</strong>
                    </div>
                    <div class="status-row">
                      <span>Toolchain file</span>
                      <strong>{rustEnvironment()?.rustToolchainFile ? "Yes" : "No"}</strong>
                    </div>
                    <div class="status-row">
                      <span>Recommended</span>
                      <strong>{rustEnvironment()?.recommendedCommand ?? "-"}</strong>
                    </div>
                  </div>

                  <div class="subsection-title">Installed toolchains</div>
                  <Show
                    when={rustEnvironment()?.installedToolchains.length}
                    fallback={<div class="empty-note">No rustup toolchains reported.</div>}
                  >
                    <ul class="event-list compact-list">
                      <For each={rustEnvironment()?.installedToolchains ?? []}>
                        {(toolchain) => (
                          <li class="event-item">
                            <div class="event-copy text-wrap">{toolchain}</div>
                          </li>
                        )}
                      </For>
                    </ul>
                  </Show>

                  <div class="subsection-title">Rust tools</div>
                  <div class="tool-status-grid">
                    <For each={rustToolStatus()}>
                      {(tool) => (
                        <div class={`pill${tool.available ? " available" : ""}`}>
                          <span class="pill-dot" />
                          <span>{tool.label}</span>
                        </div>
                      )}
                    </For>
                  </div>
                </article>

                <article class="status-card">
                  <div class="status-card-head">
                    <div class="status-card-title">Cargo / rust-analyzer workflow</div>
                    <span
                      class={`state-badge ${
                        activeDocument()?.language === "rust" ? "ready" : "muted"
                      }`}
                    >
                      {activeDocument()?.language === "rust" ? "active" : "idle"}
                    </span>
                  </div>

                  <div class="status-card-copy">{rustManagementSummary()}</div>

                  <div class="status-row-list">
                    <div class="status-row">
                      <span>Editor</span>
                      <strong>
                        {rustEnvironment()?.cargoTomlExists &&
                        rustEnvironment()?.rustAnalyzerAvailable
                          ? "rust-analyzer hover and semantic tokens"
                          : "Standalone parser hover and semantic tokens"}
                      </strong>
                    </div>
                    <div class="status-row">
                      <span>Diagnostics</span>
                      <strong>Cargo JSON diagnostics on explicit runs</strong>
                    </div>
                    <div class="status-row">
                      <span>Debug</span>
                      <strong>
                        {rustEnvironment()?.lldbAvailable || rustEnvironment()?.codelldbAvailable
                          ? "LLDB adapter detected"
                          : "Install LLDB or CodeLLDB"}
                      </strong>
                    </div>
                  </div>

                  <div class="tooling-button-grid">
                    <button
                      type="button"
                      class="pane-button"
                      disabled={!isRustToolingAvailable() || Boolean(runningRustToolingAction())}
                      onClick={() => void runRustTooling("check")}
                    >
                      {runningRustToolingAction() === "check" ? "Checking..." : "Cargo check"}
                    </button>
                    <button
                      type="button"
                      class="pane-button"
                      disabled={!isRustToolingAvailable() || Boolean(runningRustToolingAction())}
                      onClick={() => void runRustTooling("clippy")}
                    >
                      {runningRustToolingAction() === "clippy" ? "Linting..." : "Clippy"}
                    </button>
                    <button
                      type="button"
                      class="pane-button"
                      disabled={!isRustToolingAvailable() || Boolean(runningRustToolingAction())}
                      onClick={() => void runRustTooling("format")}
                    >
                      {runningRustToolingAction() === "format" ? "Formatting..." : "rustfmt"}
                    </button>
                    <button
                      type="button"
                      class="pane-button"
                      disabled={!isRustToolingAvailable() || Boolean(runningRustToolingAction())}
                      onClick={() => void runRustTooling("test")}
                    >
                      {runningRustToolingAction() === "test" ? "Testing..." : "Cargo test"}
                    </button>
                    <button
                      type="button"
                      class="pane-button"
                      disabled={!isRustToolingAvailable() || Boolean(runningRustToolingAction())}
                      onClick={() => void runRustTooling("build")}
                    >
                      {runningRustToolingAction() === "build" ? "Building..." : "Cargo build"}
                    </button>
                    <button
                      type="button"
                      class="pane-button"
                      disabled={!isRustToolingAvailable() || Boolean(runningRustToolingAction())}
                      onClick={() => void runRustTooling("doc")}
                    >
                      {runningRustToolingAction() === "doc" ? "Documenting..." : "Cargo doc"}
                    </button>
                    <button
                      type="button"
                      class="pane-button"
                      disabled={!isRustToolingAvailable() || Boolean(runningRustToolingAction())}
                      onClick={() => void runRustTooling("metadata")}
                    >
                      {runningRustToolingAction() === "metadata"
                        ? "Reading..."
                        : "Cargo metadata"}
                    </button>
                  </div>
                </article>

                <article class="status-card">
                  <div class="status-card-head">
                    <div class="status-card-title">Python environment</div>
                    <span
                      class={`state-badge ${
                        pythonEnvironment()?.uvAvailable ? "ready" : "warning"
                      }`}
                    >
                      {pythonEnvironment()?.uvAvailable ? "uv ready" : "uv missing"}
                    </span>
                  </div>

                  <div class="status-card-copy">{pythonEnvironment()?.summary}</div>

                  <div class="status-row-list">
                    <div class="status-row">
                      <span>pyproject.toml</span>
                      <strong>{pythonEnvironment()?.pyprojectExists ? "Yes" : "No"}</strong>
                    </div>
                    <div class="status-row">
                      <span>.venv</span>
                      <strong>{pythonEnvironment()?.venvExists ? "Yes" : "No"}</strong>
                    </div>
                    <div class="status-row">
                      <span>Recommended</span>
                      <strong>{pythonEnvironment()?.recommendedCommand ?? "-"}</strong>
                    </div>
                  </div>

                  <Show when={pythonEnvironment()?.pythonPath || venvPathHint()}>
                    <div class="subsection-title">Interpreter</div>
                    <pre class="terminal-output compact">
                      {pythonEnvironment()?.pythonPath || venvPathHint()}
                    </pre>
                  </Show>
                </article>

                <article class="status-card">
                  <div class="status-card-head">
                    <div class="status-card-title">Ruff / ty package workflow</div>
                    <span
                      class={`state-badge ${
                        missingImports().length ? "warning" : "muted"
                      }`}
                    >
                      {missingImports().length
                        ? `${missingImports().length} pending`
                        : "idle"}
                    </span>
                  </div>

                  <div class="status-card-copy">{pythonManagementSummary()}</div>

                  <div class="status-row-list">
                    <div class="status-row">
                      <span>Scan trigger</span>
                      <strong>Ruff and ty after active `.py` typing settles</strong>
                    </div>
                    <div class="status-row">
                      <span>Review step</span>
                      <strong>ty unresolved imports become install candidates</strong>
                    </div>
                    <div class="status-row">
                      <span>Install step</span>
                      <strong>Click one button to install everything with uv</strong>
                    </div>
                    <div class="status-row">
                      <span>Terminal and agents</span>
                      <strong>
                        {pythonEnvironment()?.venvExists
                          ? "Detected `.venv` is auto-activated"
                          : "No `.venv` to auto-activate yet"}
                      </strong>
                    </div>
                  </div>

                  <div class="tooling-button-grid">
                    <button
                      type="button"
                      class="pane-button"
                      disabled={!isPythonToolingAvailable() || Boolean(runningPythonToolingAction())}
                      onClick={() => void runPythonTooling("check")}
                    >
                      {runningPythonToolingAction() === "check" ? "Checking..." : "Ruff check"}
                    </button>
                    <button
                      type="button"
                      class="pane-button"
                      disabled={!isPythonToolingAvailable() || Boolean(runningPythonToolingAction())}
                      onClick={() => void runPythonTooling("format")}
                    >
                      {runningPythonToolingAction() === "format" ? "Formatting..." : "Format"}
                    </button>
                    <button
                      type="button"
                      class="pane-button"
                      disabled={!isPythonToolingAvailable() || Boolean(runningPythonToolingAction())}
                      onClick={() => void runPythonTooling("fixAll")}
                    >
                      {runningPythonToolingAction() === "fixAll" ? "Fixing..." : "Fix all"}
                    </button>
                    <button
                      type="button"
                      class="pane-button"
                      disabled={!isPythonToolingAvailable() || Boolean(runningPythonToolingAction())}
                      onClick={() => void runPythonTooling("organizeImports")}
                    >
                      {runningPythonToolingAction() === "organizeImports"
                        ? "Organizing..."
                        : "Organize imports"}
                    </button>
                    <button
                      type="button"
                      class="pane-button"
                      disabled={!isPythonToolingAvailable() || Boolean(runningPythonToolingAction())}
                      onClick={() => void runPythonTooling("typeCheck")}
                    >
                      {runningPythonToolingAction() === "typeCheck" ? "Checking..." : "ty check"}
                    </button>
                  </div>
                </article>

                <div class="agent-card-actions">
                  <button
                    type="button"
                    class="command-button"
                    disabled={isPreparingPythonEnvironment() || !workspaceRoot()}
                    onClick={() => void preparePythonEnvironment()}
                  >
                    {isPreparingPythonEnvironment()
                      ? "Working..."
                      : "Create or sync Python environment"}
                  </button>
                  <button
                    type="button"
                    class="command-button accent"
                    disabled={
                      isInstallingMissingPackages() || !isPythonInstallActionAvailable()
                    }
                    onClick={() => void installMissingPackages()}
                  >
                    {isInstallingMissingPackages()
                      ? "Installing..."
                      : `Install missing packages (${missingImports().length})`}
                  </button>
                </div>

                <Show when={prepareOutcome()}>
                  <div class="terminal-label">Environment activity</div>
                  <pre class="terminal-output compact">
                    {prepareOutcome()?.command}
                    {"\n\n"}
                    {prepareOutcome()?.stdout || prepareOutcome()?.stderr || "No output"}
                  </pre>
                </Show>

                <Show when={pythonToolingOutcome()}>
                  <div class="terminal-label">Python tooling activity</div>
                  <pre class="terminal-output compact">
                    {pythonToolingOutcome()?.command}
                    {"\n\n"}
                    {pythonToolingOutcome()?.stdout ||
                      pythonToolingOutcome()?.stderr ||
                      "No output"}
                  </pre>
                </Show>

                <Show when={rustToolingOutcome()}>
                  <div class="terminal-label">Rust tooling activity</div>
                  <pre class="terminal-output compact">
                    {rustToolingOutcome()?.command}
                    {"\n\n"}
                    {rustToolingOutcome()?.stdout ||
                      rustToolingOutcome()?.stderr ||
                      `${rustToolingOutcome()?.diagnostics?.length ?? 0} diagnostic(s)`}
                  </pre>
                </Show>

                <div class="terminal-label">Missing imports</div>
                <Show
                  when={missingImports().length}
                  fallback={
                    <div class="empty-note">
                      No unresolved third-party imports in the active Python file.
                    </div>
                  }
                >
                  <ul class="event-list">
                    <For each={missingImports()}>
                      {(item) => (
                        <li class="event-item">
                          <div class="event-head">
                            <span>{item.package}</span>
                            <span>{item.module}</span>
                          </div>
                          <div class="event-copy text-wrap">
                            {installCommandPreview(pythonEnvironment(), item.package)}
                          </div>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>

                <div class="terminal-label">Latest uv install activity</div>
                <Show
                  when={activeInstallEvents().length}
                  fallback={<div class="empty-note">No uv install events for the active file yet.</div>}
                >
                  <ul class="event-list">
                    <For each={activeInstallEvents()}>
                      {(event) => (
                        <li class="event-item">
                          <div class="event-head">
                            <span>{eventStateLabel(event)}</span>
                            <span>{event.package}</span>
                          </div>
                          <div class="event-copy text-wrap">
                            {event.command}
                            <Show when={event.output}>
                              <>
                                {"\n\n"}
                                {event.output}
                              </>
                            </Show>
                          </div>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
              </section>
            </Show>

            <Show when={utilityTab() === "problems"}>
              <section class="panel-section no-divider">
                <div class="pane-header compact">
                  <div>
                    <div class="pane-title">Problems</div>
                    <div class="pane-caption">
                      Active-file diagnostics from Ruff, ty, Cargo, and language services
                    </div>
                  </div>
                </div>

                <Show
                  when={activeDocument()}
                  fallback={<div class="empty-note">Open a source file to inspect diagnostics.</div>}
                >
                  <div class="problem-summary-grid">
                    <div class="problem-summary-item">
                      <span>Errors</span>
                      <strong>{activeDiagnosticStats().errors}</strong>
                    </div>
                    <div class="problem-summary-item">
                      <span>Warnings</span>
                      <strong>{activeDiagnosticStats().warnings}</strong>
                    </div>
                    <div class="problem-summary-item">
                      <span>Total</span>
                      <strong>{activeDiagnosticStats().total}</strong>
                    </div>
                  </div>

                  <Show when={activeDiagnosticSources().length}>
                    <div class="diagnostic-source-row">
                      <For each={activeDiagnosticSources()}>
                        {(source) => (
                          <span class="source-chip">
                            {source.label} {source.count}
                          </span>
                        )}
                      </For>
                    </div>
                  </Show>

                  <Show
                    when={activeDiagnosticsSorted().length}
                    fallback={<div class="empty-note">No active diagnostics for the current file.</div>}
                  >
                    <ul class="symbol-list">
                      <For each={activeDiagnosticsSorted()}>
                        {(diagnostic) => (
                          <li>
                            <button
                              type="button"
                              class="symbol-item problem-item"
                              onClick={() => resetLineJump(setJumpToLine, diagnostic.line)}
                            >
                              <span class={`symbol-kind ${diagnostic.severity}`}>
                                {diagnostic.severity}
                              </span>
                              <span class="problem-copy">
                                <span class="symbol-name">{diagnostic.message}</span>
                                <span class="problem-source">
                                  {diagnosticSourceLabel(diagnostic.module)}
                                </span>
                              </span>
                              <span class="symbol-line">
                                L{diagnostic.line}:C{diagnostic.column}
                              </span>
                            </button>
                          </li>
                        )}
                      </For>
                    </ul>
                  </Show>
                </Show>
              </section>
            </Show>

            <Show when={utilityTab() === "outline"}>
              <section class="panel-section no-divider">
                <div class="pane-header compact">
                  <div>
                    <div class="pane-title">Structure</div>
                    <div class="pane-caption">
                      tree-sitter outline and compact context
                    </div>
                  </div>
                </div>

                <div class="subsection-title">Symbols</div>
                <Show
                  when={symbols().length}
                  fallback={<div class="empty-note">No extractable symbols in the active file.</div>}
                >
                  <ul class="symbol-list">
                    <For each={symbols()}>
                      {(symbol) => (
                        <li>
                          <button
                            type="button"
                            class="symbol-item"
                            onClick={() => resetLineJump(setJumpToLine, symbol.startLine)}
                          >
                            <span class="symbol-kind">{symbol.kind}</span>
                            <span class="symbol-name">{symbol.label}</span>
                            <span class="symbol-line">L{symbol.startLine}</span>
                          </button>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>

                <div class="subsection-title">Compact context</div>
                <pre class="terminal-output context">
                  {compactContext() || "Context preview will populate from the active file."}
                </pre>
              </section>
            </Show>
          </div>
        </aside>

        <div class="pane-resizer vertical first" onMouseDown={resizeExplorer} />
        <div class="pane-resizer vertical second" onMouseDown={resizeUtility} />
      </div>

      <Show when={isTerminalVisible()}>
        <div class="pane-resizer horizontal" onMouseDown={resizeTerminal} />
      </Show>

      <Show when={isTerminalVisible()}>
        <section class="terminal-panel">
          <div class="terminal-panel-header">
            <div>
              <div class="pane-title">Terminal</div>
              <div class="pane-caption">{statusMessage()}</div>
            </div>

            <div class="terminal-panel-actions">
              <span class="terminal-panel-cwd text-wrap">
                {terminalCwd() || workspaceRoot() || "No workspace open"}
              </span>
              <button type="button" class="pane-button" onClick={() => toggleTerminal(false)}>
                Hide
              </button>
              <button
                type="button"
                class="pane-button"
                disabled={!terminalEntries().length}
                onClick={() => {
                  setTerminalEntries([]);
                  setStatus("Cleared terminal output.");
                }}
              >
                Clear
              </button>
            </div>
          </div>

          <div class="terminal-panel-body">
            <div ref={terminalLogRef} class="terminal-log">
              <Show
                when={terminalEntries().length}
                fallback={
                  <div class="terminal-empty">
                    Run PowerShell commands inside Hematite. The working directory
                    stays in sync between commands, so <code>cd</code>,{" "}
                    <code>uv sync</code>, and agent CLIs can all stay in-app.
                  </div>
                }
              >
                <For each={terminalEntries()}>
                  {(entry) => (
                    <article class={`terminal-entry${entry.success ? "" : " error"}`}>
                      <div class="terminal-entry-meta">
                        <span>{entry.timestamp}</span>
                        <span class="text-wrap">{entry.cwd}</span>
                      </div>

                      <div class="terminal-command-line">
                        <span class="terminal-command-prefix">$</span>
                        <span>{entry.command}</span>
                      </div>

                      <Show when={entry.stdout}>
                        <pre class="terminal-output terminal-log-output">{entry.stdout}</pre>
                      </Show>

                      <Show when={entry.stderr}>
                        <pre class="terminal-output terminal-log-output error">
                          {entry.stderr}
                        </pre>
                      </Show>
                    </article>
                  )}
                </For>
              </Show>
            </div>

            <form
              class="terminal-input-row"
              onSubmit={(event) => {
                event.preventDefault();
                void runTerminalCommand();
              }}
            >
              <label class="terminal-input-shell">
                <span class="terminal-prompt">
                  PS {basename(terminalCwd() || workspaceRoot() || "~")}
                </span>
                <input
                  value={terminalInput()}
                  placeholder="Run a PowerShell command in this workspace"
                  spellcheck={false}
                  onInput={(event) => setTerminalInput(event.currentTarget.value)}
                />
              </label>

              <button
                type="submit"
                class="command-button accent"
                disabled={isRunningTerminal()}
              >
                {isRunningTerminal() ? "Running..." : "Run"}
              </button>
            </form>
          </div>
        </section>
      </Show>

      <footer class="status-bar">
        <div class="status-bar-left">
          <span>{statusMessage()}</span>
          <span>{workspaceRoot() ? basename(workspaceRoot()!) : "No workspace"}</span>
        </div>
        <div class="status-bar-right">
          <Show when={activeDocument()} fallback={<span>No file</span>}>
            <span>{activeDocument()?.language}</span>
            <span>{activeDocument()?.dirty ? "Unsaved" : "Saved"}</span>
            <span>
              Ln {cursorPosition().line}, Col {cursorPosition().column}
            </span>
            <span>
              {activeDiagnosticStats().errors}E {activeDiagnosticStats().warnings}W
            </span>
            <span>{lineWrapping() ? "Wrap" : "No wrap"}</span>
            <span>{activeTab() ? basename(activeTab()!) : "No file"}</span>
          </Show>
        </div>
      </footer>
    </main>
  );
}
