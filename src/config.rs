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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FieldInputTarget {
    Text,
    ImageBase64,
    ImageUrl,
    ComfyUpload,
    AudioUpload,
}

impl Default for FieldInputTarget {
    fn default() -> Self {
        Self::Text
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowFieldMap {
    pub original_node_id: String,
    pub original_field_name: String,
    pub exposed_as: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub input_target: FieldInputTarget,
    #[serde(default)]
    pub randomize: bool,
    #[serde(default)]
    pub is_value_map: bool,
    #[serde(default)]
    pub map_keys: String,
    #[serde(default)]
    pub map_values: String,
}

/// Describes how optional input images are wired into a workflow: for every image a
/// `LoadImage` node is added and linked to `<field_prefix><N>` of node `node_id`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImageInputsConfig {
    pub node_id: String,
    pub field_prefix: String,
    #[serde(default)]
    pub max_images: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowConfig {
    pub active: bool,
    pub file_name: String,
    pub exposed_fields: Vec<WorkflowFieldMap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_inputs: Option<ImageInputsConfig>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_input_target_serialization() {
        let field_map = WorkflowFieldMap {
            original_node_id: "1".to_string(),
            original_field_name: "image".to_string(),
            exposed_as: "input_image".to_string(),
            required: true,
            input_target: FieldInputTarget::ImageBase64,
            randomize: false,
            is_value_map: false,
            map_keys: "".to_string(),
            map_values: "".to_string(),
        };

        let serialized = toml::to_string(&field_map).unwrap();
        assert!(serialized.contains("input_target = \"image_base64\""));

        let deserialized: WorkflowFieldMap = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.input_target, FieldInputTarget::ImageBase64);
    }

    #[test]
    fn test_field_input_target_default_deserialization() {
        // Test deserializing from old config format that lacks input_target
        let old_toml = r#"
            original_node_id = "1"
            original_field_name = "text"
            exposed_as = "prompt"
        "#;
        let deserialized: WorkflowFieldMap = toml::from_str(old_toml).unwrap();
        assert_eq!(deserialized.input_target, FieldInputTarget::Text);
    }

    #[test]
    fn test_image_inputs_optional() {
        let old: WorkflowConfig = toml::from_str("active = true\nfile_name = \"a\"\nexposed_fields = []").unwrap();
        assert!(old.image_inputs.is_none());

        let new: WorkflowConfig = toml::from_str(
            "active = true\nfile_name = \"a\"\nexposed_fields = []\n[image_inputs]\nnode_id = \"prompt\"\nfield_prefix = \"images.image_\"\nmax_images = 3",
        ).unwrap();
        let cfg = new.image_inputs.unwrap();
        assert_eq!(cfg.node_id, "prompt");
        assert_eq!(cfg.max_images, Some(3));
    }
}
