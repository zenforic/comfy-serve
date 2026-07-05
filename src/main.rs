mod config;
mod auth;
mod comfy;
mod llm;

#[cfg(feature = "dashboard")]
use axum::http::Uri;
use axum::{
    routing::{get, post},
    Router,
    response::{IntoResponse, Response},
    http::{StatusCode, header},
    body::Body,
    extract::State,
    Json,
};
use clap::Parser;
#[cfg(feature = "dashboard")]
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

    /// Host to listen on
    #[arg(long, env = "HOST", default_value = "127.0.0.1")]
    host: String,

    /// Log level: info or debug
    #[arg(short, long, value_enum)]
    log_level: Option<LogLevel>,

    /// Disable logging of the full workflow JSON request to ComfyUI in debug mode
    #[arg(long)]
    no_log_workflow: bool,

    /// Expand binary payloads in debug logs instead of showing [binary/(type)]
    #[arg(long)]
    log_expand_binary: bool,
}

#[cfg(feature = "dashboard")]
#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct FrontendAssets;

#[derive(Clone)]
struct AppState {
    config: Arc<RwLock<Config>>,
    comfy_client: Arc<comfy::ComfyClient>,
    dashboard_token: Arc<RwLock<Option<String>>>,
    password_hash: Arc<RwLock<String>>,
    api_keys: Arc<Vec<String>>,
    temp_images: Arc<RwLock<std::collections::HashMap<String, Vec<u8>>>>,
    log_expand_binary: bool,
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
        writeln!(file, "API_KEYS=''").unwrap();
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

async fn process_image_input(
    input_str: &str,
    target: &crate::config::FieldInputTarget,
    comfy_client: &crate::comfy::ComfyClient,
    temp_images: &Arc<RwLock<std::collections::HashMap<String, Vec<u8>>>>,
    host_header: Option<&str>,
) -> Result<(String, Option<String>), String> {
    if target == &crate::config::FieldInputTarget::Text {
        return Ok((input_str.to_string(), None));
    }

    let is_url = input_str.starts_with("http://") || input_str.starts_with("https://");
    
    if is_url && target == &crate::config::FieldInputTarget::ImageUrl {
        return Ok((input_str.to_string(), None));
    }

    let raw_bytes = if is_url {
        reqwest::get(input_str)
            .await.map_err(|e| e.to_string())?
            .bytes()
            .await.map_err(|e| e.to_string())?
            .to_vec()
    } else {
        use base64::Engine;
        let b64_str = if let Some(idx) = input_str.find("base64,") {
            &input_str[idx + 7..]
        } else {
            input_str
        };
        base64::engine::general_purpose::STANDARD.decode(b64_str.trim()).map_err(|e| format!("Base64 Error: {}", e))?
    };

    if target == &crate::config::FieldInputTarget::Text {
        return Ok((input_str.to_string(), None));
    }
    process_raw_image_bytes(raw_bytes, target, comfy_client, temp_images, host_header).await
}

async fn process_raw_image_bytes(
    raw_bytes: Vec<u8>,
    target: &crate::config::FieldInputTarget,
    comfy_client: &crate::comfy::ComfyClient,
    temp_images: &Arc<RwLock<std::collections::HashMap<String, Vec<u8>>>>,
    host_header: Option<&str>,
) -> Result<(String, Option<String>), String> {
    match target {
        crate::config::FieldInputTarget::ImageBase64 => {
            use base64::Engine;
            Ok((base64::engine::general_purpose::STANDARD.encode(&raw_bytes), None))
        }
        crate::config::FieldInputTarget::ImageUrl => {
            let id = uuid::Uuid::new_v4().to_string();
            temp_images.write().await.insert(id.clone(), raw_bytes);
            let host = host_header.unwrap_or("127.0.0.1:3000");
            Ok((format!("http://{}/api/temp-images/{}", host, id), Some(id)))
        }
        crate::config::FieldInputTarget::ComfyUpload => {
            let filename = format!("comfy_serve_temp_{}.png", uuid::Uuid::new_v4());
            let name = comfy_client.upload_image(raw_bytes, &filename).await?;
            Ok((name, None))
        }
        _ => Err("Invalid target for raw image bytes".to_string()),
    }
}

async fn generate_handler(State(state): State<AppState>, headers: axum::http::HeaderMap, Json(payload): Json<GenerateRequest>) -> impl IntoResponse {
    if !state.api_keys.is_empty() {
        let auth_header = headers.get("Authorization").and_then(|h| h.to_str().ok()).unwrap_or("");
        let token = auth_header.replace("Bearer ", "");
        if !state.api_keys.contains(&token) {
            return (StatusCode::UNAUTHORIZED, "Invalid or missing API key").into_response();
        }
    }

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

    let mut temp_cleanup_ids = Vec::new();
    let host_header = headers.get("host").and_then(|h| h.to_str().ok());

    // Required checks and apply mappings
    for field_map in &wf_config.exposed_fields {
        if field_map.required && !payload.params.contains_key(&field_map.exposed_as) {
            return (StatusCode::BAD_REQUEST, format!("Missing required parameter: {}", field_map.exposed_as)).into_response();
        }

        if let Some(val) = payload.params.get(&field_map.exposed_as) {
            let mut final_val = val.clone();
            
            let incoming_str = match val {
                serde_json::Value::String(s) => s.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Number(n) => n.to_string(),
                _ => val.to_string(),
            };

            if field_map.input_target != crate::config::FieldInputTarget::Text {
                match process_image_input(&incoming_str, &field_map.input_target, &state.comfy_client, &state.temp_images, host_header).await {
                    Ok((processed_str, temp_id)) => {
                        final_val = serde_json::Value::String(processed_str);
                        if let Some(id) = temp_id {
                            temp_cleanup_ids.push(id);
                        }
                    }
                    Err(e) => return (StatusCode::BAD_REQUEST, format!("Image processing failed for {}: {}", field_map.exposed_as, e)).into_response(),
                }
            } else if field_map.is_value_map {
                let keys: Vec<&str> = field_map.map_keys.split(',').map(|s| s.trim()).collect();
                let values: Vec<&str> = field_map.map_values.split(',').map(|s| s.trim()).collect();
                
                if let Some(idx) = keys.iter().position(|&k| k == incoming_str) {
                    if let Some(mapped_val_str) = values.get(idx) {
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
    let res = match state.comfy_client.submit_prompt(wf_json).await {
        Ok(image_bytes) => {
            Response::builder()
                .header(header::CONTENT_TYPE, "image/png")
                .body(Body::from(image_bytes))
                .unwrap()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };

    // Cleanup temp images
    if !temp_cleanup_ids.is_empty() {
        let mut temp_images = state.temp_images.write().await;
        for id in temp_cleanup_ids {
            temp_images.remove(&id);
        }
    }

    res
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

async fn openai_generate_handler(State(state): State<AppState>, headers: axum::http::HeaderMap, Json(payload): Json<OpenAiImageRequest>) -> impl IntoResponse {
    if !state.api_keys.is_empty() {
        let auth_header = headers.get("Authorization").and_then(|h| h.to_str().ok()).unwrap_or("");
        let token = auth_header.replace("Bearer ", "");
        if !state.api_keys.contains(&token) {
            return (StatusCode::UNAUTHORIZED, "Invalid or missing API key").into_response();
        }
    }

    tracing::debug!("API OpenAI Request: {}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    let config = state.config.read().await;
    
    if !config.enable_openai_compat {
        return (StatusCode::FORBIDDEN, "OpenAI compat is disabled").into_response();
    }

    // Use requested model or fallback to first active workflow
    let mut target_workflow = if let Some(m) = payload.model {
        m
    } else {
        match config.workflows.iter().find(|(_, c)| c.active) {
            Some((k, _)) => k.clone(),
            None => return (StatusCode::BAD_REQUEST, "No active workflows configured").into_response(),
        }
    };

    let wf_config = match config.workflows.get(&target_workflow) {
        Some(c) if c.active => c,
        _ => {
            // Attempt to recover if the client slugified the model name (e.g. replacing _ with -)
            let alt_target = target_workflow.replace("-", "_");
            if let Some(c) = config.workflows.get(&alt_target) {
                if c.active {
                    target_workflow = alt_target;
                    c
                } else {
                    return (StatusCode::BAD_REQUEST, "Workflow not active or not found").into_response();
                }
            } else {
                return (StatusCode::BAD_REQUEST, "Workflow not active or not found").into_response();
            }
        }
    };

    let mut workflows = match comfy::get_workflows() {
        Ok(w) => w,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load workflows").into_response(),
    };

    let mut wf_json = match workflows.remove(&target_workflow) {
        Some(json) => json,
        None => return (StatusCode::BAD_REQUEST, "Workflow JSON not found").into_response(),
    };

    // Apply the text prompt to the field mapped as "prompt" and any requested random seeds
    for field_map in &wf_config.exposed_fields {
        let mut final_val = None;

        if field_map.exposed_as == "prompt" {
            if field_map.input_target != crate::config::FieldInputTarget::Text {
                return (StatusCode::BAD_REQUEST, "The prompt field is mapped to an image input, which is not supported by the /v1/images/generations endpoint.").into_response();
            }
            final_val = Some(serde_json::Value::String(payload.prompt.clone()));
        } else if field_map.randomize {
            let random_seed: u64 = rand::random();
            final_val = Some(serde_json::Value::Number(random_seed.into()));
        }

        if let Some(val) = final_val {
            if let Some(node) = wf_json.get_mut(&field_map.original_node_id) {
                if let Some(inputs) = node.get_mut("inputs") {
                    inputs.as_object_mut().unwrap().insert(field_map.original_field_name.clone(), val);
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

async fn openai_edits_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    bytes: axum::body::Bytes,
) -> impl IntoResponse {
    if !state.api_keys.is_empty() {
        let auth_header = headers.get("Authorization").and_then(|h| h.to_str().ok()).unwrap_or("");
        let token = auth_header.replace("Bearer ", "");
        if !state.api_keys.contains(&token) {
            return (StatusCode::UNAUTHORIZED, "Invalid or missing API key").into_response();
        }
    }

    let config = state.config.read().await;
    if !config.enable_openai_compat {
        return (StatusCode::FORBIDDEN, "OpenAI compat is disabled").into_response();
    }

    let content_type = headers.get(axum::http::header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
        
    let boundary = if let Some(idx) = content_type.find("boundary=") {
        content_type[idx + 9..].trim().to_string()
    } else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": {"message": "Missing boundary in content-type", "type": "invalid_request_error"}}))
        ).into_response();
    };

    let mut prompt: Option<String> = None;
    let mut image_bytes: Option<Vec<u8>> = None;
    let mut model: Option<String> = None;
    
    let parts = parse_lenient_multipart(&bytes, &boundary);
    for (name, _filename, data) in parts {
        if name == "prompt" {
            prompt = String::from_utf8(data).ok();
        } else if name == "image" || name == "image[]" {
            image_bytes = Some(data);
        } else if name == "model" {
            model = String::from_utf8(data).ok();
        }
    }
    
    let prompt = match prompt {
        Some(p) => p,
        None => return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": {"message": "Missing prompt field", "type": "invalid_request_error"}}))
        ).into_response(),
    };
    
    let image_bytes = match image_bytes {
        Some(b) => b,
        None => return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": {"message": "Missing image field", "type": "invalid_request_error"}}))
        ).into_response(),
    };

    let mut target_workflow = if let Some(m) = model {
        m
    } else {
        match config.workflows.iter().find(|(_, c)| c.active) {
            Some((k, _)) => k.clone(),
            None => return (StatusCode::BAD_REQUEST, "No active workflows configured").into_response(),
        }
    };

    let wf_config = match config.workflows.get(&target_workflow) {
        Some(c) if c.active => c,
        _ => {
            let alt_target = target_workflow.replace("-", "_");
            if let Some(c) = config.workflows.get(&alt_target) {
                if c.active {
                    target_workflow = alt_target;
                    c
                } else {
                    return (StatusCode::BAD_REQUEST, "Workflow is inactive").into_response();
                }
            } else {
                return (StatusCode::BAD_REQUEST, format!("Workflow '{}' not found", target_workflow)).into_response();
            }
        }
    };

    let mut workflows = match comfy::get_workflows() {
        Ok(w) => w,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load workflows").into_response(),
    };

    let mut wf_json = match workflows.remove(&target_workflow) {
        Some(json) => json,
        None => return (StatusCode::BAD_REQUEST, "Workflow JSON not found").into_response(),
    };
    
    let host_header = headers.get("host").and_then(|h| h.to_str().ok());
    let mut temp_cleanup_ids = Vec::new();

    for field_map in &wf_config.exposed_fields {
        let mut final_val = None;

        if field_map.exposed_as == "prompt" {
            final_val = Some(serde_json::Value::String(prompt.clone()));
        } else if field_map.exposed_as == "image" {
            match process_raw_image_bytes(image_bytes.clone(), &field_map.input_target, &state.comfy_client, &state.temp_images, host_header).await {
                Ok((res_val, cleanup_id)) => {
                    final_val = Some(serde_json::Value::String(res_val));
                    if let Some(id) = cleanup_id {
                        temp_cleanup_ids.push(id);
                    }
                }
                Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to process image: {}", e)).into_response(),
            }
        } else if field_map.randomize {
            let random_seed: u64 = rand::random();
            final_val = Some(serde_json::Value::Number(random_seed.into()));
        }

        if let Some(val) = final_val {
            if let Some(node) = wf_json.get_mut(&field_map.original_node_id) {
                if let Some(inputs) = node.get_mut("inputs") {
                    inputs.as_object_mut().unwrap().insert(field_map.original_field_name.clone(), val);
                }
            }
        }
    }

    let submit_result = state.comfy_client.submit_prompt(wf_json).await;

    // Cleanup temp hosted images
    for id in temp_cleanup_ids {
        state.temp_images.write().await.remove(&id);
    }

    match submit_result {
        Ok(returned_image_bytes) => {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&returned_image_bytes);
            
            let res = OpenAiImageResponse {
                created: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                data: vec![OpenAiImageData {
                    b64_json: Some(b64),
                    url: None, 
                }],
            };

            (StatusCode::OK, Json(res)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(serde::Serialize)]
struct OpenAiModelListResponse {
    object: String,
    data: Vec<OpenAiModel>,
}

#[derive(serde::Serialize)]
struct OpenAiModel {
    id: String,
    object: String,
    created: u64,
    owned_by: String,
}

async fn openai_models_handler(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    if !state.api_keys.is_empty() {
        let auth_header = headers.get("Authorization").and_then(|h| h.to_str().ok()).unwrap_or("");
        let token = auth_header.replace("Bearer ", "");
        if !state.api_keys.contains(&token) {
            return (StatusCode::UNAUTHORIZED, "Invalid or missing API key").into_response();
        }
    }

    let config = state.config.read().await;
    
    if !config.enable_openai_compat {
        return (StatusCode::FORBIDDEN, "OpenAI compat is disabled").into_response();
    }

    let workflows = match comfy::get_workflows() {
        Ok(w) => w,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load workflows").into_response(),
    };

    let mut models = Vec::new();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

    for (name, c) in &config.workflows {
        if c.active && workflows.contains_key(name) {
            models.push(OpenAiModel {
                id: name.clone(),
                object: "model".to_string(),
                created: now,
                owned_by: "comfy-serve".to_string(),
            });
        }
    }

    let res = OpenAiModelListResponse {
        object: "list".to_string(),
        data: models,
    };

    (StatusCode::OK, Json(res)).into_response()
}

async fn openapi_spec_handler(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    if !state.api_keys.is_empty() {
        let auth_header = headers.get("Authorization").and_then(|h| h.to_str().ok()).unwrap_or("");
        let token = auth_header.replace("Bearer ", "");
        if !state.api_keys.contains(&token) {
            return (StatusCode::UNAUTHORIZED, "Invalid or missing API key").into_response();
        }
    }

    let config = state.config.read().await;
    
    if !config.enable_openai_compat {
        return (StatusCode::FORBIDDEN, "OpenAI compat is disabled").into_response();
    }

    let spec = serde_json::json!({
        "openapi": "3.0.0",
        "info": {
            "title": "comfy-serve OpenAI Compatible API",
            "version": "1.0.0",
            "description": "OpenAI compatible endpoints mapping to ComfyUI Workflows"
        },
        "servers": [
            {
                "url": "/v1"
            }
        ],
        "paths": {
            "/models": {
                "get": {
                    "summary": "List available models (workflows)",
                    "responses": {
                        "200": {
                            "description": "Successful response"
                        }
                    }
                }
            },
            "/images/generations": {
                "post": {
                    "summary": "Create image from prompt",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "prompt": { "type": "string" },
                                        "model": { "type": "string" }
                                    },
                                    "required": ["prompt"]
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "Successful image generation" }
                    }
                }
            },
            "/images/edits": {
                "post": {
                    "summary": "Edit or create image from multipart form data",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "multipart/form-data": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "prompt": { "type": "string" },
                                        "image": { "type": "string", "format": "binary" },
                                        "model": { "type": "string" }
                                    },
                                    "required": ["prompt", "image"]
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "Successful image edit" }
                    }
                }
            }
        }
    });

    (StatusCode::OK, Json(spec)).into_response()
}

#[derive(serde::Serialize)]
struct NativeModelInfo {
    id: String,
    fields: Vec<crate::config::WorkflowFieldMap>,
}

async fn native_models_handler(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    if !state.api_keys.is_empty() {
        let auth_header = headers.get("Authorization").and_then(|h| h.to_str().ok()).unwrap_or("");
        let token = auth_header.replace("Bearer ", "");
        if !state.api_keys.contains(&token) {
            return (StatusCode::UNAUTHORIZED, "Invalid or missing API key").into_response();
        }
    }

    let config = state.config.read().await;
    
    let workflows = match comfy::get_workflows() {
        Ok(w) => w,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load workflows").into_response(),
    };

    let mut models = Vec::new();

    for (name, c) in &config.workflows {
        if c.active && workflows.contains_key(name) {
            models.push(NativeModelInfo {
                id: name.clone(),
                fields: c.exposed_fields.clone(),
            });
        }
    }

    (StatusCode::OK, Json(models)).into_response()
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

async fn get_temp_image_handler(State(state): State<AppState>, axum::extract::Path(id): axum::extract::Path<String>) -> impl IntoResponse {
    let images = state.temp_images.read().await;
    if let Some(bytes) = images.get(&id) {
        Response::builder()
            .header(header::CONTENT_TYPE, "image/png")
            .body(Body::from(bytes.clone()))
            .unwrap()
    } else {
        (StatusCode::NOT_FOUND, "Image not found or expired").into_response()
    }
}

#[cfg(feature = "dashboard")]
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

fn parse_lenient_multipart(body: &[u8], boundary: &str) -> Vec<(String, Option<String>, Vec<u8>)> {
    let mut parts = Vec::new();
    let boundary_bytes = format!("--{}", boundary).into_bytes();
    
    let mut positions = Vec::new();
    let mut i = 0;
    while i < body.len() {
        if body[i..].starts_with(&boundary_bytes) {
            positions.push(i);
            i += boundary_bytes.len();
        } else {
            i += 1;
        }
    }

    for window in positions.windows(2) {
        let start = window[0] + boundary_bytes.len();
        let end = window[1];
        
        let mut part_data = &body[start..end];
        
        while part_data.starts_with(b"\r") || part_data.starts_with(b"\n") {
            part_data = &part_data[1..];
        }
        while part_data.ends_with(b"\r") || part_data.ends_with(b"\n") {
            part_data = &part_data[..part_data.len()-1];
        }

        let headers_end;
        let data_start;
        
        if let Some(pos) = part_data.windows(4).position(|w| w == b"\r\n\r\n") {
            headers_end = pos;
            data_start = pos + 4;
        } else if let Some(pos) = part_data.windows(2).position(|w| w == b"\n\n") {
            headers_end = pos;
            data_start = pos + 2;
        } else {
            continue;
        }

        let headers_str = String::from_utf8_lossy(&part_data[..headers_end]);
        let data = &part_data[data_start..];

        let mut name = String::new();
        let mut filename = None;

        for line in headers_str.lines() {
            let line = line.trim();
            if line.to_lowercase().starts_with("content-disposition:") {
                if let Some(name_idx) = line.find("name=\"") {
                    let rest = &line[name_idx + 6..];
                    if let Some(end_idx) = rest.find("\"") {
                        name = rest[..end_idx].to_string();
                    }
                }
                if let Some(fname_idx) = line.find("filename=\"") {
                    let rest = &line[fname_idx + 10..];
                    if let Some(end_idx) = rest.find("\"") {
                        filename = Some(rest[..end_idx].to_string());
                    }
                }
            }
        }

        if !name.is_empty() {
            parts.push((name, filename, data.to_vec()));
        }
    }
    
    parts
}

async fn request_logger(
    State(app_state): State<AppState>,
    state: axum::extract::Request,
    next: axum::middleware::Next,
) -> impl IntoResponse {
    let method = state.method().clone();
    let uri = state.uri().clone();
    
    // Get peer address from ConnectInfo
    let peer_addr = state
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|axum::extract::ConnectInfo(addr)| addr.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let content_type = state.headers().get(axum::http::header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();

    let is_debug = tracing::enabled!(tracing::Level::DEBUG);
    let is_multipart = content_type.starts_with("multipart/");

    let (state, body_str) = if is_multipart || is_debug {
        let (mut parts, body) = state.into_parts();
        let bytes = match axum::body::to_bytes(body, usize::MAX).await {
            Ok(b) => b,
            Err(_) => axum::body::Bytes::new(),
        };

        if is_multipart {
            if is_debug {
                tracing::debug!("First 20 bytes of body: {:?}", &bytes[..std::cmp::min(20, bytes.len())]);
            }
            let mut detected_boundary = None;
            // Find the first occurrence of "--" in the first 100 bytes
            if let Some(start_idx) = bytes.windows(2).take(100).position(|w| w == b"--") {
                let boundary_start = start_idx + 2;
                if let Some(end_offset) = bytes[boundary_start..].iter().position(|&b| b == b'\r' || b == b'\n') {
                    let boundary_end = boundary_start + end_offset;
                    if boundary_end > boundary_start {
                        if let Ok(b_str) = std::str::from_utf8(&bytes[boundary_start..boundary_end]) {
                            let b_trimmed = b_str.trim();
                            if !b_trimmed.is_empty() && b_trimmed.len() < 100 {
                                detected_boundary = Some(b_trimmed.to_string());
                            }
                        }
                    }
                }
            }

            if let Some(boundary) = detected_boundary {
                let new_content_type = format!("multipart/form-data; boundary={}", boundary);
                if let Ok(hv) = axum::http::HeaderValue::from_str(&new_content_type) {
                    parts.headers.insert(axum::http::header::CONTENT_TYPE, hv);
                }
            }
        }

        let body_str = if is_debug {
            if is_multipart && !app_state.log_expand_binary {
                let current_ct = parts.headers.get(axum::http::header::CONTENT_TYPE)
                    .and_then(|h| h.to_str().ok())
                    .unwrap_or(&content_type);
                
                let boundary = if let Some(idx) = current_ct.find("boundary=") {
                    current_ct[idx + 9..].trim().to_string()
                } else {
                    "".to_string()
                };

                let mut summary = String::new();
                summary.push_str(&format!("Multipart Payload (Content-Type: {}, Size: {} bytes):\n", current_ct, bytes.len()));
                
                let parsed_parts = parse_lenient_multipart(&bytes, &boundary);
                if parsed_parts.is_empty() {
                    summary.push_str("  [Warning: No fields found or failed to parse. Lenient parser returned 0 parts.]\n");
                    let len = bytes.len();
                    let start = String::from_utf8_lossy(&bytes[..std::cmp::min(500, len)]);
                    let end = if len > 500 {
                        String::from_utf8_lossy(&bytes[len - 500..])
                    } else {
                        std::borrow::Cow::Borrowed("")
                    };
                    summary.push_str(&format!("  [Raw Body Start]:\n{}\n", start));
                    summary.push_str(&format!("  [Raw Body End]:\n{}\n", end));
                } else {
                    for (name, filename, data) in parsed_parts {
                        let has_file = filename.is_some();
                        let mut is_text = false;
                        let mut text_val = String::new();
                        
                        if !has_file {
                            if let Ok(text) = String::from_utf8(data.clone()) {
                                is_text = true;
                                text_val = text;
                            }
                        }
                        
                        if is_text {
                            summary.push_str(&format!("  - {}: {}\n", name, text_val));
                        } else {
                            summary.push_str(&format!("  - {}: [binary data]\n", name));
                        }
                    }
                }
                Some(summary)
            } else {
                let is_binary = content_type.starts_with("image/") || content_type.starts_with("application/octet-stream");

                if is_binary && !app_state.log_expand_binary {
                    Some(format!("[binary/{}]", if content_type.is_empty() { "unknown" } else { &content_type }))
                } else {
                    let body_str = String::from_utf8_lossy(&bytes).to_string();
                    Some(body_str)
                }
            }
        } else {
            None
        };

        let state = axum::extract::Request::from_parts(parts, axum::body::Body::from(bytes));
        (state, body_str)
    } else {
        (state, None)
    };

    let response = next.run(state).await;
    let status = response.status();

    tracing::info!(
        "{} - \"{} {} HTTP/1.1 {}\"",
        peer_addr,
        method,
        uri,
        status
    );

    if let Some(body_str) = body_str {
        if status != axum::http::StatusCode::NOT_FOUND {
            tracing::debug!("Request payload: {}", body_str);
        }
    }

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
    if let Ok(content) = std::fs::read_to_string("config.toml") {
        if content.contains("workflows") && !content.contains("input_target") {
            println!("Old config format detected (missing input_target mapping settings).");
            println!("Would you like to backup your old config and migrate it to the new format? (y/N)");
            let mut input = String::new();
            if std::io::stdin().read_line(&mut input).is_ok() && input.trim().eq_ignore_ascii_case("y") {
                let backup_path = "config.toml.bak";
                if let Ok(_) = std::fs::copy("config.toml", backup_path) {
                    if let Ok(_) = config::save_config(&config, "config.toml") {
                        println!("Migrated config and saved backup to {}", backup_path);
                    }
                }
            }
        }
    }

    let comfy_client = Arc::new(comfy::ComfyClient::new(config.comfyui_url.clone(), !args.no_log_workflow));
    
    let hash = std::env::var("DASHBOARD_PASSWORD_HASH").unwrap_or_default().trim().to_string();
    
    let api_keys_env = std::env::var("API_KEYS").unwrap_or_default();
    let api_keys: Vec<String> = api_keys_env
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    
    let state = AppState {
        config: Arc::new(RwLock::new(config)),
        comfy_client,
        dashboard_token: Arc::new(RwLock::new(None)),
        password_hash: Arc::new(RwLock::new(hash)),
        api_keys: Arc::new(api_keys),
        temp_images: Arc::new(RwLock::new(std::collections::HashMap::new())),
        log_expand_binary: args.log_expand_binary,
    };

    info!("Starting comfy-serve API server...");

    let mut app = Router::new()
        .route("/api/health", get(health_check))
        .route("/api/workflows", get(list_workflows_handler))
        .route("/api/models", get(native_models_handler))
        .route("/api/config", get(get_config_handler).post(update_config_handler))
        .route("/api/generate", post(generate_handler))
        .route("/v1/images/generations", post(openai_generate_handler))
        .route("/v1/images/edits", post(openai_edits_handler))
        .route("/v1/models", get(openai_models_handler))
        .route("/v1/openapi.json", get(openapi_spec_handler))
        .route("/api/login", post(login_handler))
        .route("/api/auth_check", get(check_auth_handler))
        .route("/api/restructure", post(restructure_handler))
        .route("/api/temp-images/{id}", get(get_temp_image_handler))
        .with_state(state.clone());

        // Add more API routes here

    if args.dashboard {
        #[cfg(feature = "dashboard")]
        {
            info!("Dashboard enabled. Serving on /");
            // Serve static assets via fallback so it doesn't conflict with API
            app = app.fallback(static_handler);
        }
        #[cfg(not(feature = "dashboard"))]
        {
            tracing::warn!("Dashboard was requested via --dashboard, but the binary was not compiled with the 'dashboard' feature.");
            app = app.fallback(|| async { (StatusCode::NOT_FOUND, "Dashboard feature disabled at compile time") });
        }
    } else {
        app = app.fallback(|| async { (StatusCode::NOT_FOUND, "Not Found") });
    }

    // Apply logger middleware last so it catches fallback (404) requests
    app = app
        .layer(axum::extract::DefaultBodyLimit::disable())
        .layer(axum::middleware::from_fn_with_state(state.clone(), request_logger));

    let host_addr: std::net::IpAddr = args.host.parse().expect("Invalid IP address for --host");
    let addr = SocketAddr::from((host_addr, args.port));
    info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}
