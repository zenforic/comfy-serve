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
1. Navigate to `frontend/`.
2. Run `npm run dev` to test your changes independently (you will need to mock API responses if the backend is not running or proxy the Vite server).
3. **CRITICAL**: When you are done editing the frontend, you **MUST** run `npm run build` inside `frontend/`. If you don't build the frontend, `rust-embed` will not pick up the changes during the Cargo build process, and the binary will contain outdated assets.

## API Endpoints

- `GET /api/workflows` - Returns a list of available workflow JSON files in `active-workflows/`.
- `GET /api/config` - Returns the current `config.toml` layout.
- `POST /api/config` - Overwrites `config.toml`.
- `POST /api/generate` - The main custom generation endpoint.
- `POST /v1/images/generations` - An OpenAI compatible endpoint mimicking DALL-E image generation requests. Maps the "prompt" to a specific field configured in the dashboard.
- `POST /api/login` - Authenticates a dashboard session.

## State Management

The backend uses `Arc<RwLock<T>>` for sharing state (configuration, dashboard token, password hashes) across asynchronous Axum handlers safely.

## Parameter Mapping & Validation

Workflow configurations in `config.toml` allow mapping fields using the `WorkflowFieldMap` struct:
- **`required`**: If true, `POST /api/generate` will reject requests (`400 Bad Request`) missing this parameter.
- **`is_value_map` / `map_keys` / `map_values`**: Maps incoming API strings/booleans/numbers to specific ComfyUI values. The API evaluates `map_keys` (comma-separated), matches the incoming string's index, and casts the corresponding string in `map_values` to the native JSON type required by the workflow (e.g. mapping `"true,false"` to `"0,0.9"`).

## LLM Assisted Restructure

The dashboard allows automated field extraction using an OpenAI-compatible endpoint.
- Handled by `src/llm.rs` and the `POST /api/restructure` endpoint.
- Accepts a workflow JSON and user prompt, and wraps it in a strict system prompt demanding a JSON array matching the `WorkflowFieldMap` layout.
- The user can optionally override the target Model string from the popup. If left blank in the onboarding config, the `model` parameter is intentionally omitted from the HTTP request to maximize compatibility with local servers like vLLM.

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
