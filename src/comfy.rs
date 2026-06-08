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

#[derive(Deserialize, Debug)]
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
}

impl ComfyClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            http: Client::new(),
        }
    }

    pub async fn submit_prompt(&self, prompt_json: serde_json::Value) -> Result<Vec<u8>, String> {
        let client_id = Uuid::new_v4().to_string();
        
        let prompt_req = ComfyPrompt {
            prompt: prompt_json,
            client_id: client_id.clone(),
        };

        // Submit prompt
        let res = self.http.post(format!("{}/prompt", self.base_url))
            .json(&prompt_req)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let prompt_res: ComfyPromptResponse = res.json().await.map_err(|e| e.to_string())?;
        
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

        while let Some(msg) = read.next().await {
            let msg = msg.map_err(|e| e.to_string())?;
            if let Message::Text(text) = msg {
                if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                    if ws_msg.r#type == "executed" {
                        let data = ws_msg.data;
                        if data["prompt_id"] == prompt_id {
                            let output = &data["output"];
                            // Assume we get images from the first node that produces them
                            for (_node_id, node_output) in output.as_object().unwrap() {
                                if let Some(images) = node_output.get("images") {
                                    if let Some(images_array) = images.as_array() {
                                        for image_info in images_array {
                                            let filename = image_info["filename"].as_str().unwrap();
                                            let subfolder = image_info["subfolder"].as_str().unwrap_or("");
                                            let folder_type = image_info["type"].as_str().unwrap_or("output");
                                            
                                            // Fetch image
                                            let img_url = format!("{}/view?filename={}&subfolder={}&type={}", 
                                                self.base_url, filename, subfolder, folder_type);
                                            let img_bytes = self.http.get(&img_url).send().await.unwrap().bytes().await.unwrap().to_vec();
                                            output_images.push(img_bytes);
                                        }
                                    }
                                }
                            }
                            break;
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

