# Comfy-Serve

Comfy-Serve is a high-performance Rust proxy server and React dashboard designed to seamlessly interface with a local or remote [ComfyUI](https://github.com/Comfy-Org/ComfyUI) instance. It provides an intuitive GUI workspace to expose and map specific ComfyUI workflow parameters to simple REST API endpoints, including an OpenAI-compatible `/v1/images/generations` route.

## Features

- **React Dashboard**: An elegant dusk-themed dashboard to visually map complex ComfyUI nodes (e.g., KSampler seed, positive prompt, CFG scale) to simple API parameters.
- **LLM Assisted Restructuring**: Automatically analyze and expose workflow variables using an optional OpenAI-compatible LLM endpoint.
- **Dynamic Caching Bypass**: Automatically forces ComfyUI's `SaveImageWebsocket` nodes (and disk-saving nodes when `--cleanup` is enabled) to execute every time, ensuring reliable asset delivery without missing files from disk.
- **Audio Support**: Supports ComfyUI audio workflows via `SaveAudio` nodes. Audio outputs (wav, mp3, flac, opus) are automatically detected and returned with the correct `Content-Type`. Audio inputs are uploaded to ComfyUI for use with `LoadAudio` nodes.
- **Optional Reference Images**: A workflow can declare an `image_inputs` mapping to accept any number of optional reference images without needing fixed `LoadImage` nodes. Each incoming image is uploaded to ComfyUI and automatically injected and wired into the workflow as its own `LoadImage` node.
- **Disconnect-Aware Cancellation**: If the client aborts a request mid-generation (Ctrl+C, timeout, closed tab), the queued or running ComfyUI job is automatically cancelled so GPU time isn't wasted, and temporary uploads and hosted images are still cleaned up.
- **OpenAI Compatible Endpoint**: Drop-in replacement for OpenAI's image generation API. Route your existing AI apps to Comfy-Serve effortlessly.

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (cargo)
- [Node.js](https://nodejs.org/) & npm (for building the frontend dashboard)
- A running ComfyUI instance (default assumes `http://127.0.0.1:8188`)

### Installation

**1. Pre-built Binaries (Recommended)**
You can download pre-built binaries for Windows and Linux from the [GitHub Releases](https://github.com/zenforic/comfy-serve/releases). These binaries come with the compiled React dashboard out of the box.

**2. Install via Crates.io**
You can install Comfy-Serve globally directly from crates.io:
```bash
cargo install comfy-serve
```
The crate ships the frontend *sources*, not the compiled assets, so a default install uses your installed Node.js/npm to build the dashboard automatically during the Cargo build (a `build.rs` script runs `npm ci`/`npm install` and `npm run build`). You can also skip that step entirely and install the headless version (no dashboard) by disabling default features:
```bash
cargo install comfy-serve --no-default-features
```
If npm is not found and the dist folder is missing or outdated, the build stops with a clear error explaining your options (install Node.js, use `--no-default-features`, or pre-build the frontend manually and set `COMFY_SERVE_SKIP_FRONTEND=1`).

### Building and Running from Source

1. **Build the Frontend** (optional — `cargo build` does this for you automatically via `build.rs`):
   ```bash
   cd frontend
   npm install
   npm run build
   cd ..
   ```
   *Note: The frontend assets are embedded directly into the Rust binary using `rust-embed`, so they must exist before compilation finishes. `build.rs` handles this: it rebuilds the frontend only when `frontend/dist` is missing or out of date, and skips everything when built with `--no-default-features`. Set `COMFY_SERVE_SKIP_FRONTEND=1` to force-skip the npm build (useful offline or for cross-compiling; the binary will embed whatever `frontend/dist` already contains).

2. **Run the Server**:
   ```bash
   cargo run --release -- --dashboard
   ```

3. **Access the Dashboard**:
   Open your browser and navigate to `http://127.0.0.1:3000`. You will be prompted to set up an initial password and configure your ComfyUI server details.

### Adding Workflows

To expose a ComfyUI workflow via the API:
1. In ComfyUI, ensure you are using a `SaveImageWebsocket` or standard `SaveImage` node (or `SaveAudio`/`SaveAudioAdvanced`/`SaveAudioMP3`/`SaveAudioOpus` for audio workflows) for your final output.
2. Click **Save (API Format)** in ComfyUI to export the workflow JSON.
3. Place the JSON file into the `active-workflows/` directory in the `comfy-serve` root folder.
4. Refresh the Comfy-Serve dashboard to see and map the new workflow!

## API Endpoints

- `GET /api/models` - Lists active configured workflows and their required parameters, including a `supports_images` flag for workflows that accept reference images via `image_inputs`.
- `POST /api/generate` - The main custom generation endpoint. Supports both image and audio workflows; the response `Content-Type` is set dynamically based on the output type. Workflows configured with `image_inputs` also accept an optional `params.images` array (each entry a URL or Base64 string, optionally a `data:` URI); the images are uploaded to ComfyUI and injected as `LoadImage` nodes. Sending `images` to a workflow without `image_inputs` returns `400`.
### OpenAI Compatible API

You can use standard OpenAI client libraries to interface with `comfy-serve` by pointing the base URL to your `comfy-serve` instance.

- **`GET /v1/models`**: Returns all active workflows as available models.
- **`POST /v1/images/generations`**: Acts as a drop-in replacement for DALL-E generation. Maps the requested `prompt` JSON string to whichever node you exposed as `prompt`.
- **`POST /v1/images/edits`**: Acts as a drop-in replacement for DALL-E editing. Accepts `multipart/form-data` uploads, mapping the `prompt` string to the `prompt` node, and the `image` file binary to the `image` node. When the target workflow is configured with `image_inputs`, all `image` / `image[]` parts are forwarded as reference images instead (respecting `max_images`).

## Environment Configuration

You can configure Comfy-Serve using a `.env` file in the root of the project.
Create a `.env` file and define the following variables if needed:

```env
# The port the API and dashboard will run on (default is 3000)
PORT=3000

# The host IP the server will bind to (default is 127.0.0.1)
HOST=0.0.0.0

# Set a pre-hashed Argon2 password for dashboard authentication, otherwise it can be set at first run of the dashboard.
DASHBOARD_PASSWORD_HASH=your_argon2_hash_here

# Optional: A comma-separated list of API keys required to hit the image generation endpoints.
# If left blank or unset, the generation endpoints will remain open and unprotected.
API_KEYS=sk-mysecretkey,sk-anotherkey

# Optional: A comma-separated list of allowed origins for browser-based CORS requests
# (e.g. calling the API via fetch/XHR from a web app on a different domain).
# If left blank or unset, no CORS headers are sent and cross-origin browser requests will fail.
CORS_ORIGIN=https://example.com,https://another-app.example.com
```

## Manual Configuration (`config.toml`)

If you are running the headless version or prefer to configure the server manually without using the dashboard, you can create or edit the `config.toml` file in the root of the project.

Here is an example `config.toml` layout:

```toml
comfyui_url = "http://127.0.0.1:8188"
enable_openai_compat = true

# Optional: Configuration for LLM Assisted Restructuring (OpenAI-compatible only)
[llm]
base_url = "https://api.openai.com/v1"
model = "gpt-4o"
api_key = "sk-..."

# Define your active workflows
[workflows."my-workflow.json"]
active = true
file_name = "my-workflow.json"

# Map a specific ComfyUI node field to a simple API parameter
[[workflows."my-workflow.json".exposed_fields]]
original_node_id = "3"
original_field_name = "text"
exposed_as = "prompt"
required = true
input_target = "text" # Options: text, image_base64, image_url, comfy_upload, audio_upload
randomize = false # Set true to inject a random seed when hitting the OpenAI endpoint
is_value_map = false
map_keys = ""
map_values = ""

# Example of mapping boolean values to specific node numerical values
[[workflows."my-workflow.json".exposed_fields]]
original_node_id = "5"
original_field_name = "value"
exposed_as = "turbo"
required = false
is_value_map = true
map_keys = "true,false"
map_values = "0,0.9"

# Optional: accept optional reference images (params.images on /api/generate,
# or multiple image parts on /v1/images/edits) without fixed LoadImage nodes.
# Each image is uploaded to ComfyUI and wired as [reference_N, 0] into
# <node_id>.inputs.<field_prefix><N> (e.g. images.image_1, images.image_2, ...).
# Also editable in the dashboard via the "Supports input images" panel.
[workflows."my-workflow.json".image_inputs]
node_id = "7"                    # Node whose inputs receive the image links
field_prefix = "images.image_"   # Field naming pattern for the incoming images
max_images = 4                   # Optional cap; omit to allow any number
```

## CLI Options

- `--dashboard`: Enables serving the React dashboard on `/`.
- `--host <HOST>`: IP address to listen on (default: `127.0.0.1`).
- `-p, --port <PORT>`: Port to listen on (default: `3000`).
- `--log-level debug`: Enables verbose debugging logs.
- `--no-log-workflow`: Disables logging of the full ComfyUI JSON payload to the console when in debug mode, keeping the console clean while still logging the incoming frontend parameters.
- `--cleanup <DIRECTORY>`: When specified, comfy-serve will search the given directory (resolved from the working directory unless absolute) for files saved to disk by ComfyUI `SaveImage` and `SaveAudio` nodes and delete them after retrieval, preventing disk clutter. Only effective when ComfyUI runs on the same machine. Cleanup is disabled if this flag is not provided.
- `--cleanup-input <DIRECTORY>`: When specified, comfy-serve will search the given directory (resolved from the working directory unless absolute) for temporary files uploaded to ComfyUI's input folder and delete them after the request completes — including when the request ends early or the client disconnects mid-generation. This prevents the ComfyUI input directory from piling up with `comfy_serve_temp` images and audio files during edit workflows and `params.images` generation.
- `--log-expand-binary`: Expands binary payloads (like multipart forms) in the debug logs instead of showing `[binary/(type)]`.
- `--cors-origin <ORIGIN>`: Allowed origin for browser CORS requests (e.g. `https://example.com`). Can be repeated or provided as a comma-separated list, and can also be set via the `CORS_ORIGIN` env var. If omitted, no CORS headers are sent, so cross-origin browser `fetch`/`XHR` calls to the API will fail (server-to-server calls, curl, etc. are unaffected since CORS is a browser-enforced mechanism). This is only needed when calling `comfy-serve` directly from a browser-based app hosted on a different origin; if a reverse proxy or edge service (e.g. Cloudflare Access) already handles CORS in front of comfy-serve, you don't need this flag.

## License
MIT

## AI Disclosure
Because this project can be used in situations where strong security matters (i.e. in cases where the server is exposed to the internet), I am explicitly disclosing that the project was AI assisted. While the presence of `AGENTS.md` is already usually a telling sign, full transparency is important in this case. I do have much pre-AI experience, but I am not confident enough in my ability to securely code http servers against attacks done the way they are nowadays.