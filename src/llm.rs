use reqwest::Client;
use serde::{Deserialize, Serialize};
use crate::config::{LlmConfig, WorkflowFieldMap};

#[derive(Serialize)]
struct OpenAIChatRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    messages: Vec<OpenAIChatMessage>,
}

#[derive(Serialize)]
struct OpenAIChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAIChatResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

#[derive(Deserialize)]
struct OpenAIMessage {
    content: String,
}

#[derive(Deserialize)]
struct LlmMappingsResponse {
    mappings: Vec<WorkflowFieldMap>,
}

pub async fn restructure_workflow(llm_config: &LlmConfig, workflow_json: &serde_json::Value, user_prompt: &str) -> Result<Vec<WorkflowFieldMap>, String> {
    let client = Client::new();
    let model = llm_config.model.clone();
    
    let system_prompt = r#"You are an automated ComfyUI workflow parser.
The user wants to map specific properties from the provided ComfyUI workflow JSON to a simplified API structure.
Find the exact Node IDs and internal field names in the JSON that match the user's request.

You MUST output ONLY a valid JSON object containing a "mappings" array. Do not include markdown formatting or backticks.
Schema for the output:
{
  "mappings": [
    {
      "original_node_id": "string",
      "original_field_name": "string",
      "exposed_as": "string",
      "required": boolean (default to false unless user explicitly requires it),
      "input_target": "string" (one of: "text", "image_base64", "image_url", "comfy_upload". Default to "text" unless the field handles images),
      "is_value_map": boolean (true if user wants to map incoming values like true/false to specific numbers/strings),
      "map_keys": "string" (comma separated incoming values, e.g. "true,false", or "" if not used),
      "map_values": "string" (comma separated mapped ComfyUI values, e.g. "0,0.9", or "" if not used)
    }
  ]
}
"#.to_string();

    let user_message = format!("User Prompt: {}\n\nWorkflow JSON:\n{}", user_prompt, serde_json::to_string(workflow_json).unwrap());

    let req_body = OpenAIChatRequest {
        model,
        messages: vec![
            OpenAIChatMessage { role: "system".to_string(), content: system_prompt },
            OpenAIChatMessage { role: "user".to_string(), content: user_message },
        ],
    };

    let mut req = client.post(format!("{}/chat/completions", llm_config.base_url.trim_end_matches('/')));
    if let Some(key) = &llm_config.api_key {
        if !key.is_empty() {
            req = req.bearer_auth(key);
        }
    }

    let res = req.json(&req_body).send().await.map_err(|e| format!("LLM request failed: {}", e))?;
    
    if !res.status().is_success() {
        let err_text = res.text().await.unwrap_or_default();
        return Err(format!("LLM returned error: {}", err_text));
    }

    let chat_res: OpenAIChatResponse = res.json().await.map_err(|e| format!("Failed to parse LLM response: {}", e))?;
    
    let mut content = chat_res.choices.first().map(|c| c.message.content.clone()).unwrap_or_default();
    
    // Clean markdown if the LLM ignores instructions
    content = content.trim().to_string();
    if content.starts_with("```json") {
        content = content["```json".len()..].to_string();
    } else if content.starts_with("```") {
        content = content["```".len()..].to_string();
    }
    if content.ends_with("```") {
        content = content[..content.len() - "```".len()].to_string();
    }
    content = content.trim().to_string();
    
    let parsed: LlmMappingsResponse = serde_json::from_str(&content).map_err(|e| format!("Failed to parse LLM mappings JSON: {}. Content: {}", e, content))?;
    
    Ok(parsed.mappings)
}
