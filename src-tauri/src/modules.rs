use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque, hash_map::DefaultHasher},
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
use tauri::{AppHandle, Manager};
use wasmi::{
    Caller, Config, EnforcedLimits, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder,
};
use wasmi_wasi::{
    WasiCtx, WasiCtxBuilder,
    wasi_common::pipe::{ReadPipe, WritePipe},
};

const MANIFEST_FILE_NAME: &str = "module.json";
const WASM_FILE_NAME: &str = "module.wasm";
const MODULES_STATE_FILE_NAME: &str = "modules-state.json";
const MODULES_LOCK_FILE_NAME: &str = "modules.lock";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_WASM_BYTES: u64 = 16 * 1024 * 1024;
const MAX_STATE_BYTES: u64 = 1024 * 1024;
const MAX_MODULE_ID_BYTES: usize = 128;
const MAX_COMMANDS: usize = 64;
const MAX_ACTIVATION_EVENTS: usize = 64;
const MAX_CACHE_ENTRIES: usize = 32;
const MAX_CACHE_SOURCE_BYTES: usize = 32 * 1024 * 1024;
const MEMORY_BYTES: usize = 32 * 1024 * 1024;
const FUEL_LIMIT: u64 = 10_000_000;
const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_LOGS: usize = 64;
const MAX_LOG_BYTES: usize = 8 * 1024;
const MAX_WASI_STDIO_BYTES: usize = 64 * 1024;
const MAX_HOST_CALLS: u32 = 1024;
const MAX_TABLE_ELEMENTS: usize = 4096;

