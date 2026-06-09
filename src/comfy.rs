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

pub struct ComfyClient {
    base_url: String,
    http: Client,
    log_workflow: bool,
}

impl ComfyClient {
    pub fn new(base_url: String, log_workflow: bool) -> Self {
        Self {
            base_url,
            http: Client::new(),
            log_workflow,
        }
    }

    pub async fn submit_prompt(&self, mut prompt_json: serde_json::Value) -> Result<Vec<u8>, String> {
        let client_id = Uuid::new_v4().to_string();
        
        // Find all SaveImageWebsocket nodes and prevent caching
        let mut ws_image_nodes = std::collections::HashSet::new();
        if let Some(obj) = prompt_json.as_object_mut() {
            for (node_id, node) in obj {
                if let Some(class_type) = node.get("class_type").and_then(|c| c.as_str()) {
                    if class_type == "SaveImageWebsocket" {
                        ws_image_nodes.insert(node_id.clone());
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

        let mut output_images = Vec::new();
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
                            output_images.push(bin_vec[8..].to_vec());
                        }
                    }
                }
                _ => {}
            }
        }

        // Fetch history for standard SaveImage nodes
        let history_url = format!("{}/history/{}", self.base_url, prompt_id);
        if let Ok(res) = self.http.get(&history_url).send().await {
            if let Ok(history_json) = res.json::<serde_json::Value>().await {
                if let Some(history) = history_json.get(&prompt_id) {
                    if let Some(outputs) = history.get("outputs") {
                        if let Some(outputs_obj) = outputs.as_object() {
                            for (_node_id, node_output) in outputs_obj {
                                if let Some(images) = node_output.get("images") {
                                    if let Some(images_array) = images.as_array() {
                                        for image_info in images_array {
                                            if let Some(filename) = image_info["filename"].as_str() {
                                                let subfolder = image_info["subfolder"].as_str().unwrap_or("");
                                                let folder_type = image_info["type"].as_str().unwrap_or("output");
                                                
                                                let img_url = format!("{}/view?filename={}&subfolder={}&type={}", 
                                                    self.base_url, filename, subfolder, folder_type);
                                                
                                                if let Ok(res) = self.http.get(&img_url).send().await {
                                                    if let Ok(bytes) = res.bytes().await {
                                                        output_images.push(bytes.to_vec());
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

        if output_images.is_empty() {
            return Err("No images generated".to_string());
        }

        // Return the first image for synchronous API simplicity
        Ok(output_images.remove(0))
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

