```
                 |     |     |
                 |     |     |
                  \    |    /
                   \   |   /
                    \  |  /                    .--.
               ======\(•)/======              (    )
                      \|/                       )  )   ~ crescent moon
                       |                        (  (
                       |                         `--'
                       |
                       |          S H I V A   D E C O M P I L E R
                  ~~~~~~~~~~~~
                   trishula

        collect . emulate . reverse   -   safe sample analysis
```

# ShivaDecompiler

A web tool, written in **Rust**, for **safe collection and analysis of samples**,
aimed at security research / reverse engineering. It is an **original**
implementation (not a port or copy of any existing download manager), with an
interface inspired by the workflow of a classic download manager.

The idea: download files **fast** (including from the `.onion` network), store
everything in an isolated vault, then **emulate/detonate** binaries in a remote
Docker host and **reverse** them with a disassembly module - with nothing running
on the operator's machine.

## Features

### Collection (Downloads)
- Download queue with **New download**: several URLs at once.
- **Segmented downloads** (multiple connections per file, via HTTP Range) to
  maximize speed - the aria2/AriaNg-style performance feature.
- **Real-time speed** per task and **global speed** in the header.
- **Automatic Tor**: `.onion` URLs are routed through the SOCKS5 proxy
  (`socks5h://127.0.0.1:9050`); clearnet goes direct.
- **Downloading / Waiting / Stopped** states with counters.
- **Task detail** (side panel): URL, state, connections, progress, bytes, speed,
  `sha256`, on-disk path, error.
- **Remove** (with **real cancellation** of an in-flight transfer),
  **Retry errored**, **Clear completed**.
- Each file is written as **opaque bytes** and its **SHA-256** is computed.
  Nothing is executed locally.

### Vault terminal
- Terminal-style, **jailed** navigation of the sample folder:
  `ls`, `cd <dir>`, `pwd`, `cat <file>`, `clear`, `help`.
- **No OS command execution** - it is a safe file browser with traversal
  protection (`../` cannot escape the root).

### Sandbox (remote detonation)
- Binaries are detonated in a **remote Docker host** (dedicated VPS/lab),
  **never** on the operator's machine.
- Hardened container: `--network none` by default, read-only rootfs,
  `cap_drop ALL`, non-root user, memory/pids/time limits.
- Pluggable executor (`StubExecutor` until the Docker-over-TLS / SSH runner is
  wired in).

### Disassembly Packages
- Submits a binary to several disassembly/decompilation backends side by side
  (asm, pseudo-C, LLM-assisted reversing) - similar idea to dogbolt.
- Pluggable backends via the `DisasmBackend` trait.

### System
- **Engine status**: active/waiting/completed/failed, total speed, total
  downloaded, Tor, sandbox.
- **Settings**: refresh interval and default connections (persisted in the
  browser).
- **PT/EN language**, white/gray theme, Helvetica font, square buttons.

## Architecture

```
crates/
  shiva-core/    headless engine
    download.rs  queue + segmented download + Tor + cancellation + stats
    vault.rs     jailed file browser (terminal)
    sandbox.rs   remote Docker detonation contract
    disasm.rs    Disassembly Packages module
  shiva-server/  Axum server: serves the frontend + JSON API
web/             SPA frontend (HTML/CSS/JS, no build step)
```

## Running

```bash
cargo run -p shiva-server
# opens http://127.0.0.1:8777
```

> On Windows, the `x86_64-pc-windows-gnu` toolchain needs MinGW-w64 on PATH
> (for `dlltool`/`gcc`/`ld`). E.g. WinLibs.

### Environment variables

| Var | Default | Description |
|-----|---------|-------------|
| `SHIVA_BIND` | `127.0.0.1:8777` | Bind address |
| `SHIVA_DOWNLOAD_DIR` | `./downloads` | Sample vault |
| `SHIVA_TOR_PROXY` | `socks5h://127.0.0.1:9050` | Tor proxy (for `.onion`) |
| `SHIVA_DOCKER_HOST` | - | Analysis VPS Docker (TLS recommended) |
| `SHIVA_WEB_DIR` | `./web` | Frontend directory |

## API

| Method | Route | Description |
|--------|-------|-------------|
| `GET`  | `/api/health` | Status |
| `GET`  | `/api/stats` | Aggregates (counts, total speed) |
| `GET`  | `/api/downloads` | List tasks |
| `POST` | `/api/downloads` | `{ "url", "connections?", "out_name?" }` enqueue |
| `DELETE` | `/api/downloads/{id}` | Remove/cancel |
| `GET`  | `/api/fs?path=` | List a vault directory |
| `GET`  | `/api/fs/cat?path=` | Read (head) a vault file |
| `POST` | `/api/sandbox/detonate` | Remote detonation |
| `POST` | `/api/disasm` | Run analysis backends |

## Security

- Collected binaries **never** run on the operator's host.
- `.onion` always via Tor (DNS resolved on the proxy, `socks5h`).
- The vault terminal is jailed, with no command execution.
- The detonation container is isolated and network-less by default.

## Current status

Functional base. The sandbox executor and the disassembly backends are
registrable *stubs*; download **pause/resume** is not resumable yet. Next steps:
wire Docker-over-TLS on the VPS, plug in the reversing engines, and the LLM.

## License

AGPL-3.0-or-later.