static MODULE_RUNTIME: OnceLock<ModuleRuntime> = OnceLock::new();
static MODULE_REGISTRY: OnceLock<Mutex<()>> = OnceLock::new();
static STAGING_SEQUENCE: AtomicUsize = AtomicUsize::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum Capability {
    Log,
    Wasi,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ModuleManifest {
    manifest_version: u32,
    abi_version: u32,
    publisher: String,
    name: String,
    display_name: String,
    version: String,
    #[serde(default)]
    description: String,
    entry: String,
    activation_events: Vec<String>,
    #[serde(default)]
    capabilities: Vec<Capability>,
    contributes: Contributions,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Contributions {
    commands: Vec<CommandContribution>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CommandContribution {
    command: String,
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    category: Option<String>,
}

#[derive(Debug, Clone)]
struct ValidatedManifest {
    id: String,
    manifest: ModuleManifest,
}

#[derive(Debug, Clone)]
struct InstalledModule {
    id: String,
    directory: PathBuf,
    manifest: ModuleManifest,
}

#[derive(Debug, Clone)]
struct ModulePaths {
    base: PathBuf,
    modules: PathBuf,
    state: PathBuf,
    lock: PathBuf,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistentState {
    #[serde(default)]
    enabled_ids: BTreeSet<String>,
    #[serde(default)]
    granted_capabilities: BTreeMap<String, BTreeSet<Capability>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Catalog {
    modules: Vec<ModuleInfo>,
    errors: Vec<String>,
    policy: ModulePolicy,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModuleInfo {
    id: String,
    publisher: String,
    name: String,
    display_name: String,
    version: String,
    description: String,
    enabled: bool,
    needs_approval: bool,
    capabilities: Vec<Capability>,
    commands: Vec<CommandContribution>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModulePolicy {
    memory_mi_b: u32,
    fuel: u64,
    max_input_bytes: usize,
    max_output_bytes: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SetModuleEnabledRequest {
    module_id: String,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UninstallModuleRequest {
    module_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExecuteModuleCommandRequest {
    command: String,
    input_json: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExecuteModuleCommandResult {
    module_id: String,
    command: String,
    output: String,
    logs: Vec<ModuleLog>,
    fuel_consumed: u64,
    duration_ms: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModuleLog {
    level: LogLevel,
    message: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum LogLevel {
    Info,
    Warn,
    Error,
}

struct HostState {
    limits: StoreLimits,
    granted_capabilities: BTreeSet<Capability>,
    logs: Vec<ModuleLog>,
    host_calls: u32,
    violation: Option<String>,
    wasi: Option<WasiCtx>,
}

#[derive(Clone)]
struct WasiOutput {
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl WasiOutput {
    fn new() -> Self {
        Self {
            bytes: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn snapshot(&self) -> Result<Vec<u8>, String> {
        self.bytes
            .lock()
            .map(|bytes| bytes.clone())
            .map_err(|_| "WASI output buffer lock is poisoned.".to_string())
    }
}

impl Write for WasiOutput {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let mut bytes = self
            .bytes
            .lock()
            .map_err(|_| io::Error::other("WASI output buffer lock is poisoned"))?;
        let remaining = MAX_WASI_STDIO_BYTES.saturating_sub(bytes.len());
        if remaining == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "WASI output limit exceeded",
            ));
        }
        let written = remaining.min(buffer.len());
        bytes.extend_from_slice(&buffer[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct WasiStreams {
    stdout: WasiOutput,
    stderr: WasiOutput,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    fingerprint: u64,
    byte_len: usize,
}

struct CacheEntry {
    module_id: String,
    key: CacheKey,
    engine: Engine,
    module: Module,
    wasm_len: usize,
}

#[derive(Default)]
struct ModuleCache {
    entries: VecDeque<CacheEntry>,
    total_source_bytes: usize,
}

struct ModuleRuntime {
    cache: Mutex<ModuleCache>,
    module_gates: Mutex<BTreeMap<String, Weak<Mutex<()>>>>,
    active: AtomicUsize,
    active_limit: usize,
}

#[derive(Clone)]
struct CompiledModule {
    engine: Engine,
    module: Module,
}

struct ActivePermit(&'static AtomicUsize);

impl Drop for ActivePermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

impl ActivePermit {
    fn try_acquire(runtime: &'static ModuleRuntime) -> Result<Self, String> {
        let mut current = runtime.active.load(Ordering::Acquire);
        loop {
            if current >= runtime.active_limit {
                return Err(format!(
                    "Module runtime is busy (maximum {} active invocations).",
                    runtime.active_limit
                ));
            }
            match runtime.active.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(Self(&runtime.active)),
                Err(observed) => current = observed,
            }
        }
    }
}

fn module_runtime() -> &'static ModuleRuntime {
    MODULE_RUNTIME.get_or_init(|| ModuleRuntime {
        cache: Mutex::new(ModuleCache::default()),
        module_gates: Mutex::new(BTreeMap::new()),
        active: AtomicUsize::new(0),
        active_limit: std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .min(4),
    })
}

fn new_module_engine() -> Engine {
    let mut config = Config::default();
    config
        .consume_fuel(true)
        .ignore_custom_sections(true)
        .enforced_limits(EnforcedLimits::strict());
    Engine::new(&config)
}

fn registry() -> &'static Mutex<()> {
    MODULE_REGISTRY.get_or_init(|| Mutex::new(()))
}

fn paths_for_app(app: &AppHandle) -> Result<ModulePaths, String> {
    let base = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("Failed to resolve Hematite app data: {error}"))?;
    Ok(ModulePaths {
        modules: base.join("modules"),
        state: base.join(MODULES_STATE_FILE_NAME),
        lock: base.join(MODULES_LOCK_FILE_NAME),
        base,
    })
}

fn acquire_process_lock(paths: &ModulePaths) -> Result<File, String> {
    fs::create_dir_all(&paths.base)
        .map_err(|error| format!("Failed to create app data directory: {error}"))?;
    match fs::symlink_metadata(&paths.lock) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err("Module registry lock path is not a regular file.".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to inspect module registry lock: {error}")),
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&paths.lock)
        .map_err(|error| format!("Failed to open module registry lock: {error}"))?;
    file.lock()
        .map_err(|error| format!("Failed to lock the module registry: {error}"))?;
    Ok(file)
}

fn validate_id_segment(kind: &str, value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit()
        || bytes
            .iter()
            .any(|byte| !byte.is_ascii_lowercase() && !byte.is_ascii_digit() && *byte != b'-')
    {
        return Err(format!(
            "{kind} must match [a-z0-9][a-z0-9-]* using lowercase ASCII."
        ));
    }
    Ok(())
}

fn parse_manifest(bytes: &[u8]) -> Result<ValidatedManifest, String> {
    let manifest: ModuleManifest =
        serde_json::from_slice(bytes).map_err(|error| format!("Invalid module.json: {error}"))?;
    validate_manifest(manifest)
}

fn validate_manifest(manifest: ModuleManifest) -> Result<ValidatedManifest, String> {
    if manifest.manifest_version != 1 {
        return Err("manifestVersion must be 1.".into());
    }
    if manifest.abi_version != 1 {
        return Err("abiVersion must be 1.".into());
    }
    if manifest.entry != WASM_FILE_NAME {
        return Err("entry must be exactly module.wasm.".into());
    }
    validate_id_segment("publisher", &manifest.publisher)?;
    validate_id_segment("name", &manifest.name)?;
    if manifest.publisher == "hematite" {
        return Err("The publisher name hematite is reserved.".into());
    }
    let id = format!("{}.{}", manifest.publisher, manifest.name);
    if id.len() > MAX_MODULE_ID_BYTES {
        return Err(format!(
            "Computed module id exceeds {MAX_MODULE_ID_BYTES} bytes."
        ));
    }
    if manifest.display_name.trim().is_empty() {
        return Err("displayName must not be empty.".into());
    }
    if manifest.version.trim().is_empty() {
        return Err("version must not be empty.".into());
    }
    if manifest.contributes.commands.len() > MAX_COMMANDS {
        return Err(format!(
            "A module may contribute at most {MAX_COMMANDS} commands."
        ));
    }
    if manifest.activation_events.len() > MAX_ACTIVATION_EVENTS {
        return Err(format!(
            "A module may declare at most {MAX_ACTIVATION_EVENTS} activation events."
        ));
    }

    let command_prefix = format!("{id}.");
    let mut commands = BTreeSet::new();
    for command in &manifest.contributes.commands {
        if command.command != id && !command.command.starts_with(&command_prefix) {
            return Err(format!(
                "Command {} must be the module id or use its {command_prefix} namespace.",
                command.command
            ));
        }
        if command.title.trim().is_empty() {
            return Err(format!("Command {} has an empty title.", command.command));
        }
        if !commands.insert(command.command.clone()) {
            return Err(format!("Duplicate command {}.", command.command));
        }
    }

    let mut events = BTreeSet::new();
    for event in &manifest.activation_events {
        if !events.insert(event.clone()) {
            return Err(format!("Duplicate activation event {event}."));
        }
    }
    let expected_events = commands
        .iter()
        .map(|command| format!("onCommand:{command}"))
        .collect::<BTreeSet<_>>();
    if events != expected_events {
        return Err(
            "activationEvents must contain exactly one onCommand event for each contributed command."
                .into(),
        );
    }

    let capability_count = manifest
        .capabilities
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .len();
    if capability_count != manifest.capabilities.len() {
        return Err("Duplicate capabilities are not allowed.".into());
    }

    Ok(ValidatedManifest { id, manifest })
}

fn requested_capabilities(manifest: &ModuleManifest) -> BTreeSet<Capability> {
    manifest.capabilities.iter().copied().collect()
}

fn ensure_modules_root(paths: &ModulePaths) -> Result<PathBuf, String> {
    fs::create_dir_all(&paths.base)
        .map_err(|error| format!("Failed to create app data directory: {error}"))?;
    match fs::symlink_metadata(&paths.modules) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err("The app-owned modules path is not a regular directory.".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&paths.modules)
                .map_err(|error| format!("Failed to create modules directory: {error}"))?;
        }
        Err(error) => return Err(format!("Failed to inspect modules directory: {error}")),
    }
    fs::canonicalize(&paths.modules)
        .map_err(|error| format!("Failed to resolve modules directory: {error}"))
}

fn existing_modules_root(paths: &ModulePaths) -> Result<Option<PathBuf>, String> {
    let metadata = match fs::symlink_metadata(&paths.modules) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Failed to inspect modules directory: {error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("The app-owned modules path is not a regular directory.".into());
    }
    fs::canonicalize(&paths.modules)
        .map(Some)
        .map_err(|error| format!("Failed to resolve modules directory: {error}"))
}

fn canonical_direct_child_directory(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Failed to inspect directory: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Expected a regular directory, not a symlink.".into());
    }
    let canonical =
        fs::canonicalize(path).map_err(|error| format!("Failed to resolve directory: {error}"))?;
    if canonical.parent() != Some(root) {
        return Err("Directory escapes its canonical parent.".into());
    }
    Ok(canonical)
}

fn canonical_regular_child(root: &Path, path: &Path) -> Result<(PathBuf, u64), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("Failed to inspect file: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Expected a regular file, not a symlink.".into());
    }
    let canonical =
        fs::canonicalize(path).map_err(|error| format!("Failed to resolve file: {error}"))?;
    if canonical.parent() != Some(root) {
        return Err("File escapes its canonical package directory.".into());
    }
    Ok((canonical, metadata.len()))
}

fn read_bounded_regular_child(root: &Path, path: &Path, max_bytes: u64) -> Result<Vec<u8>, String> {
    let (canonical, reported_len) = canonical_regular_child(root, path)?;
    if reported_len > max_bytes {
        return Err(format!("File exceeds the {max_bytes}-byte limit."));
    }
    let mut bytes = Vec::with_capacity(reported_len as usize);
    File::open(&canonical)
        .map_err(|error| format!("Failed to open file: {error}"))?
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Failed to read file: {error}"))?;
    if bytes.len() as u64 > max_bytes {
        return Err(format!("File exceeds the {max_bytes}-byte limit."));
    }
    Ok(bytes)
}

fn read_bounded_owned_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("Failed to inspect file: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Expected a regular app-owned file, not a symlink.".into());
    }
    if metadata.len() > max_bytes {
        return Err(format!("File exceeds the {max_bytes}-byte limit."));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .map_err(|error| format!("Failed to open file: {error}"))?
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Failed to read file: {error}"))?;
    if bytes.len() as u64 > max_bytes {
        return Err(format!("File exceeds the {max_bytes}-byte limit."));
    }
    Ok(bytes)
}

fn load_state(paths: &ModulePaths) -> Result<PersistentState, String> {
    match fs::symlink_metadata(&paths.state) {
        Ok(_) => {
            let bytes = read_bounded_owned_file(&paths.state, MAX_STATE_BYTES)?;
            serde_json::from_slice(&bytes)
                .map_err(|error| format!("Invalid modules-state.json: {error}"))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(PersistentState::default())
        }
        Err(error) => Err(format!("Failed to inspect modules-state.json: {error}")),
    }
}

fn unique_sibling(parent: &Path, prefix: &str) -> Result<PathBuf, String> {
    for _ in 0..128 {
        let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!("{prefix}-{}-{sequence}", std::process::id()));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err("Failed to allocate a unique staging path.".into())
}

fn replace_file(target: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| "State file has no parent directory.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create state directory: {error}"))?;
    let staging = unique_sibling(parent, ".modules-state-staging")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging)
        .map_err(|error| format!("Failed to create state staging file: {error}"))?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&staging);
        return Err(format!("Failed to write module state: {error}"));
    }
    drop(file);

    let backup = if target.exists() {
        let metadata = fs::symlink_metadata(target)
            .map_err(|error| format!("Failed to inspect existing module state: {error}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            let _ = fs::remove_file(&staging);
            return Err("Existing modules-state.json is not a regular file.".into());
        }
        let backup = match unique_sibling(parent, ".modules-state-backup") {
            Ok(backup) => backup,
            Err(error) => {
                let _ = fs::remove_file(&staging);
                return Err(error);
            }
        };
        if let Err(error) = fs::rename(target, &backup) {
            let _ = fs::remove_file(&staging);
            return Err(format!("Failed to stage existing module state: {error}"));
        }
        Some(backup)
    } else {
        None
    };

    if let Err(error) = fs::rename(&staging, target) {
        let _ = fs::remove_file(&staging);
        if let Some(backup) = &backup
            && let Err(restore_error) = fs::rename(backup, target)
        {
            return Err(format!(
                "Failed to commit module state: {error}; restoring the prior state also failed: {restore_error}"
            ));
        }
        return Err(format!("Failed to commit module state: {error}"));
    }
    if let Some(backup) = backup {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

fn save_state(paths: &ModulePaths, state: &PersistentState) -> Result<(), String> {
    let bytes = serde_json::to_vec(state)
        .map_err(|error| format!("Failed to serialize module state: {error}"))?;
    replace_file(&paths.state, &bytes)
}

fn scan_installed_modules(paths: &ModulePaths) -> (Vec<InstalledModule>, Vec<String>) {
    let root = match existing_modules_root(paths) {
        Ok(Some(root)) => root,
        Ok(None) => return (Vec::new(), Vec::new()),
        Err(error) => return (Vec::new(), vec![error]),
    };
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) => {
            return (
                Vec::new(),
                vec![format!("Failed to enumerate installed modules: {error}")],
            );
        }
    };
    let mut paths_to_scan = Vec::new();
    let mut errors = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => paths_to_scan.push(entry.path()),
            Err(error) => errors.push(format!("Failed to inspect a module entry: {error}")),
        }
    }
    paths_to_scan.sort();

    let mut modules = Vec::new();
    let mut ids = BTreeSet::new();
    for path in paths_to_scan {
        let folder = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("<non-utf8>")
            .to_string();
        let result = (|| {
            let directory = canonical_direct_child_directory(&root, &path)?;
            let manifest_bytes = read_bounded_regular_child(
                &directory,
                &directory.join(MANIFEST_FILE_NAME),
                MAX_MANIFEST_BYTES,
            )?;
            let validated = parse_manifest(&manifest_bytes)?;
            if folder != validated.id {
                return Err(format!(
                    "Installed folder name {folder} does not match module id {}.",
                    validated.id
                ));
            }
            let (_, wasm_len) =
                canonical_regular_child(&directory, &directory.join(WASM_FILE_NAME))?;
            if wasm_len > MAX_WASM_BYTES {
                return Err(format!(
                    "module.wasm exceeds the {MAX_WASM_BYTES}-byte limit."
                ));
            }
            if !ids.insert(validated.id.clone()) {
                return Err(format!("Duplicate installed module id {}.", validated.id));
            }
            Ok(InstalledModule {
                id: validated.id,
                directory,
                manifest: validated.manifest,
            })
        })();
        match result {
            Ok(module) => modules.push(module),
            Err(error) => errors.push(format!("{folder}: {error}")),
        }
    }
    modules.sort_by(|left, right| left.id.cmp(&right.id));
    (modules, errors)
}

fn catalog_from_parts(
    modules: &[InstalledModule],
    state: &PersistentState,
    errors: Vec<String>,
) -> Catalog {
    Catalog {
        modules: modules
            .iter()
            .map(|module| {
                let requested = requested_capabilities(&module.manifest);
                let granted = state
                    .granted_capabilities
                    .get(&module.id)
                    .cloned()
                    .unwrap_or_default();
                let needs_approval = requested != granted;
                ModuleInfo {
                    id: module.id.clone(),
                    publisher: module.manifest.publisher.clone(),
                    name: module.manifest.name.clone(),
                    display_name: module.manifest.display_name.clone(),
                    version: module.manifest.version.clone(),
                    description: module.manifest.description.clone(),
                    enabled: state.enabled_ids.contains(&module.id) && !needs_approval,
                    needs_approval,
                    capabilities: module.manifest.capabilities.clone(),
                    commands: module.manifest.contributes.commands.clone(),
                }
            })
            .collect(),
        errors,
        policy: ModulePolicy {
            memory_mi_b: (MEMORY_BYTES / (1024 * 1024)) as u32,
            fuel: FUEL_LIMIT,
            max_input_bytes: MAX_INPUT_BYTES,
            max_output_bytes: MAX_OUTPUT_BYTES,
        },
    }
}

fn catalog_at(paths: &ModulePaths) -> Catalog {
    let (state, state_error) = match load_state(paths) {
        Ok(state) => (state, None),
        Err(error) => (PersistentState::default(), Some(error)),
    };
    let (modules, mut errors) = scan_installed_modules(paths);
    if let Some(error) = state_error {
        errors.insert(0, error);
    }
    catalog_from_parts(&modules, &state, errors)
}

#[tauri::command]
pub(crate) fn list_modules(app: AppHandle) -> Result<Catalog, String> {
    let paths = paths_for_app(&app)?;
    let _guard = registry()
        .lock()
        .map_err(|_| "Module registry lock is poisoned.".to_string())?;
    let _process_lock = acquire_process_lock(&paths)?;
    Ok(catalog_at(&paths))
}

fn update_install_state(state: &mut PersistentState, module: &ValidatedManifest, _is_update: bool) {
    // Unsigned packages cannot safely inherit trust from a previous package with the same ID.
    state.enabled_ids.remove(&module.id);
    state.granted_capabilities.remove(&module.id);
}

fn swap_staged_directory(
    root: &Path,
    staging: &Path,
    target: &Path,
) -> Result<Option<PathBuf>, String> {
    let backup = if target.exists() {
        canonical_direct_child_directory(root, target)?;
        let backup = unique_sibling(root, ".module-backup")?;
        fs::rename(target, &backup)
            .map_err(|error| format!("Failed to stage existing module: {error}"))?;
        Some(backup)
    } else {
        None
    };
    if let Err(error) = fs::rename(staging, target) {
        if let Some(backup) = &backup {
            if let Err(restore_error) = fs::rename(backup, target) {
                return Err(format!(
                    "Failed to commit module package: {error}; restoring the prior module also failed: {restore_error}"
                ));
            }
        }
        return Err(format!("Failed to commit module package: {error}"));
    }
    Ok(backup)
}

fn rollback_installed_directory(target: &Path, backup: Option<&Path>) -> Result<(), String> {
    fs::remove_dir_all(target)
        .map_err(|error| format!("Failed to remove the uncommitted module: {error}"))?;
    if let Some(backup) = backup {
        fs::rename(backup, target)
            .map_err(|error| format!("Failed to restore the prior module: {error}"))?;
    }
    Ok(())
}

fn module_fingerprint(bytes: &[u8]) -> CacheKey {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    CacheKey {
        fingerprint: hasher.finish(),
        byte_len: bytes.len(),
    }
}

fn cache_store(
    module_id: &str,
    key: CacheKey,
    compiled: CompiledModule,
    wasm_len: usize,
) -> Result<(), String> {
    let runtime = module_runtime();
    let mut cache = runtime
        .cache
        .lock()
        .map_err(|_| "Compiled module cache lock is poisoned.".to_string())?;
    if let Some(index) = cache
        .entries
        .iter()
        .position(|entry| entry.module_id == module_id)
    {
        let removed = cache.entries.remove(index).expect("cache index");
        cache.total_source_bytes = cache.total_source_bytes.saturating_sub(removed.wasm_len);
    }
    cache.total_source_bytes = cache.total_source_bytes.saturating_add(wasm_len);
    cache.entries.push_back(CacheEntry {
        module_id: module_id.to_string(),
        key,
        engine: compiled.engine,
        module: compiled.module,
        wasm_len,
    });
    while cache.entries.len() > MAX_CACHE_ENTRIES
        || cache.total_source_bytes > MAX_CACHE_SOURCE_BYTES
    {
        let Some(removed) = cache.entries.pop_front() else {
            break;
        };
        cache.total_source_bytes = cache.total_source_bytes.saturating_sub(removed.wasm_len);
    }
    Ok(())
}

fn cached_or_compile(module_id: &str, wasm: &[u8]) -> Result<CompiledModule, String> {
    let runtime = module_runtime();
    let key = module_fingerprint(wasm);
    {
        let mut cache = runtime
            .cache
            .lock()
            .map_err(|_| "Compiled module cache lock is poisoned.".to_string())?;
        if let Some(index) = cache
            .entries
            .iter()
            .position(|entry| entry.module_id == module_id && entry.key == key)
        {
            let entry = cache.entries.remove(index).expect("cache index");
            let compiled = CompiledModule {
                engine: entry.engine.clone(),
                module: entry.module.clone(),
            };
            cache.entries.push_back(entry);
            return Ok(compiled);
        }
    }
    let engine = new_module_engine();
    let module = Module::new(&engine, wasm)
        .map_err(|error| format!("Failed to compile module.wasm: {error}"))?;
    let compiled = CompiledModule { engine, module };
    cache_store(module_id, key, compiled.clone(), wasm.len())?;
    Ok(compiled)
}

fn install_selected(paths: &ModulePaths, selected: &Path) -> Result<Catalog, String> {
    if selected.file_name().and_then(|value| value.to_str()) != Some(MANIFEST_FILE_NAME) {
        return Err("Select the package's direct-child module.json file.".into());
    }
    let source_parent = selected
        .parent()
        .ok_or_else(|| "Selected module.json has no package directory.".to_string())?;
    let source_metadata = fs::symlink_metadata(source_parent)
        .map_err(|error| format!("Failed to inspect package directory: {error}"))?;
    if source_metadata.file_type().is_symlink() || !source_metadata.is_dir() {
        return Err("The package directory must be a regular directory, not a symlink.".into());
    }
    let source_root = fs::canonicalize(source_parent)
        .map_err(|error| format!("Failed to resolve package directory: {error}"))?;
    let manifest_bytes = read_bounded_regular_child(&source_root, selected, MAX_MANIFEST_BYTES)?;
    let validated = parse_manifest(&manifest_bytes)?;
    let wasm_bytes = read_bounded_regular_child(
        &source_root,
        &source_root.join(WASM_FILE_NAME),
        MAX_WASM_BYTES,
    )?;
    let engine = new_module_engine();
    let module = Module::new(&engine, &wasm_bytes)
        .map_err(|error| format!("Failed to compile module.wasm: {error}"))?;
    let compiled = CompiledModule { engine, module };

    let _guard = registry()
        .lock()
        .map_err(|_| "Module registry lock is poisoned.".to_string())?;
    let _process_lock = acquire_process_lock(paths)?;
    let root = ensure_modules_root(paths)?;
    let target = root.join(&validated.id);
    let gate = module_gate(&validated.id)?;
    let _module_permit = gate
        .try_lock()
        .map_err(|_| format!("Module {} is already running.", validated.id))?;
    let is_update = target.exists();
    if is_update {
        canonical_direct_child_directory(&root, &target)?;
    }
    let mut state = load_state(paths)?;
    update_install_state(&mut state, &validated, is_update);

    let staging = unique_sibling(&root, ".module-staging")?;
    fs::create_dir(&staging)
        .map_err(|error| format!("Failed to create module staging directory: {error}"))?;
    let stage_result = fs::write(staging.join(MANIFEST_FILE_NAME), &manifest_bytes)
        .and_then(|_| fs::write(staging.join(WASM_FILE_NAME), &wasm_bytes));
    if let Err(error) = stage_result {
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("Failed to stage module package: {error}"));
    }

    let backup = match swap_staged_directory(&root, &staging, &target) {
        Ok(backup) => backup,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    if let Err(error) = save_state(paths, &state) {
        if let Err(rollback_error) = rollback_installed_directory(&target, backup.as_deref()) {
            return Err(format!(
                "{error}; rolling back the module package also failed: {rollback_error}"
            ));
        }
        return Err(error);
    }
    if let Some(backup) = backup {
        let _ = fs::remove_dir_all(backup);
    }
    cache_store(
        &validated.id,
        module_fingerprint(&wasm_bytes),
        compiled,
        wasm_bytes.len(),
    )?;
    Ok(catalog_at(paths))
}

#[tauri::command]
pub(crate) fn install_module(app: AppHandle) -> Result<Option<Catalog>, String> {
    let Some(selected) = FileDialog::new()
        .add_filter("Hematite module manifest", &["json"])
        .pick_file()
    else {
        return Ok(None);
    };
    let paths = paths_for_app(&app)?;
    install_selected(&paths, &selected).map(Some)
}

#[tauri::command]
pub(crate) fn set_module_enabled(
    app: AppHandle,
    request: SetModuleEnabledRequest,
) -> Result<Catalog, String> {
    let module_id = request.module_id;
    validate_module_id(&module_id)?;
    let paths = paths_for_app(&app)?;
    let _guard = registry()
        .lock()
        .map_err(|_| "Module registry lock is poisoned.".to_string())?;
    let _process_lock = acquire_process_lock(&paths)?;
    let gate = module_gate(&module_id)?;
    let _module_permit = gate
        .try_lock()
        .map_err(|_| format!("Module {module_id} is already running."))?;
    let (modules, _) = scan_installed_modules(&paths);
    let module = modules
        .iter()
        .find(|module| module.id == module_id)
        .ok_or_else(|| format!("Module {module_id} is not installed."))?;
    let mut state = load_state(&paths)?;
    if request.enabled {
        state.enabled_ids.insert(module_id.clone());
        state
            .granted_capabilities
            .insert(module_id, requested_capabilities(&module.manifest));
    } else {
        state.enabled_ids.remove(&module_id);
    }
    save_state(&paths, &state)?;
    Ok(catalog_at(&paths))
}

fn validate_module_id(module_id: &str) -> Result<(), String> {
    let Some((publisher, name)) = module_id.split_once('.') else {
        return Err("moduleId must be the computed publisher.name id.".into());
    };
    if name.contains('.') {
        return Err("moduleId must contain exactly one dot.".into());
    }
    validate_id_segment("publisher", publisher)?;
    validate_id_segment("name", name)?;
    if publisher == "hematite" {
        return Err("The publisher name hematite is reserved.".into());
    }
    if module_id.len() > MAX_MODULE_ID_BYTES {
        return Err(format!("moduleId exceeds {MAX_MODULE_ID_BYTES} bytes."));
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn uninstall_module(
    app: AppHandle,
    request: UninstallModuleRequest,
) -> Result<Catalog, String> {
    validate_module_id(&request.module_id)?;
    let paths = paths_for_app(&app)?;
    let _guard = registry()
        .lock()
        .map_err(|_| "Module registry lock is poisoned.".to_string())?;
    let _process_lock = acquire_process_lock(&paths)?;
    let gate = module_gate(&request.module_id)?;
    let _module_permit = gate
        .try_lock()
        .map_err(|_| format!("Module {} is already running.", request.module_id))?;
    let root = existing_modules_root(&paths)?
        .ok_or_else(|| format!("Module {} is not installed.", request.module_id))?;
    let target = root.join(&request.module_id);
    canonical_direct_child_directory(&root, &target)
        .map_err(|_| format!("Module {} is not installed safely.", request.module_id))?;
    fs::create_dir_all(&paths.base)
        .map_err(|error| format!("Failed to create app data directory: {error}"))?;
    let backup = unique_sibling(&paths.base, ".module-uninstall")?;
    fs::rename(&target, &backup)
        .map_err(|error| format!("Failed to stage module removal: {error}"))?;

    let mut state = match load_state(&paths) {
        Ok(state) => state,
        Err(error) => {
            if let Err(restore_error) = fs::rename(&backup, &target) {
                return Err(format!(
                    "{error}; restoring the staged module also failed: {restore_error}"
                ));
            }
            return Err(error);
        }
    };
    state.enabled_ids.remove(&request.module_id);
    state.granted_capabilities.remove(&request.module_id);
    if let Err(error) = save_state(&paths, &state) {
        if let Err(restore_error) = fs::rename(&backup, &target) {
            return Err(format!(
                "{error}; restoring the staged module also failed: {restore_error}"
            ));
        }
        return Err(error);
    }
    fs::remove_dir_all(&backup).map_err(|error| {
        format!("Module state was removed, but package cleanup failed: {error}")
    })?;
    if let Ok(mut cache) = module_runtime().cache.lock() {
        if let Some(index) = cache
            .entries
            .iter()
            .position(|entry| entry.module_id == request.module_id)
        {
            let removed = cache.entries.remove(index).expect("cache index");
            cache.total_source_bytes = cache.total_source_bytes.saturating_sub(removed.wasm_len);
        }
    }
    Ok(catalog_at(&paths))
}

fn execute_request(
    paths: &ModulePaths,
    request: ExecuteModuleCommandRequest,
) -> Result<ExecuteModuleCommandResult, String> {
    if request.command.is_empty() {
        return Err("command must not be empty.".into());
    }
    if request.input_json.len() > MAX_INPUT_BYTES {
        return Err(format!(
            "inputJson exceeds the {MAX_INPUT_BYTES}-byte limit."
        ));
    }
    let input_value: Value = serde_json::from_str(&request.input_json)
        .map_err(|error| format!("inputJson is not valid JSON: {error}"))?;
    let envelope = serde_json::to_vec(&json!({
        "command": request.command,
        "input": input_value,
    }))
    .map_err(|error| format!("Failed to serialize module input: {error}"))?;
    if envelope.len() > MAX_INPUT_BYTES {
        return Err(format!(
            "Serialized module input exceeds the {MAX_INPUT_BYTES}-byte limit."
        ));
    }

    let _guard = registry()
        .lock()
        .map_err(|_| "Module registry lock is poisoned.".to_string())?;
    let process_lock = acquire_process_lock(paths)?;
    let state = load_state(paths)?;
    let (modules, _) = scan_installed_modules(paths);
    let matches = modules
        .iter()
        .filter(|module| {
            module
                .manifest
                .contributes
                .commands
                .iter()
                .any(|command| command.command == request.command)
        })
        .collect::<Vec<_>>();
    let module = match matches.as_slice() {
        [] => {
            return Err(format!(
                "No installed module contributes {}.",
                request.command
            ));
        }
        [module] => *module,
        _ => {
            return Err(format!(
                "Command {} is contributed by multiple installed modules.",
                request.command
            ));
        }
    };
    let gate = module_gate(&module.id)?;
    let _module_permit = gate
        .try_lock()
        .map_err(|_| format!("Module {} is already running.", module.id))?;
    let requested = requested_capabilities(&module.manifest);
    let granted = state
        .granted_capabilities
        .get(&module.id)
        .cloned()
        .unwrap_or_default();
    if requested != granted {
        return Err(format!(
            "Module {} requires capability approval before execution.",
            module.id
        ));
    }
    if !state.enabled_ids.contains(&module.id) {
        return Err(format!("Module {} is disabled.", module.id));
    }
    let wasm = read_bounded_regular_child(
        &module.directory,
        &module.directory.join(WASM_FILE_NAME),
        MAX_WASM_BYTES,
    )?;
    let module_id = module.id.clone();
    let command = request.command;
    drop(process_lock);
    drop(_guard);

    let started = Instant::now();
    let mut result = execute_wasm(&module_id, &command, &wasm, &envelope, granted)?;
    result.duration_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    Ok(result)
}

fn module_gate(module_id: &str) -> Result<Arc<Mutex<()>>, String> {
    let runtime = module_runtime();
    let mut gates = runtime
        .module_gates
        .lock()
        .map_err(|_| "Module concurrency registry lock is poisoned.".to_string())?;
    gates.retain(|_, gate| gate.strong_count() > 0);
    if let Some(gate) = gates.get(module_id).and_then(Weak::upgrade) {
        return Ok(gate);
    }
    let gate = Arc::new(Mutex::new(()));
    gates.insert(module_id.to_string(), Arc::downgrade(&gate));
    Ok(gate)
}

fn host_violation(caller: &mut Caller<'_, HostState>, message: impl Into<String>) -> i32 {
    let state = caller.data_mut();
    if state.violation.is_none() {
        state.violation = Some(message.into());
    }
    -1
}

fn host_call(
    mut caller: Caller<'_, HostState>,
    op: i32,
    request_ptr: i32,
    request_len: i32,
    output_ptr: i32,
    output_capacity: i32,
) -> i32 {
    {
        let state = caller.data_mut();
        state.host_calls = state.host_calls.saturating_add(1);
        if state.host_calls > MAX_HOST_CALLS {
            return host_violation(&mut caller, "Host call limit exceeded.");
        }
        if !state.granted_capabilities.contains(&Capability::Log) {
            return host_violation(&mut caller, "The log capability was not granted.");
        }
        if state.logs.len() >= MAX_LOGS {
            return host_violation(&mut caller, "Log entry limit exceeded.");
        }
    }
    if output_ptr != 0 || output_capacity != 0 {
        return host_violation(
            &mut caller,
            "Reserved host_call output arguments must be zero.",
        );
    }
    let level = match op {
        1 => LogLevel::Info,
        2 => LogLevel::Warn,
        3 => LogLevel::Error,
        _ => return host_violation(&mut caller, format!("Unsupported host operation {op}.")),
    };
    if request_ptr < 0 || request_len < 0 || request_len as usize > MAX_LOG_BYTES {
        return host_violation(&mut caller, "Invalid log pointer or length.");
    }
    let Some(memory) = caller
        .get_export("memory")
        .and_then(|export| export.into_memory())
    else {
        return host_violation(&mut caller, "Guest memory export is unavailable.");
    };
    let start = request_ptr as usize;
    let len = request_len as usize;
    let Some(end) = start.checked_add(len) else {
        return host_violation(&mut caller, "Log memory range overflowed.");
    };
    if end > memory.data_size(&caller) {
        return host_violation(&mut caller, "Log memory range is out of bounds.");
    }
    let mut bytes = vec![0; len];
    if memory.read(&caller, start, &mut bytes).is_err() {
        return host_violation(&mut caller, "Failed to read guest log memory.");
    }
    let Ok(message) = String::from_utf8(bytes) else {
        return host_violation(&mut caller, "Guest log message is not UTF-8.");
    };
    caller.data_mut().logs.push(ModuleLog { level, message });
    0
}

fn build_wasi_context(
    module_id: &str,
    command: &str,
    input: &[u8],
) -> Result<(WasiCtx, WasiStreams), String> {
    let stdout = WasiOutput::new();
    let stderr = WasiOutput::new();
    let mut builder = WasiCtxBuilder::new();
    builder
        .arg(module_id)
        .map_err(|error| format!("Failed to set WASI module argument: {error}"))?;
    builder
        .arg(command)
        .map_err(|error| format!("Failed to set WASI command argument: {error}"))?;
    builder
        .stdin(Box::new(ReadPipe::from(input)))
        .stdout(Box::new(WritePipe::new(stdout.clone())))
        .stderr(Box::new(WritePipe::new(stderr.clone())));
    Ok((builder.build(), WasiStreams { stdout, stderr }))
}

fn wasi_context_mut(state: &mut HostState) -> &mut WasiCtx {
    state.wasi.get_or_insert_with(|| {
        let mut builder = WasiCtxBuilder::new();
        builder.build()
    })
}

fn append_wasi_stream_logs(logs: &mut Vec<ModuleLog>, bytes: &[u8], level: LogLevel) {
    for chunk in bytes.chunks(MAX_LOG_BYTES) {
        if chunk.is_empty() || logs.len() >= MAX_LOGS {
            break;
        }
        let mut message = String::from_utf8_lossy(chunk).into_owned();
        if message.len() > MAX_LOG_BYTES {
            let mut end = MAX_LOG_BYTES;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
        }
        logs.push(ModuleLog { level, message });
    }
}

fn append_wasi_logs(streams: &WasiStreams, logs: &mut Vec<ModuleLog>) -> Result<(), String> {
    append_wasi_stream_logs(logs, &streams.stdout.snapshot()?, LogLevel::Info);
    append_wasi_stream_logs(logs, &streams.stderr.snapshot()?, LogLevel::Error);
    Ok(())
}

fn execute_wasm(
    module_id: &str,
    command: &str,
    wasm: &[u8],
    input: &[u8],
    granted_capabilities: BTreeSet<Capability>,
) -> Result<ExecuteModuleCommandResult, String> {
    let compiled = cached_or_compile(module_id, wasm)?;
    let wasi_enabled = granted_capabilities.contains(&Capability::Wasi);
    let (wasi, wasi_streams) = if wasi_enabled {
        let (context, streams) = build_wasi_context(module_id, command, input)?;
        (Some(context), Some(streams))
    } else {
        (None, None)
    };
    let limits = StoreLimitsBuilder::new()
        .memory_size(MEMORY_BYTES)
        .table_elements(MAX_TABLE_ELEMENTS)
        .instances(1)
        .tables(1)
        .memories(1)
        .build();
    let mut store = Store::new(
        &compiled.engine,
        HostState {
            limits,
            granted_capabilities,
            logs: Vec::new(),
            host_calls: 0,
            violation: None,
            wasi,
        },
    );
    store.limiter(|state| &mut state.limits);
    store
        .set_fuel(FUEL_LIMIT)
        .map_err(|error| format!("Failed to set module fuel: {error}"))?;

    let mut linker = Linker::new(&compiled.engine);
    linker
        .func_wrap("hematite", "host_call", host_call)
        .map_err(|error| format!("Failed to link module host ABI: {error}"))?;
    if wasi_enabled {
        wasmi_wasi::add_to_linker(&mut linker, wasi_context_mut)
            .map_err(|error| format!("Failed to link WASI Preview 1: {error}"))?;
    }
    let instance = linker
        .instantiate_and_start(&mut store, &compiled.module)
        .map_err(|error| format!("Failed to instantiate module: {error}"))?;
    if wasi_enabled && instance.get_export(&store, "_initialize").is_some() {
        instance
            .get_typed_func::<(), ()>(&store, "_initialize")
            .map_err(|_| "_initialize must have type ()->().".to_string())?
            .call(&mut store, ())
            .map_err(|error| format!("WASI reactor initialization trapped: {error}"))?;
    }
    let memory = instance
        .get_export(&store, "memory")
        .and_then(|export| export.into_memory())
        .ok_or_else(|| "Module must export memory.".to_string())?;
    let alloc = instance
        .get_typed_func::<i32, i32>(&store, "hematite_alloc")
        .map_err(|_| "Module must export hematite_alloc(i32)->i32.".to_string())?;
    let dispatch = instance
        .get_typed_func::<(i32, i32), i64>(&store, "hematite_dispatch")
        .map_err(|_| "Module must export hematite_dispatch(i32,i32)->i64.".to_string())?;
    if instance.get_export(&store, "hematite_dealloc").is_some() {
        instance
            .get_typed_func::<(i32, i32), ()>(&store, "hematite_dealloc")
            .map_err(|_| "hematite_dealloc must have type (i32,i32)->().".to_string())?;
    }

    let input_len =
        i32::try_from(input.len()).map_err(|_| "Module input length is invalid.".to_string())?;
    let input_ptr = alloc
        .call(&mut store, input_len)
        .map_err(|error| format!("Module allocation trapped: {error}"))?;
    if input_ptr < 0 {
        return Err("hematite_alloc returned a negative pointer.".into());
    }
    let input_start = input_ptr as usize;
    let input_end = input_start
        .checked_add(input.len())
        .ok_or_else(|| "Module input memory range overflowed.".to_string())?;
    if input_end > memory.data_size(&store) {
        return Err("hematite_alloc returned an out-of-bounds range.".into());
    }
    memory
        .write(&mut store, input_start, input)
        .map_err(|error| format!("Failed to write module input: {error}"))?;

    let packed_result = dispatch.call(&mut store, (input_ptr, input_len));
    if let Some(violation) = store.data().violation.as_ref() {
        return Err(format!("Module host-call violation: {violation}"));
    }
    let packed = packed_result.map_err(|error| format!("Module dispatch trapped: {error}"))? as u64;
    let output_ptr = (packed & u32::MAX as u64) as usize;
    let output_len = (packed >> 32) as usize;
    if output_len > MAX_OUTPUT_BYTES {
        return Err(format!(
            "Module output exceeds the {MAX_OUTPUT_BYTES}-byte limit."
        ));
    }
    let output_end = output_ptr
        .checked_add(output_len)
        .ok_or_else(|| "Module output memory range overflowed.".to_string())?;
    if output_end > memory.data_size(&store) {
        return Err("Module returned an out-of-bounds output range.".into());
    }
    let mut output_bytes = vec![0; output_len];
    memory
        .read(&store, output_ptr, &mut output_bytes)
        .map_err(|error| format!("Failed to read module output: {error}"))?;
    let output = String::from_utf8(output_bytes)
        .map_err(|_| "Module output is not valid UTF-8.".to_string())?;
    let fuel_remaining = store
        .get_fuel()
        .map_err(|error| format!("Failed to read module fuel: {error}"))?;
    let fuel_consumed = FUEL_LIMIT.saturating_sub(fuel_remaining);
    let mut logs = std::mem::take(&mut store.data_mut().logs);
    if let Some(streams) = wasi_streams.as_ref() {
        append_wasi_logs(streams, &mut logs)?;
    }
    Ok(ExecuteModuleCommandResult {
        module_id: module_id.to_string(),
        command: command.to_string(),
        output,
        logs,
        fuel_consumed,
        duration_ms: 0,
    })
}

#[tauri::command]
pub(crate) async fn execute_module_command(
    app: AppHandle,
    request: ExecuteModuleCommandRequest,
) -> Result<ExecuteModuleCommandResult, String> {
    let paths = paths_for_app(&app)?;
    let active_permit = ActivePermit::try_acquire(module_runtime())?;
    tauri::async_runtime::spawn_blocking(move || {
        let _active_permit = active_permit;
        execute_request(&paths, request)
    })
    .await
    .map_err(|error| format!("Module execution worker failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_manifest(capabilities: &str) -> Vec<u8> {
        format!(
            r#"{{
                "manifestVersion": 1,
                "abiVersion": 1,
                "publisher": "sample",
                "name": "hello",
                "displayName": "Hello",
                "version": "1.0.0",
                "description": "test",
                "entry": "module.wasm",
                "activationEvents": ["onCommand:sample.hello"],
                "capabilities": {capabilities},
                "contributes": {{
                    "commands": [{{
                        "command": "sample.hello",
                        "title": "Run"
                    }}]
                }}
            }}"#
        )
        .into_bytes()
    }

    #[test]
    fn valid_manifest_and_dispatch_work() {
        let parsed = parse_manifest(&valid_manifest(r#"["log"]"#)).expect("valid manifest");
        assert_eq!(parsed.id, "sample.hello");
        let wasm = wat::parse_str(
            r#"(module
                (import "hematite" "host_call"
                    (func $host_call (param i32 i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 16) "hello")
                (data (i32.const 64) "ok")
                (func (export "hematite_alloc") (param i32) (result i32)
                    i32.const 128)
                (func (export "hematite_dispatch") (param i32 i32) (result i64)
                    i32.const 1
                    i32.const 16
                    i32.const 5
                    i32.const 0
                    i32.const 0
                    call $host_call
                    drop
                    i64.const 8589934656))"#,
        )
        .expect("wat");
        let result = execute_wasm(
            "sample.hello",
            "sample.hello.run",
            &wasm,
            br#"{"command":"sample.hello.run","input":null}"#,
            BTreeSet::from([Capability::Log]),
        )
        .expect("dispatch");
        assert_eq!(result.output, "ok");
        assert_eq!(result.logs.len(), 1);
        assert_eq!(result.logs[0].message, "hello");
        assert!(result.fuel_consumed > 0);
    }

    #[test]
    fn wasi_preview1_is_capability_gated_and_captured() {
        assert!(parse_manifest(&valid_manifest(r#"["wasi"]"#)).is_ok());
        let wasm = wat::parse_str(
            r#"(module
                (import "wasi_snapshot_preview1" "environ_sizes_get"
                    (func $environ_sizes_get (param i32 i32) (result i32)))
                (import "wasi_snapshot_preview1" "fd_write"
                    (func $fd_write (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 16) "wasi-out")
                (data (i32.const 32) "\10\00\00\00\08\00\00\00")
                (data (i32.const 64) "ok")
                (func (export "hematite_alloc") (param i32) (result i32)
                    i32.const 128)
                (func (export "hematite_dispatch") (param i32 i32) (result i64)
                    i32.const 48
                    i32.const 52
                    call $environ_sizes_get
                    drop
                    i32.const 1
                    i32.const 32
                    i32.const 1
                    i32.const 56
                    call $fd_write
                    drop
                    i64.const 8589934656))"#,
        )
        .expect("wat");

        let error = execute_wasm(
            "sample.wasi-denied",
            "sample.wasi-denied.run",
            &wasm,
            b"null",
            BTreeSet::new(),
        )
        .expect_err("WASI must be capability gated");
        assert!(error.contains("wasi_snapshot_preview1"), "{error}");

        let result = execute_wasm(
            "sample.wasi",
            "sample.wasi.run",
            &wasm,
            b"null",
            BTreeSet::from([Capability::Wasi]),
        )
        .expect("WASI dispatch");
        assert_eq!(result.output, "ok");
        assert_eq!(result.logs.len(), 1);
        assert_eq!(result.logs[0].message, "wasi-out");
    }

    #[test]
    fn infinite_loop_is_stopped_by_fuel() {
        let wasm = wat::parse_str(
            r#"(module
                (memory (export "memory") 1)
                (func (export "hematite_alloc") (param i32) (result i32)
                    i32.const 0)
                (func (export "hematite_dispatch") (param i32 i32) (result i64)
                    (loop $forever
                        br $forever)
                    unreachable))"#,
        )
        .expect("wat");
        let error = execute_wasm(
            "sample.loop",
            "sample.loop.run",
            &wasm,
            b"null",
            BTreeSet::new(),
        )
        .expect_err("fuel trap");
        assert!(error.to_ascii_lowercase().contains("fuel"), "{error}");
    }

    #[test]
    fn ids_capabilities_and_paths_are_strict() {
        assert!(validate_id_segment("publisher", "lower-case9").is_ok());
        assert!(validate_id_segment("publisher", "Upper").is_err());
        assert!(validate_module_id("sample.hello.extra").is_err());
        assert!(parse_manifest(&valid_manifest(r#"["network"]"#)).is_err());

        let root = std::env::temp_dir().join(format!(
            "hematite-module-path-test-{}-{}",
            std::process::id(),
            STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let nested = root.join("nested");
        fs::create_dir_all(&nested).expect("temp directories");
        fs::write(nested.join("module.json"), valid_manifest("[]")).expect("temp manifest");
        let canonical_root = fs::canonicalize(&root).expect("canonical root");
        let error = read_bounded_regular_child(
            &canonical_root,
            &nested.join("module.json"),
            MAX_MANIFEST_BYTES,
        )
        .expect_err("nested package file");
        assert!(error.contains("escapes"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unsigned_update_requires_reapproval() {
        let with_log = parse_manifest(&valid_manifest(r#"["log"]"#)).expect("manifest");
        let mut state = PersistentState::default();
        state.enabled_ids.insert(with_log.id.clone());
        state
            .granted_capabilities
            .insert(with_log.id.clone(), BTreeSet::from([Capability::Log]));
        update_install_state(&mut state, &with_log, true);
        assert!(!state.enabled_ids.contains(&with_log.id));
        assert!(!state.granted_capabilities.contains_key(&with_log.id));
    }

    #[test]
    fn strict_compiler_limits_are_enforced() {
        let params = "(param i32)".repeat(33);
        let wasm = wat::parse_str(format!("(module (func {params}))")).expect("wat");
        assert!(Module::new(&new_module_engine(), &wasm).is_err());
    }

    #[test]
    fn output_and_memory_limits_are_enforced() {
        let oversized_output = wat::parse_str(
            r#"(module
                (memory (export "memory") 1)
                (func (export "hematite_alloc") (param i32) (result i32)
                    i32.const 0)
                (func (export "hematite_dispatch") (param i32 i32) (result i64)
                    i64.const 1125904201809920))"#,
        )
        .expect("wat");
        let error = execute_wasm(
            "sample.output",
            "sample.output.run",
            &oversized_output,
            b"null",
            BTreeSet::new(),
        )
        .expect_err("output limit");
        assert!(error.contains("output exceeds"), "{error}");

        let oversized_memory = wat::parse_str(
            r#"(module
                (memory (export "memory") 513)
                (func (export "hematite_alloc") (param i32) (result i32)
                    i32.const 0)
                (func (export "hematite_dispatch") (param i32 i32) (result i64)
                    i64.const 0))"#,
        )
        .expect("wat");
        let error = execute_wasm(
            "sample.memory",
            "sample.memory.run",
            &oversized_memory,
            b"null",
            BTreeSet::new(),
        )
        .expect_err("memory limit");
        assert!(
            error.to_ascii_lowercase().contains("memory")
                || error.to_ascii_lowercase().contains("resource"),
            "{error}"
        );
    }
}
