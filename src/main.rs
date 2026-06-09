mod config;
mod auth;
mod comfy;
mod llm;

use axum::{
    routing::{get, post},
    Router,
    response::{IntoResponse, Response},
    http::{StatusCode, Uri, header},
    body::Body,
    extract::State,
    Json,
};
use clap::Parser;
use rust_embed::RustEmbed;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::config::Config;

#[derive(clap::ValueEnum, Clone, Debug)]
enum LogLevel {
    Info,
    Debug,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Start the dashboard web UI
    #[arg(short, long)]
    dashboard: bool,

    /// Port to listen on
    #[arg(short, long, env = "PORT", default_value_t = 3000)]
    port: u16,

    /// Log level: info or debug
    #[arg(short, long, value_enum)]
    log_level: Option<LogLevel>,

    /// Disable logging of the full workflow JSON request to ComfyUI in debug mode
    #[arg(long)]
    no_log_workflow: bool,
}

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct FrontendAssets;

#[derive(Clone)]
struct AppState {
    config: Arc<RwLock<Config>>,
    comfy_client: Arc<comfy::ComfyClient>,
    dashboard_token: Arc<RwLock<Option<String>>>,
    password_hash: Arc<RwLock<String>>,
}

async fn list_workflows_handler(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    let token = headers.get("Authorization").and_then(|h| h.to_str().ok()).unwrap_or("").replace("Bearer ", "");
    let expected = state.dashboard_token.read().await.clone().unwrap_or_default();
    if expected.is_empty() || token != expected { return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(); }

    match comfy::get_workflows() {
        Ok(workflows) => {
            (StatusCode::OK, Json(workflows)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

async fn get_config_handler(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    let token = headers.get("Authorization").and_then(|h| h.to_str().ok()).unwrap_or("").replace("Bearer ", "");
    let expected = state.dashboard_token.read().await.clone().unwrap_or_default();
    if expected.is_empty() || token != expected { return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(); }

    let config = state.config.read().await;
    (StatusCode::OK, Json(config.clone())).into_response()
}

async fn update_config_handler(State(state): State<AppState>, headers: axum::http::HeaderMap, Json(new_config): Json<Config>) -> impl IntoResponse {
    let token = headers.get("Authorization").and_then(|h| h.to_str().ok()).unwrap_or("").replace("Bearer ", "");
    let expected = state.dashboard_token.read().await.clone().unwrap_or_default();
    if expected.is_empty() || token != expected { return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(); }

    let mut config = state.config.write().await;
    *config = new_config.clone();
    
    if let Err(e) = config::save_config(&config, "config.toml") {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to save config: {}", e)).into_response();
    }
    
    (StatusCode::OK, "Config updated").into_response()
}

#[derive(serde::Deserialize)]
struct LoginRequest {
    password: String,
}

async fn login_handler(State(state): State<AppState>, Json(payload): Json<LoginRequest>) -> impl IntoResponse {
    let mut hash = state.password_hash.write().await;
    
    if hash.is_empty() {
        // First run setup
        let new_hash = crate::auth::hash_password(&payload.password);
        // Save to .env (simple approach: append or rewrite)
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(".env").unwrap();
        writeln!(file, "DASHBOARD_PASSWORD_HASH='{}'", new_hash).unwrap();
        *hash = new_hash.clone();
        
        // Also generate a token for session
        let token = uuid::Uuid::new_v4().to_string();
        *state.dashboard_token.write().await = Some(token.clone());
        return (StatusCode::OK, Json(serde_json::json!({"token": token, "is_new": true}))).into_response();
    }

    if crate::auth::verify_password(&payload.password, &hash) {
        let token = uuid::Uuid::new_v4().to_string();
        *state.dashboard_token.write().await = Some(token.clone());
        (StatusCode::OK, Json(serde_json::json!({"token": token}))).into_response()
    } else {
        (StatusCode::UNAUTHORIZED, "Invalid password").into_response()
    }
}

async fn check_auth_handler(State(state): State<AppState>, req: axum::extract::Request) -> impl IntoResponse {
    let auth_header = req.headers().get("Authorization").and_then(|h| h.to_str().ok());
    if let Some(auth) = auth_header {
        let token = auth.replace("Bearer ", "");
        let expected = state.dashboard_token.read().await.clone().unwrap_or_default();
        if !expected.is_empty() && token == expected {
            return (StatusCode::OK, "Authenticated").into_response();
        }
    }
    (StatusCode::UNAUTHORIZED, "Unauthorized").into_response()
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GenerateRequest {
    workflow: String,
    params: std::collections::HashMap<String, serde_json::Value>,
}

async fn generate_handler(State(state): State<AppState>, Json(payload): Json<GenerateRequest>) -> impl IntoResponse {
    tracing::debug!("API Generate Request: {}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    let config = state.config.read().await;
    
    // Check if workflow is active in config
    let wf_config = match config.workflows.get(&payload.workflow) {
        Some(c) if c.active => c,
        _ => return (StatusCode::BAD_REQUEST, "Workflow not active or not found").into_response(),
    };

    // Load workflow JSON
    let mut workflows = match comfy::get_workflows() {
        Ok(w) => w,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load workflows").into_response(),
    };

    let mut wf_json = match workflows.remove(&payload.workflow) {
        Some(json) => json,
        None => return (StatusCode::BAD_REQUEST, "Workflow JSON not found").into_response(),
    };

    // Required checks and apply mappings
    for field_map in &wf_config.exposed_fields {
        if field_map.required && !payload.params.contains_key(&field_map.exposed_as) {
            return (StatusCode::BAD_REQUEST, format!("Missing required parameter: {}", field_map.exposed_as)).into_response();
        }

        if let Some(val) = payload.params.get(&field_map.exposed_as) {
            let mut final_val = val.clone();
            
            if field_map.is_value_map {
                let incoming_str = match val {
                    serde_json::Value::String(s) => s.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Number(n) => n.to_string(),
                    _ => val.to_string(),
                };
                
                let keys: Vec<&str> = field_map.map_keys.split(',').map(|s| s.trim()).collect();
                let values: Vec<&str> = field_map.map_values.split(',').map(|s| s.trim()).collect();
                
                if let Some(idx) = keys.iter().position(|&k| k == incoming_str) {
                    if let Some(mapped_val_str) = values.get(idx) {
                        // Attempt to parse mapped value as JSON (e.g. number/bool), fallback to string
                        final_val = serde_json::from_str(mapped_val_str)
                            .unwrap_or_else(|_| serde_json::Value::String(mapped_val_str.to_string()));
                    }
                }
            }

            // Apply to json
            if let Some(node) = wf_json.get_mut(&field_map.original_node_id) {
                if let Some(inputs) = node.get_mut("inputs") {
                    inputs.as_object_mut().unwrap().insert(field_map.original_field_name.clone(), final_val);
                }
            }
        }
    }

    // Submit to ComfyUI
    match state.comfy_client.submit_prompt(wf_json).await {
        Ok(image_bytes) => {
            Response::builder()
                .header(header::CONTENT_TYPE, "image/png")
                .body(Body::from(image_bytes))
                .unwrap()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
struct OpenAiImageRequest {
    prompt: String,
    model: Option<String>,
    // Ignore n, size, etc for now to keep it simple
}

#[derive(serde::Serialize)]
struct OpenAiImageResponse {
    created: u64,
    data: Vec<OpenAiImageData>,
}

#[derive(serde::Serialize)]
struct OpenAiImageData {
    b64_json: Option<String>,
    url: Option<String>,
}

async fn openai_generate_handler(State(state): State<AppState>, Json(payload): Json<OpenAiImageRequest>) -> impl IntoResponse {
    tracing::debug!("API OpenAI Request: {}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    let config = state.config.read().await;
    
    if !config.enable_openai_compat {
        return (StatusCode::FORBIDDEN, "OpenAI compat is disabled").into_response();
    }

    // Use requested model or fallback to first active workflow
    let target_workflow = if let Some(m) = payload.model {
        m
    } else {
        match config.workflows.iter().find(|(_, c)| c.active) {
            Some((k, _)) => k.clone(),
            None => return (StatusCode::BAD_REQUEST, "No active workflows configured").into_response(),
        }
    };

    let wf_config = match config.workflows.get(&target_workflow) {
        Some(c) if c.active => c,
        _ => return (StatusCode::BAD_REQUEST, "Workflow not active or not found").into_response(),
    };

    let mut workflows = match comfy::get_workflows() {
        Ok(w) => w,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load workflows").into_response(),
    };

    let mut wf_json = match workflows.remove(&target_workflow) {
        Some(json) => json,
        None => return (StatusCode::BAD_REQUEST, "Workflow JSON not found").into_response(),
    };

    // Apply the text prompt to the field mapped as "prompt"
    for field_map in &wf_config.exposed_fields {
        if field_map.exposed_as == "prompt" {
            if let Some(node) = wf_json.get_mut(&field_map.original_node_id) {
                if let Some(inputs) = node.get_mut("inputs") {
                    inputs.as_object_mut().unwrap().insert(field_map.original_field_name.clone(), serde_json::Value::String(payload.prompt.clone()));
                }
            }
        }
    }

    match state.comfy_client.submit_prompt(wf_json).await {
        Ok(image_bytes) => {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&image_bytes);
            
            let res = OpenAiImageResponse {
                created: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                data: vec![OpenAiImageData {
                    b64_json: Some(b64),
                    url: None, // We don't host the image natively yet, so return base64
                }],
            };

            (StatusCode::OK, Json(res)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct RestructureRequest {
    workflow: String,
    prompt: String,
    model: Option<String>,
}

async fn restructure_handler(State(state): State<AppState>, headers: axum::http::HeaderMap, Json(payload): Json<RestructureRequest>) -> impl IntoResponse {
    let token = headers.get("Authorization").and_then(|h| h.to_str().ok()).unwrap_or("").replace("Bearer ", "");
    let expected = state.dashboard_token.read().await.clone().unwrap_or_default();
    if expected.is_empty() || token != expected { return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(); }

    let config = state.config.read().await.clone();
    
    let mut llm_config = match config.llm {
        Some(l) => l,
        None => return (StatusCode::BAD_REQUEST, "LLM not configured").into_response(),
    };
    
    if let Some(m) = payload.model {
        if !m.trim().is_empty() {
            llm_config.model = Some(m);
        } else {
            llm_config.model = None;
        }
    }

    let mut workflows = match comfy::get_workflows() {
        Ok(w) => w,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load workflows").into_response(),
    };

    let wf_json = match workflows.remove(&payload.workflow) {
        Some(json) => json,
        None => return (StatusCode::BAD_REQUEST, "Workflow JSON not found").into_response(),
    };

    match crate::llm::restructure_workflow(&llm_config, &wf_json, &payload.prompt).await {
        Ok(mappings) => (StatusCode::OK, Json(mappings)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

async fn static_handler(uri: Uri) -> impl IntoResponse {
    let mut path = uri.path().trim_start_matches('/').to_string();

    if path.is_empty() {
        path = "index.html".to_string();
    }

    match FrontendAssets::get(path.as_str()) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data))
                .unwrap()
        }
        None => {
            // Fallback to index.html for SPA routing
            match FrontendAssets::get("index.html") {
                Some(content) => {
                    let mime = mime_guess::from_path("index.html").first_or_octet_stream();
                    Response::builder()
                        .header(header::CONTENT_TYPE, mime.as_ref())
                        .body(Body::from(content.data))
                        .unwrap()
                }
                None => {
                    Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Body::from("404 Not Found. Did you build the frontend?"))
                        .unwrap()
                }
            }
        }
    }
}

async fn request_logger(
    state: axum::extract::Request,
    next: axum::middleware::Next,
) -> impl IntoResponse {
    let method = state.method().clone();
    let uri = state.uri().clone();
    
    // Get peer address from ConnectInfo
    let peer_addr = state.extensions().get::<std::net::SocketAddr>().map(|a| a.to_string()).unwrap_or_else(|| "unknown".to_string());

    let response = next.run(state).await;
    let status = response.status();

    tracing::info!(
        "{} - \"{} {} HTTP/1.1 {}\"",
        peer_addr,
        method,
        uri,
        status
    );

    response
}

async fn health_check() -> &'static str {
    "OK"
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
    info!("Ctrl-C received, shutting down gracefully...");
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv(); // Load .env first so clap can pick up PORT
    let args = Args::parse();

    let level = match args.log_level {
        Some(LogLevel::Info) => tracing_subscriber::filter::LevelFilter::INFO,
        Some(LogLevel::Debug) => tracing_subscriber::filter::LevelFilter::DEBUG,
        None => tracing_subscriber::filter::LevelFilter::INFO,
    };
    tracing_subscriber::fmt().with_max_level(level).init();

    let config = config::load_config("config.toml");
    let comfy_client = Arc::new(comfy::ComfyClient::new(config.comfyui_url.clone(), !args.no_log_workflow));
    
    let hash = std::env::var("DASHBOARD_PASSWORD_HASH").unwrap_or_default().trim().to_string();
    
    let state = AppState {
        config: Arc::new(RwLock::new(config)),
        comfy_client,
        dashboard_token: Arc::new(RwLock::new(None)),
        password_hash: Arc::new(RwLock::new(hash)),
    };

    info!("Starting comfy-serve API server...");

    let mut app = Router::new()
        .route("/api/health", get(health_check))
        .route("/api/workflows", get(list_workflows_handler))
        .route("/api/config", get(get_config_handler).post(update_config_handler))
        .route("/api/generate", post(generate_handler))
        .route("/v1/images/generations", post(openai_generate_handler))
        .route("/api/login", post(login_handler))
        .route("/api/auth_check", get(check_auth_handler))
        .route("/api/restructure", post(restructure_handler))
        .layer(axum::middleware::from_fn(request_logger))
        .with_state(state);

        // Add more API routes here

    if args.dashboard {
        info!("Dashboard enabled. Serving on /");
        // Serve static assets via fallback so it doesn't conflict with API
        app = app.fallback(static_handler);
    } else {
        app = app.fallback(|| async { (StatusCode::NOT_FOUND, "Not Found") });
    }

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}
