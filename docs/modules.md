# WebAssembly Modules (experimental)

Hematite Modules are small, local WebAssembly packages. The model borrows the
useful parts of VS Code extensions—manifests, activation events, contributed
commands, and explicit enablement—without claiming VS Code binary or API
compatibility.

## v1 status

| Status | Surface |
| --- | --- |
| Supported | Local package installation, manifest/ABI version 1, `onCommand` activation, command contributions, and the `log` and `wasi` capabilities. |
| Experimental | The Modules panel and the module subsystem as a whole. Future surfaces will use explicit manifest and ABI versions. |
| Not supported | VS Code `.vsix` packages or APIs, marketplaces, signatures, module dependencies, hot reload, WASI Preview 2/components, webviews, native binaries, background workers, or ambient host filesystem, network, process, or environment access. |

## Package and installation

A package directory has these required direct children:

```text
hello/
├── module.json
└── module.wasm
```

`module.json` cannot point elsewhere: `entry` must be exactly `module.wasm`.
Source files may live beside the required files, but the runtime loads only the
manifest and compiled WebAssembly entry.

Install a package with the picker in the **Modules** panel. Hematite copies it
to a host-owned, platform-specific app-local-data location; callers must not
depend on that destination path. The package itself is unchanged, so the same
directory and `module.wasm` can be copied and installed on macOS, Windows, and
Linux.

New installations are disabled. Enabling one grants the capabilities currently
requested by its manifest. Because v1 packages are unsigned, every replacement
is installed disabled and requires a fresh enable/capability approval, even when
its requested capabilities did not change. A replacement that adds capabilities
therefore cannot inherit the earlier grant.

## Manifest v1

```json
{
  "manifestVersion": 1,
  "abiVersion": 1,
  "publisher": "example",
  "name": "hello",
  "displayName": "Hello WASM Module",
  "version": "0.1.0",
  "description": "Minimal Hematite ABI v1 command module.",
  "entry": "module.wasm",
  "activationEvents": ["onCommand:example.hello"],
  "contributes": {
    "commands": [
      { "command": "example.hello", "title": "Hello from WASM" }
    ]
  },
  "capabilities": ["log"]
}
```

`publisher` and `name` must each match lowercase
`[a-z0-9][a-z0-9-]*`; the `hematite` publisher is reserved by the host. Together
they form the module ID `<publisher>.<name>`. Every contributed command ID must
either equal that module ID or start with `<module-id>.`.

Version 1 accepts only command activation events. `activationEvents` must exactly
mirror `contributes.commands`: each contributed command has one corresponding
`onCommand:<command-id>` entry, with no missing or extra events. Command entries
contain `command`, `title`, and an optional `category`. `log` enables the
Hematite logging import. `wasi` enables the sandboxed WASI Preview 1 surface
described below. Other capabilities are rejected.

## ABI v1

All pointers and lengths address the module's exported linear memory. Strings
and payloads are UTF-8.

The guest must export:

```text
memory
hematite_alloc(size: i32) -> i32
hematite_dispatch(input_ptr: i32, input_len: i32) -> i64
```

It may export `hematite_dealloc(ptr: i32, len: i32) -> ()`.

The host serializes each invocation as this JSON envelope, allocates guest
memory, and copies its UTF-8 bytes there:

```json
{ "command": "example.hello", "input": null }
```

`input` may be any JSON value. `hematite_dispatch` returns the output length in
the high 32 bits and the output pointer in the low 32 bits:

```text
result = (u64(output_len) << 32) | u32(output_ptr)
```

The output is bounded UTF-8. JSON is conventional for structured results but
is not required by the ABI. Returned ranges must be inside exported memory, and
the output bytes must remain live until `hematite_dispatch` returns and the host
copies them. The store is then discarded; module memory never persists between
invocations.

For host logging, a guest with the granted `log` capability may import:

```text
hematite.host_call(
  op: i32,
  req_ptr: i32,
  req_len: i32,
  output_ptr: i32,
  output_capacity: i32
) -> i32
```

The request range contains a UTF-8 log message. Operation `1` logs at info,
`2` at warn, and `3` at error level. In ABI v1, `output_ptr` and
`output_capacity` are reserved and both must be zero.

## WASI Preview 1

A module with the granted `wasi` capability may import
`wasi_snapshot_preview1`. Hematite uses Wasmi's official Preview 1 adapter with
a fresh, capability-scoped context for every invocation:

- `argv[0]` is the module ID and `argv[1]` is the invoked command ID.
- stdin contains the same bounded JSON command envelope passed to
  `hematite_dispatch`.
- stdout and stderr use bounded in-memory pipes and are returned as info and
  error logs respectively.
- no host environment variables, directories, files, sockets, or inherited
  stdio handles are exposed.

WASI support is reactor-style: the normal Hematite ABI exports remain required.
If `_initialize: () -> ()` is exported, Hematite calls it before
`hematite_dispatch`; `_start` is not invoked. Filesystem and socket calls have no
preopened resources and therefore cannot escape the module sandbox.

## Runtime limits and lifecycle

| Resource | v1 limit |
| --- | ---: |
| Fuel per invocation | 10,000,000 |
| Linear memory | 32 MiB |
| Input envelope | 64 KiB |
| Output | 256 KiB |
| WASI stdout / stderr | 64 KiB each |
| Concurrent calls per module | 1 |
| Concurrent calls globally | `min(4, hardware parallelism)` |

Logs and the compiled-module cache are bounded. Modules are compiled lazily and
each invocation receives a fresh store and instance. Hematite creates no module
polling loop, idle worker, or background activation task.

## Author workflow

1. Create `module.json` and a guest that implements ABI v1.
2. Compile the guest to the required direct-child `module.wasm`. For the WAT
   example, run one of these commands from a copied package directory:

   ```sh
   wat2wasm hello.wat -o module.wasm
   # or
   wasm-tools parse hello.wat -o module.wasm
   ```

3. Open the Modules panel, install the package with its picker, review the
   requested capability, and enable it.
4. Copy that same package directory to another supported OS to install it
   there; do not rebuild it for the destination platform.

The repository keeps the readable WAT source but does not check in the generated
`module.wasm`.
