use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures_util::StreamExt;


use uuid::Uuid;

#[derive(Serialize)]
pub struct ComfyPrompt {
    pub prompt: serde_json::Value,
    pub client_id: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ComfyPromptResponse {
    pub prompt_id: String,
    pub node_errors: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
pub struct WsMessage {
    pub r#type: String,
    pub data: serde_json::Value,
}

/// Represents an output asset (image or audio) retrieved from ComfyUI.
#[derive(Debug, Clone)]
pub struct OutputAsset {
    pub data: Vec<u8>,
    pub content_type: String,
    pub extension: String,
    pub filename: Option<String>,
}

/// Derives the MIME content-type and file extension from a filename.
pub fn derive_content_type(filename: &str) -> (String, String) {
    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "png"  => ("image/png".to_string(), "png".to_string()),
        "jpg" | "jpeg" => ("image/jpeg".to_string(), "jpg".to_string()),
        "webp" => ("image/webp".to_string(), "webp".to_string()),
        "gif"  => ("image/gif".to_string(), "gif".to_string()),
        "bmp"  => ("image/bmp".to_string(), "bmp".to_string()),
        "wav"  => ("audio/wav".to_string(), "wav".to_string()),
        "mp3"  => ("audio/mpeg".to_string(), "mp3".to_string()),
        "flac" => ("audio/flac".to_string(), "flac".to_string()),
        "opus" => ("audio/opus".to_string(), "opus".to_string()),
        _      => ("application/octet-stream".to_string(), "bin".to_string()),
    }
}

pub struct ComfyClient {
    base_url: String,
    http: Client,
    log_workflow: bool,
    cleanup_dir: Option<std::path::PathBuf>,
}

impl ComfyClient {
    pub fn new(base_url: String, log_workflow: bool, cleanup_dir: Option<std::path::PathBuf>) -> Self {
        Self {
            base_url,
            http: Client::new(),
            log_workflow,
            cleanup_dir,
        }
    }

