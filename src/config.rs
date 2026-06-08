use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::fs;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub model: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowFieldMap {
    pub original_node_id: String,
    pub original_field_name: String,
    pub exposed_as: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub is_value_map: bool,
    #[serde(default)]
    pub map_keys: String,
    #[serde(default)]
    pub map_values: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowConfig {
    pub active: bool,
    pub file_name: String,
    pub exposed_fields: Vec<WorkflowFieldMap>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub comfyui_url: String,
    pub llm: Option<LlmConfig>,
    pub workflows: HashMap<String, WorkflowConfig>,
    #[serde(default)]
    pub enable_openai_compat: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            comfyui_url: "http://127.0.0.1:8188".to_string(),
            llm: None,
            workflows: HashMap::new(),
            enable_openai_compat: false,
        }
    }
}

pub fn load_config<P: AsRef<Path>>(path: P) -> Config {
    if let Ok(content) = fs::read_to_string(path.as_ref()) {
        toml::from_str(&content).unwrap_or_default()
    } else {
        Config::default()
    }
}

pub fn save_config<P: AsRef<Path>>(config: &Config, path: P) -> Result<(), std::io::Error> {
    let toml_str = toml::to_string(config).unwrap();
    fs::write(path, toml_str)
}
