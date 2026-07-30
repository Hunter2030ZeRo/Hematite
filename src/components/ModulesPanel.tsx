import { invoke } from "@tauri-apps/api/core";
import { For, Show, createSignal, onMount } from "solid-js";

type ModuleCommand = {
  command: string;
  title: string;
  category?: string | null;
};

type ModuleInfo = {
  id: string;
  publisher: string;
  name: string;
  displayName: string;
  version: string;
  description: string;
  enabled: boolean;
  needsApproval: boolean;
  capabilities: string[];
  commands: ModuleCommand[];
};

type ModulePolicy = {
  memoryMiB: number;
  fuel: number;
  maxInputBytes: number;
  maxOutputBytes: number;
};

type ModuleCatalog = {
  modules: ModuleInfo[];
  errors: string[];
  policy: ModulePolicy;
};

type ModuleExecution = {
  moduleId: string;
  command: string;
  output: string;
  logs: Array<{ level: string; message: string }>;
  fuelConsumed: number;
  durationMs: number;
};

type ModulesPanelProps = {
  onStatus: (message: string) => void;
};

export default function ModulesPanel(props: ModulesPanelProps) {
  const [catalog, setCatalog] = createSignal<ModuleCatalog | null>(null);
  const [operation, setOperation] = createSignal<string | null>(null);
  const [errorMessage, setErrorMessage] = createSignal("");
  const [inputJson, setInputJson] = createSignal("{}");
  const [execution, setExecution] = createSignal<ModuleExecution | null>(null);

  const isBusy = () => operation() !== null;

  async function runOperation<T>(
    key: string,
    failureMessage: string,
    action: () => Promise<T>
  ): Promise<T | undefined> {
    setOperation(key);
    setErrorMessage("");
    try {
      return await action();
    } catch (error) {
      const message = `${failureMessage}: ${String(error)}`;
      setErrorMessage(message);
      props.onStatus(message);
      return undefined;
    } finally {
      setOperation(null);
    }
  }

  async function refreshModules() {
    const next = await runOperation("refresh", "Could not list modules", () =>
      invoke<ModuleCatalog>("list_modules")
    );
    if (!next) {
      return;
    }
    setCatalog(next);
    props.onStatus(`Refreshed module catalog (${next.modules.length} installed).`);
  }

  async function installModule() {
    const next = await runOperation("install", "Could not install module", () =>
      invoke<ModuleCatalog | null>("install_module")
    );
    if (next === undefined) {
      return;
    }
    if (next === null) {
      props.onStatus("Module installation canceled.");
      return;
    }
    setCatalog(next);
    props.onStatus("Installed module and refreshed the catalog.");
  }

  async function setModuleEnabled(module: ModuleInfo, enabled: boolean) {
    if (
      enabled &&
      module.capabilities.length > 0 &&
      !window.confirm(
        `${module.displayName} requests these host capabilities:\n\n- ${module.capabilities.join(
          "\n- "
        )}\n\nEnable the module and grant these capabilities?`
      )
    ) {
      return false;
    }

    const next = await runOperation("toggle", "Could not update module", () =>
      invoke<ModuleCatalog>("set_module_enabled", {
        request: { moduleId: module.id, enabled },
      })
    );
    if (!next) {
      return false;
    }
    setCatalog(next);
    props.onStatus(`${enabled ? "Enabled" : "Disabled"} ${module.displayName}.`);
    return true;
  }

  async function uninstallModule(module: ModuleInfo) {
    if (!window.confirm(`Uninstall ${module.displayName} (${module.id})?`)) {
      return;
    }
    const next = await runOperation("uninstall", "Could not uninstall module", () =>
      invoke<ModuleCatalog>("uninstall_module", {
        request: { moduleId: module.id },
      })
    );
    if (!next) {
      return;
    }
    setCatalog(next);
    props.onStatus(`Uninstalled ${module.displayName} and refreshed the catalog.`);
  }

  async function executeCommand(command: ModuleCommand) {
    const input = inputJson().trim();
    try {
      JSON.parse(input);
    } catch {
      const message = "Module command input must be valid JSON.";
      setErrorMessage(message);
      props.onStatus(message);
      return;
    }

    const next = await runOperation("execute", "Could not execute module command", () =>
      invoke<ModuleExecution>("execute_module_command", {
        request: { command: command.command, inputJson: input },
      })
    );
    if (!next) {
      return;
    }
    setExecution(next);
    props.onStatus(`Ran ${next.command} in ${next.durationMs.toLocaleString()} ms.`);
  }

  onMount(() => void refreshModules());

  return (
    <section class="modules-panel panel-section no-divider" aria-label="WebAssembly modules">
      <div class="pane-header compact modules-header">
        <div>
          <div class="pane-title">Modules</div>
          <div class="pane-caption">Portable WebAssembly extensions</div>
        </div>
        <div class="modules-header-actions">
          <button
            type="button"
            class="pane-button"
            disabled={isBusy()}
            onClick={() => void installModule()}
          >
            {operation() === "install" ? "Installing..." : "Install"}
          </button>
          <button
            type="button"
            class="pane-button"
            disabled={isBusy()}
            onClick={() => void refreshModules()}
          >
            {operation() === "refresh" ? "Refreshing..." : "Refresh"}
          </button>
        </div>
      </div>

      <Show when={catalog()?.policy}>
        {(policy) => (
          <div class="modules-policy" aria-label="Module host policy">
            <div class="modules-policy-item">
              <span>Memory</span>
              <strong>{policy().memoryMiB} MiB</strong>
            </div>
            <div class="modules-policy-item">
              <span>Fuel</span>
              <strong>{policy().fuel.toLocaleString()}</strong>
            </div>
            <div class="modules-policy-item">
              <span>Input</span>
              <strong>{policy().maxInputBytes.toLocaleString()} B</strong>
            </div>
            <div class="modules-policy-item">
              <span>Output</span>
              <strong>{policy().maxOutputBytes.toLocaleString()} B</strong>
            </div>
          </div>
        )}
      </Show>

      <Show when={errorMessage()}>
        <div class="modules-error" role="alert">
          {errorMessage()}
        </div>
      </Show>
      <For each={catalog()?.errors ?? []}>
        {(error) => (
          <div class="modules-error" role="alert">
            {error}
          </div>
        )}
      </For>

      <label class="modules-input">
        <span>Command input (JSON)</span>
        <textarea
          rows={4}
          value={inputJson()}
          spellcheck={false}
          aria-label="Shared module command input as JSON"
          onInput={(event) => setInputJson(event.currentTarget.value)}
        />
      </label>

      <Show
        when={catalog()}
        fallback={
          <div class="empty-note">
            {operation() === "refresh" ? "Loading modules..." : "Module catalog unavailable."}
          </div>
        }
      >
        <Show
          when={(catalog()?.modules.length ?? 0) > 0}
          fallback={<div class="empty-note">No WebAssembly modules are installed.</div>}
        >
          <div class="modules-list">
            <For each={catalog()?.modules ?? []}>
              {(module) => (
                <article class="modules-card">
                  <div class="modules-card-head">
                    <div class="modules-identity">
                      <div class="modules-title-line">
                        <strong>{module.displayName || module.name}</strong>
                        <span class="modules-version">v{module.version}</span>
                      </div>
                      <div class="modules-id">
                        {module.publisher}/{module.name} · {module.id}
                      </div>
                    </div>
                    <label class="modules-toggle">
                      <input
                        type="checkbox"
                        checked={module.enabled}
                        disabled={isBusy()}
                        aria-label={`Enable ${module.displayName || module.name}`}
                        onChange={async (event) => {
                          const input = event.currentTarget;
                          const changed = await setModuleEnabled(module, input.checked);
                          if (!changed) {
                            input.checked = module.enabled;
                          }
                        }}
                      />
                      <span>{module.enabled ? "Enabled" : "Disabled"}</span>
                    </label>
                  </div>

                  <div class="modules-badges">
                    <span class={`state-badge ${module.enabled ? "ready" : "muted"}`}>
                      {module.enabled ? "enabled" : "disabled"}
                    </span>
                    <Show when={module.needsApproval}>
                      <span class="state-badge warning">approval needed</span>
                    </Show>
                  </div>

                  <div class="modules-description">
                    {module.description || "No description provided."}
                  </div>

                  <div>
                    <div class="subsection-title modules-subsection-title">
                      Requested capabilities
                    </div>
                    <Show
                      when={module.capabilities.length > 0}
                      fallback={<div class="modules-muted">No host capabilities requested.</div>}
                    >
                      <div class="modules-capabilities">
                        <For each={module.capabilities}>
                          {(capability) => (
                            <span class="modules-capability">{capability}</span>
                          )}
                        </For>
                      </div>
                    </Show>
                  </div>

                  <Show when={module.commands.length > 0}>
                    <div>
                      <div class="subsection-title modules-subsection-title">Commands</div>
                      <div class="modules-command-list">
                        <For each={module.commands}>
                          {(command) => (
                            <button
                              type="button"
                              class="pane-button modules-command"
                              disabled={isBusy() || !module.enabled || module.needsApproval}
                              onClick={() => void executeCommand(command)}
                            >
                              <span>{command.title}</span>
                              <Show when={command.category}>
                                <small>{command.category}</small>
                              </Show>
                            </button>
                          )}
                        </For>
                      </div>
                    </div>
                  </Show>

                  <button
                    type="button"
                    class="pane-button modules-uninstall"
                    disabled={isBusy()}
                    onClick={() => void uninstallModule(module)}
                  >
                    {operation() === "uninstall" ? "Uninstalling..." : "Uninstall"}
                  </button>
                </article>
              )}
            </For>
          </div>
        </Show>
      </Show>

      <Show when={execution()}>
        {(result) => (
          <section class="modules-result" aria-label="Latest module command result">
            <div class="modules-result-head">
              <div>
                <div class="modules-result-title">{result().command}</div>
                <div class="modules-id">{result().moduleId}</div>
              </div>
              <div class="modules-result-meta">
                <span>{result().fuelConsumed.toLocaleString()} fuel</span>
                <span>{result().durationMs.toLocaleString()} ms</span>
              </div>
            </div>
            <pre class="terminal-output compact modules-output">
              {result().output}
            </pre>
            <Show when={result().logs.length > 0}>
              <ul class="modules-log-list" aria-label="Module command logs">
                <For each={result().logs}>
                  {(log) => (
                    <li class="modules-log" data-level={log.level}>
                      <span class="modules-log-level">{log.level}</span>
                      <span>{log.message}</span>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </section>
        )}
      </Show>
    </section>
  );
}