    pub async fn submit_prompt(&self, mut prompt_json: serde_json::Value) -> Result<Vec<OutputAsset>, String> {
        let client_id = Uuid::new_v4().to_string();
        
        // Find all Save nodes and prevent caching
        let mut ws_image_nodes = std::collections::HashSet::new();
        if let Some(obj) = prompt_json.as_object_mut() {
            for (node_id, node) in obj {
                if let Some(class_type) = node.get("class_type").and_then(|c| c.as_str()) {
                    let is_ws_save = class_type == "SaveImageWebsocket";
                    let is_disk_save = class_type.starts_with("SaveImage") || class_type.starts_with("SaveAudio");
                    
                    if is_ws_save || (is_disk_save && self.cleanup_dir.is_some()) {
                        if is_ws_save {
                            ws_image_nodes.insert(node_id.clone());
                        }
                        // Inject a random string to inputs to prevent ComfyUI from caching this node
                        if let Some(inputs) = node.get_mut("inputs").and_then(|i| i.as_object_mut()) {
                            inputs.insert("comfy_serve_salt".to_string(), serde_json::json!(client_id.clone()));
                        }
                    }
                }
            }
        }

        let prompt_req = ComfyPrompt {
            prompt: prompt_json.clone(),
            client_id: client_id.clone(),
        };

        if self.log_workflow {
            tracing::debug!("ComfyUI Request: {}", serde_json::to_string_pretty(&prompt_req).unwrap_or_else(|_| "Invalid JSON".to_string()));
        } else {
            tracing::debug!("ComfyUI Request: [Workflow logging disabled]");
        }

        // Submit prompt
        let res = self.http.post(format!("{}/prompt", self.base_url))
            .json(&prompt_req)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let prompt_res: ComfyPromptResponse = res.json().await.map_err(|e| e.to_string())?;

        tracing::debug!("ComfyUI Response: {}", serde_json::to_string_pretty(&prompt_res).unwrap_or_else(|_| "Invalid JSON".to_string()));
        
        if let Some(errs) = prompt_res.node_errors {
            if !errs.as_object().unwrap().is_empty() {
                return Err(format!("ComfyUI Node Errors: {:?}", errs));
            }
        }

        let prompt_id = prompt_res.prompt_id;

        // Connect WS to wait for completion
        let ws_url = self.base_url.replace("http://", "ws://").replace("https://", "wss://");
        let ws_url = format!("{}/ws?clientId={}", ws_url, client_id);

        let (ws_stream, _) = connect_async(&ws_url).await.map_err(|e| e.to_string())?;
        let (_, mut read) = ws_stream.split();

        let mut output_assets: Vec<OutputAsset> = Vec::new();
        let mut current_node = String::new();

        while let Some(msg) = read.next().await {
            let msg = msg.map_err(|e| e.to_string())?;
            match msg {
                Message::Text(text) => {
                    if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                        tracing::debug!("WS Text: {}", text);
                        if ws_msg.r#type == "executing" {
                            let data = ws_msg.data;
                            let msg_prompt_id = data.get("prompt_id").and_then(|id| id.as_str());
                            
                            if msg_prompt_id.is_none() || msg_prompt_id == Some(&prompt_id) {
                                let is_null_node = match data.get("node") {
                                    Some(serde_json::Value::Null) => true,
                                    // if missing entirely, treat as not null
                                    None => false,
                                    _ => false,
                                };

                                if is_null_node {
                                    tracing::debug!("Execution done (node is null)");
                                    break; // Execution done
                                }
                                if let Some(node_id) = data.get("node").and_then(|n| n.as_str()) {
                                    current_node = node_id.to_string();
                                    tracing::debug!("Current node updated to: {}", current_node);
                                }
                            }
                        }
                    }
                }
                Message::Binary(bin) => {
                    tracing::debug!("WS Binary message received, len: {}, current_node: {}", bin.len(), current_node);
                    if ws_image_nodes.contains(&current_node) {
                        let bin_vec = bin.to_vec();
                        if bin_vec.len() > 8 {
                            // The first 8 bytes are type/meta, rest is image data
                            tracing::debug!("Captured image from WS node {}", current_node);
                            output_assets.push(OutputAsset {
                                data: bin_vec[8..].to_vec(),
                                content_type: "image/png".to_string(),
                                extension: "png".to_string(),
                                filename: None,
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        // Fetch history for standard SaveImage / SaveAudio nodes
        let history_url = format!("{}/history/{}", self.base_url, prompt_id);
        if let Ok(res) = self.http.get(&history_url).send().await {
            if let Ok(history_json) = res.json::<serde_json::Value>().await {
                if let Some(history) = history_json.get(&prompt_id) {
                    if let Some(outputs) = history.get("outputs") {
                        if let Some(outputs_obj) = outputs.as_object() {
                            for (_node_id, node_output) in outputs_obj {
                                // Check for images in history
                                if let Some(images) = node_output.get("images") {
                                    if let Some(images_array) = images.as_array() {
                                        for image_info in images_array {
                                            if let Some(filename) = image_info["filename"].as_str() {
                                                let subfolder = image_info["subfolder"].as_str().unwrap_or("");
                                                let folder_type = image_info["type"].as_str().unwrap_or("output");
                                                
                                                let (content_type, extension) = derive_content_type(filename);
                                                let view_url = format!("{}/view?filename={}&subfolder={}&type={}", 
                                                    self.base_url, filename, subfolder, folder_type);
                                                
                                                if let Ok(res) = self.http.get(&view_url).send().await {
                                                    if let Ok(bytes) = res.bytes().await {
                                                        tracing::debug!("Fetched {} from history", filename);
                                                        output_assets.push(OutputAsset {
                                                            data: bytes.to_vec(),
                                                            content_type,
                                                            extension,
                                                            filename: Some(filename.to_string()),
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                // Check for audio in history (SaveAudio / SaveAudioAdvanced / SaveAudioMP3 / SaveAudioOpus)
                                if let Some(audio) = node_output.get("audio") {
                                    if let Some(audio_array) = audio.as_array() {
                                        for audio_info in audio_array {
                                            if let Some(filename) = audio_info["filename"].as_str() {
                                                let subfolder = audio_info["subfolder"].as_str().unwrap_or("");
                                                let folder_type = audio_info["type"].as_str().unwrap_or("output");
                                                
                                                let (content_type, extension) = derive_content_type(filename);
                                                let view_url = format!("{}/view?filename={}&subfolder={}&type={}", 
                                                    self.base_url, filename, subfolder, folder_type);
                                                
                                                if let Ok(res) = self.http.get(&view_url).send().await {
                                                    if let Ok(bytes) = res.bytes().await {
                                                        tracing::debug!("Fetched {} from history", filename);
                                                        output_assets.push(OutputAsset {
                                                            data: bytes.to_vec(),
                                                            content_type,
                                                            extension,
                                                            filename: Some(filename.to_string()),
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Cleanup disk-saved files if enabled
        if let Some(cleanup_dir) = &self.cleanup_dir {
            let disk_filenames: Vec<String> = output_assets
                .iter()
                .filter_map(|a| a.filename.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            if !disk_filenames.is_empty() {
                let dir = cleanup_dir.clone();
                let filenames = disk_filenames;
                tokio::task::spawn_blocking(move || {
                    Self::walk_and_cleanup(&dir, &filenames);
                });
            }
        }

        if output_assets.is_empty() {
            return Err("No outputs generated".to_string());
        }

        Ok(output_assets)
    }

    fn walk_and_cleanup(dir: &std::path::Path, filenames: &[String]) {
        let set: std::collections::HashSet<&str> = filenames.iter().map(|s| s.as_str()).collect();
        if let Err(e) = Self::walk_dir(dir, &set) {
            tracing::warn!("Failed to walk cleanup directory '{}': {}", dir.display(), e);
        }
    }

    fn walk_dir(dir: &std::path::Path, filenames: &std::collections::HashSet<&str>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_symlink() {
                continue;
            }
            if path.is_dir() {
                if let Err(e) = Self::walk_dir(&path, filenames) {
                    tracing::warn!("Failed to walk subdirectory '{}': {}", path.display(), e);
                }
            } else if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if filenames.contains(name) {
                        match std::fs::remove_file(&path) {
                            Ok(_) => tracing::debug!("Cleaned up disk file: {}", path.display()),
                            Err(e) => tracing::warn!("Failed to remove '{}': {}", path.display(), e),
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn upload_file(&self, file_bytes: Vec<u8>, filename: &str) -> Result<String, String> {
        let url = format!("{}/upload/image", self.base_url);
        
        let (content_type, _) = derive_content_type(filename);
        
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(filename.to_string())
            .mime_str(&content_type).unwrap_or_else(|_| reqwest::multipart::Part::bytes(vec![]));
            
        let form = reqwest::multipart::Form::new()
            .part("image", part)
            .text("overwrite", "true");

        let res = self.http.post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        #[derive(Deserialize)]
        struct UploadResponse {
            name: String,
        }

        let upload_res: UploadResponse = res.json().await.map_err(|e| e.to_string())?;
        Ok(upload_res.name)
    }
}

pub fn get_workflows() -> Result<HashMap<String, serde_json::Value>, String> {
    let mut workflows = HashMap::new();
    let entries = std::fs::read_dir("active-workflows").map_err(|e| e.to_string())?;
    for entry in entries {
        if let Ok(entry) = entry {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                        let name = path.file_stem().unwrap().to_str().unwrap().to_string();
                        workflows.insert(name, json);
                    }
                }
            }
        }
    }
        Ok(workflows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn setup_test_dir() -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("comfy_serve_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn test_cleanup_basic() {
        let root = setup_test_dir();
        let file_path = root.join("test_image.png");
        fs::write(&file_path, "data").unwrap();

        ComfyClient::walk_and_cleanup(&root, &["test_image.png".to_string()]);
        assert!(!file_path.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_cleanup_recursive() {
        let root = setup_test_dir();
        let sub = root.join("subfolder");
        fs::create_dir_all(&sub).unwrap();
        let file_path = sub.join("test_image.png");
        fs::write(&file_path, "data").unwrap();

        ComfyClient::walk_and_cleanup(&root, &["test_image.png".to_string()]);
        assert!(!file_path.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_cleanup_no_match() {
        let root = setup_test_dir();
        let file_path = root.join("keep_me.png");
        fs::write(&file_path, "data").unwrap();

        ComfyClient::walk_and_cleanup(&root, &["delete_me.png".to_string()]);
        assert!(file_path.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_cleanup_multiple_files() {
        let root = setup_test_dir();
        let f1 = root.join("img1.png");
        let f2 = root.join("img2.png");
        fs::write(&f1, "data").unwrap();
        fs::write(&f2, "data").unwrap();

        ComfyClient::walk_and_cleanup(&root, &["img1.png".to_string(), "img2.png".to_string()]);
        assert!(!f1.exists());
        assert!(!f2.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    #[cfg(windows)]
    fn test_cleanup_symlink_safety() {
        let root = setup_test_dir();
        let target_dir = setup_test_dir();
        let target_file = target_dir.join("target.png");
        fs::write(&target_file, "data").unwrap();

        // Create symlink in root pointing to target_dir
        if let Err(e) = std::os::windows::fs::symlink_dir(&target_dir, &root.join("link")) {
            tracing::warn!("Skipping symlink test: {} (likely lacks privileges)", e);
        } else {
            ComfyClient::walk_and_cleanup(&root, &["target.png".to_string()]);
            // The file in target_dir should NOT be deleted because the walker skips symlinks
            assert!(target_file.exists());
        }
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&target_dir);
    }

    #[test]
    fn test_cleanup_duplicate_filenames() {
        let root = setup_test_dir();
        let sub1 = root.join("dir1");
        let sub2 = root.join("dir2");
        fs::create_dir_all(&sub1).unwrap();
        fs::create_dir_all(&sub2).unwrap();
        
        let f1 = sub1.join("same.png");
        let f2 = sub2.join("same.png");
        fs::write(&f1, "data").unwrap();
        fs::write(&f2, "data").unwrap();

        ComfyClient::walk_and_cleanup(&root, &["same.png".to_string()]);
        assert!(!f1.exists());
        assert!(!f2.exists());
        let _ = fs::remove_dir_all(&root);
    }
}


