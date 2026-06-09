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

## CLI Options

- `--dashboard`: Enables serving the React dashboard on `/`.
- `--log-level debug`: Enables verbose debugging logs.
- `--no-log-workflow`: Disables logging of the full ComfyUI JSON payload to the console when in debug mode, keeping the console clean while still logging the incoming frontend parameters.

## License
MIT
