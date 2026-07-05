# Comfy-Serve Agentic Documentation

This file serves as documentation for AI agents continuing work on the `comfy-serve` project.

## Architecture

`comfy-serve` is a dual-purpose application:
1. **Rust API Server (Backend)**: Built with `axum`. Acts as a proxy to a local or remote ComfyUI instance.
2. **React Dashboard (Frontend)**: A single-page application (SPA) built with Vite and React. The frontend is bundled into the Rust executable at compile-time using `rust-embed`.

## Directory Structure

- `src/` - Rust backend source code.
  - `main.rs` - Axum router, state management, and API endpoints.
  - `config.rs` - Configuration serialization (`config.toml`).
  - `comfy.rs` - Logic to submit prompts to ComfyUI via HTTP/WebSocket and retrieve results.
  - `auth.rs` - Argon2 password hashing logic for the dashboard.
- `frontend/` - React frontend source code.
  - `src/App.tsx` - Main routing and logic for Login, Onboarding, and the Dashboard Workspace.
  - `src/index.css` - Global dusk theme styles.
- `active-workflows/` - Directory where exported ComfyUI workflow JSONs must be placed to be recognized by the server.

## Working on the Frontend

If you need to make changes to the dashboard UI:
- **Theme Constraints**: The dashboard utilizes pre-defined CSS classes for interactive elements to maintain its custom styling. Avoid inline styles for buttons. Use `.accent-btn` for primary actions, `.secondary-btn` for standard actions, `.danger-btn` for destructive actions, and `.flat-btn` for text-only/tab buttons.
1. Navigate to `frontend/`.
2. Run `npm run dev` to test your changes independently (you will need to mock API responses if the backend is not running or proxy the Vite server).
3. **CRITICAL**: When you are done editing the frontend, you **MUST** run `npm run build` inside `frontend/`. If you don't build the frontend, `rust-embed` will not pick up the changes during the Cargo build process, and the binary will contain outdated assets.

## API Endpoints

- `GET /api/workflows` - Returns a list of available workflow JSON files in `active-workflows/`. (Dashboard only)
- `GET /api/models` - Returns a list of active configured workflows and their exposed parameters. (Secured by `Authorization: Bearer <key>` if `API_KEYS` is configured in `.env`)
- `GET /v1/models` - OpenAI compatible endpoint returning a list of active workflows as available models. (Secured by `Authorization: Bearer <key>` if `API_KEYS` is configured in `.env`)
- `GET /api/config` - Returns the current `config.toml` layout.
- `POST /api/config` - Overwrites `config.toml`.
- `POST /api/generate` - The main custom generation endpoint. (Secured by `Authorization: Bearer <key>` if `API_KEYS` is configured in `.env`)
- `POST /v1/images/generations` and `POST /v1/images/edits` - OpenAI compatible endpoints mimicking DALL-E image generation and edit requests. `generations` maps the "prompt" JSON string to a specific field exposed as `prompt`. `edits` accepts `multipart/form-data`, mapping the `prompt` string to the `prompt` field and the `image` binary to the `image` field. (Secured by `Authorization: Bearer <key>` if `API_KEYS` is configured in `.env`)
- `POST /api/login` - Authenticates a dashboard session.

## State Management

The backend uses `Arc<RwLock<T>>` for sharing state (configuration, dashboard token, password hashes) across asynchronous Axum handlers safely.

## Parameter Mapping & Validation

Workflow configurations in `config.toml` allow mapping fields using the `WorkflowFieldMap` struct:
- **`required`**: If true, `POST /api/generate` will reject requests (`400 Bad Request`) missing this parameter.
- **`randomize`**: If true, and if the OpenAI compatible `v1/images/generations` endpoint is hit, a random `u64` will automatically be generated and injected into this field to provide output variety.
- **`input_target`**: Instructs the server on how to intercept and format incoming parameters. 
  - `text`: Passed as standard JSON values. 
  - `image_base64`: Incoming URLs are downloaded and converted to Base64 automatically.
  - `image_url`: Incoming Base64 strings are decoded and temporarily hosted at `/api/temp-images/<uuid>` for nodes that expect URLs.
  - `comfy_upload`: Image bytes (from URL or Base64) are uploaded natively to ComfyUI's `/upload/image` API with a temp name and `overwrite=true`, making them directly usable by standard ComfyUI `LoadImage` nodes.
- **`is_value_map` / `map_keys` / `map_values`**: Maps incoming API strings/booleans/numbers to specific ComfyUI values. The API evaluates `map_keys` (comma-separated), matches the incoming string's index, and casts the corresponding string in `map_values` to the native JSON type required by the workflow (e.g. mapping `"true,false"` to `"0,0.9"`).

## LLM Assisted Restructure

The dashboard allows automated field extraction using an OpenAI-compatible endpoint.
- Handled by `src/llm.rs` and the `POST /api/restructure` endpoint.
- Accepts a workflow JSON and user prompt, and wraps it in a strict system prompt demanding a JSON array matching the `WorkflowFieldMap` layout.
- The user can optionally override the target Model string from the popup. If left blank in the onboarding config, the `model` parameter is intentionally omitted from the HTTP request to maximize compatibility with local servers like vLLM.

## Cargo Features

The frontend dashboard is optionally compiled into the binary via the `dashboard` Cargo feature (which is enabled by default in `Cargo.toml`).
- **Pristine Builds / Crates.io**: If you need to build or publish the crate in a pristine state without the pre-compiled frontend assets (e.g. for `cargo publish` or `cargo install`), you must disable the default features: `cargo build --no-default-features`.
- **Pre-built Binaries**: When building for GitHub releases, standard `cargo build --release` includes the dashboard assets since `dashboard` is a default feature.

## CLI Arguments

- `--log-level debug`: Enables verbose debugging logs.
- `--no-log-workflow`: Disables logging of the full ComfyUI JSON payload to the console when in debug mode, keeping the console clean while still logging the incoming frontend parameters.
- `--host <HOST>`: Specifies the IP address to bind the API server to. Defaults to `127.0.0.1` but can be changed to `0.0.0.0` for external network access.
- `-p, --port <PORT>`: Specifies the port to bind the API server to. Defaults to `3000`.

## Handling ComfyUI WebSocket Caching

`SaveImageWebsocket` nodes in ComfyUI do not write to the disk/history, meaning if ComfyUI caches the node execution, the binary image is never transmitted over the WebSocket. To fix this, `comfy-serve` dynamically intercepts all incoming workflows, identifies any `SaveImageWebsocket` nodes, and injects a randomized `comfy_serve_salt` hidden input. This forces the node to bypass ComfyUI's cache entirely and always execute, guaranteeing that images are delivered to the proxy.

## Future Work

- **Workflow Queuing**: Currently, `comfy-serve` opens a WebSocket connection and waits synchronously for the image to generate. If many requests come in, this could hold many open connections. Consider switching to an async task polling model if traffic scales.
- **Output Storage**: Currently returns raw image bytes natively or Base64 in the OpenAI compat wrapper. It may be beneficial to save generated outputs locally and return persistent URLs instead.
