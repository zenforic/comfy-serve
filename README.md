# Comfy-Serve

Comfy-Serve is a high-performance Rust proxy server and React dashboard designed to seamlessly interface with a local or remote [ComfyUI](https://github.com/comfyanonymous/ComfyUI) instance. It provides an intuitive GUI workspace to expose and map specific ComfyUI workflow parameters to simple REST API endpoints, including an OpenAI-compatible `/v1/images/generations` route.

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

### Building and Running

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

## CLI Options

- `--dashboard`: Enables serving the React dashboard on `/`.
- `--host <HOST>`: IP address to listen on (default: `127.0.0.1`).
- `-p, --port <PORT>`: Port to listen on (default: `3000`).
- `--log-level debug`: Enables verbose debugging logs.
- `--no-log-workflow`: Disables logging of the full ComfyUI JSON payload to the console when in debug mode, keeping the console clean while still logging the incoming frontend parameters.

## License
MIT

## AI Disclosure
Because this project can be used in situations where security matters, I am disclosing that the project was AI assisted. While the presence of `AGENTS.md` is usually a telling sign, full transparency is important in this case.
