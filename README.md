# AI Agent — Windows client + Linux relay server

Thin Windows desktop app (Tauri) that talks to a Linux relay server. The
server hosts the agent providers (Claude Code CLI, Codex CLI, direct
Anthropic / OpenAI APIs); the client only renders the conversation.

```
┌─────────────────────────┐         HTTP + SSE          ┌──────────────────────────┐
│  Windows client (Tauri) │ ────────────────────────►   │  Linux server (Axum)     │
│  • React UI             │                             │  • POST /api/chat (SSE)  │
│  • reqwest SSE consumer │ ◄──── stream of events ──── │  • runs claude / codex   │
└─────────────────────────┘                             │  • or calls LLM APIs     │
                                                        └──────────────────────────┘
```

API keys, CLI binaries, working directories all live on the server. The
Windows machine just needs network access to the server URL.

## Repo layout

```
/
├── Cargo.toml                   # workspace
├── crates/
│   ├── agent/                   # shared: AgentProvider trait + provider impls
│   └── server/                  # aiagent-server (Axum binary)
├── src-tauri/                   # Tauri client crate
├── src/                         # React UI (Vite)
├── package.json
└── .github/workflows/release.yml
```

## Run the server (on this Linux box)

```bash
cargo run -p aiagent-server
# listens on 0.0.0.0:8788 by default
# override with: AIAGENT_SERVER_ADDR=0.0.0.0:9000 cargo run -p aiagent-server
```

Make sure the providers you want are reachable from the server:

| Provider id | Requirement on server |
|---|---|
| `claude-code-cli` | `claude` on PATH (`npm i -g @anthropic-ai/claude-code`) |
| `codex-cli` | `codex` on PATH |
| `anthropic-api` | `ANTHROPIC_API_KEY` env var (provider stubbed — see TODO) |
| `openai-api` | `OPENAI_API_KEY` env var (provider stubbed — see TODO) |

Endpoints:

- `GET  /api/health` — liveness
- `GET  /api/providers` — list registered providers
- `GET  /api/providers/:id/detect` — true if usable
- `POST /api/chat` — body `{ provider_id, prompt, working_dir?, provider_config? }`, returns SSE
  - first event: `event: session` with `{ session_id }`
  - subsequent events: `event: agent` with one `AgentEvent` JSON payload each
- `POST /api/cancel/:session_id` — abort a running session

## Run the Windows client

### Dev mode on this Linux box (for iteration)

You can run the Tauri client on Linux for development — it speaks the same
protocol the Windows build will:

```bash
sudo apt install libwebkit2gtk-4.1-dev libssl-dev librsvg2-dev \
                 libayatana-appindicator3-dev build-essential file
# install Rust if you haven't:  curl https://sh.rustup.rs -sSf | sh
npm install
npm run tauri:dev
```

In the app's top bar, set "Relay server" to where your server is listening
(e.g. `http://127.0.0.1:8788`).

### Production Windows build

This repo ships a GitHub Actions workflow (`.github/workflows/release.yml`)
that builds signed .msi and .exe installers on `windows-latest` runners.

To cut a release:

```bash
git tag v0.1.0
git push origin v0.1.0
```

The workflow:
1. Builds the Tauri client on Windows → uploads `.msi` and NSIS `.exe`
   to a GitHub Release (draft).
2. Builds the Linux server binary in parallel → uploads as artifact.
3. On non-tag pushes, just runs `cargo check --workspace` as CI.

If you don't have a Windows machine and don't want to use GitHub Actions,
the other options (cross-compile with `cargo-xwin`, or use a Windows VM)
are documented in the Tauri docs but tend to be flakier than CI.

## How to add a new provider

1. Create `crates/agent/src/providers/my_provider.rs` implementing `AgentProvider`.
2. Register it in `crates/agent/src/registry.rs::default_providers`.
3. Restart the server. The client picks it up via `/api/providers`.

The frontend never sees provider-specific types — every provider's output
is translated into the internal `AgentEvent` enum (`started / text /
tool_call / tool_result / error / finished`).

## Config file (client side)

Persisted as JSON. Currently stores `server_url`, `active_provider`,
`working_dir`. Locations:

- Windows: `%APPDATA%\aiagent\config.json`
- macOS:   `~/Library/Application Support/aiagent/config.json`
- Linux:   `$XDG_CONFIG_HOME/aiagent/config.json`

Override with `AIAGENT_CONFIG_DIR=/path/to/dir`.

## Status / TODOs

- [x] HTTP + SSE protocol end-to-end
- [x] Claude Code CLI provider parses `stream-json`
- [ ] Codex CLI provider parses structured output (currently raw stdout passthrough)
- [ ] Anthropic API provider (stub)
- [ ] OpenAI API provider (stub)
- [ ] Multi-turn / session resume
- [ ] Authentication / authorization on the server (currently open; add a
      `Bearer` token check before exposing to the internet)
- [ ] TLS termination — put nginx/Caddy in front before going public
