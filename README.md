# Comfy-Serve

Comfy-Serve is a high-performance Rust proxy server and React dashboard designed to seamlessly interface with a local or remote [ComfyUI](https://github.com/Comfy-Org/ComfyUI) instance. It provides an intuitive GUI workspace to expose and map specific ComfyUI workflow parameters to simple REST API endpoints, including an OpenAI-compatible `/v1/images/generations` route.

## Features

- **React Dashboard**: An elegant dusk-themed dashboard to visually map complex ComfyUI nodes (e.g., KSampler seed, positive prompt, CFG scale) to simple API parameters.
- **LLM Assisted Restructuring**: Automatically analyze and expose workflow variables using an optional OpenAI-compatible LLM endpoint.
- **Dynamic Caching Bypass**: Automatically forces ComfyUI's `SaveImageWebsocket` nodes to execute every time without cluttering disk history, ensuring smooth image delivery.
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
If you want to install Comfy-Serve globally via Cargo, you can install it directly from crates.io. 
*Note: Because the crate does not ship with the compiled React frontend assets, you must install the headless version (without the dashboard) by disabling default features.*
```bash
cargo install comfy-serve --no-default-features
```
To install with the dashboard from source, see the instructions below.

### Building and Running from Source

1. **Build the Frontend**:
   ```bash
   cd frontend
   npm install
   npm run build
   cd ..
   ```
   *Note: Building the frontend is required before compiling the Rust server, as the frontend assets are embedded directly into the binary using `rust-embed`.*

2. **Run the Server**:
   ```bash
   cargo run --release -- --dashboard
   ```

3. **Access the Dashboard**:
   Open your browser and navigate to `http://127.0.0.1:3000`. You will be prompted to set up an initial password and configure your ComfyUI server details.

### Adding Workflows

To expose a ComfyUI workflow via the API:
1. In ComfyUI, ensure you are using a `SaveImageWebsocket` or standard `SaveImage` node for your final output.
2. Click **Save (API Format)** in ComfyUI to export the workflow JSON.
3. Place the JSON file into the `active-workflows/` directory in the `comfy-serve` root folder.
4. Refresh the Comfy-Serve dashboard to see and map the new workflow!

## API Endpoints

- `GET /api/models` - Lists active configured workflows and their required parameters.
- `POST /api/generate` - The main custom image generation endpoint.
- `GET /v1/models` - OpenAI compatible endpoint returning a list of active workflows as available models.
- `POST /v1/images/generations` - OpenAI compatible endpoint mimicking DALL-E image generation requests.

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
input_target = "text" # Options: text, image_base64, image_url, comfy_upload
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
```

## CLI Options

- `--dashboard`: Enables serving the React dashboard on `/`.
- `--host <HOST>`: IP address to listen on (default: `127.0.0.1`).
- `-p, --port <PORT>`: Port to listen on (default: `3000`).
- `--log-level debug`: Enables verbose debugging logs.
- `--no-log-workflow`: Disables logging of the full ComfyUI JSON payload to the console when in debug mode, keeping the console clean while still logging the incoming frontend parameters.

## License
MIT

## AI Disclosure
Because this project can be used in situations where strong security matters (i.e. in cases where the server is exposed to the internet), I am explicitly disclosing that the project was AI assisted. While the presence of `AGENTS.md` is already usually a telling sign, full transparency is important in this case. I do have much pre-AI experience, but I am not confident enough in my ability to securely code http servers against attacks done the way they are nowadays.