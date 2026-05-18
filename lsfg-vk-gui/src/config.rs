use serde::{Deserialize, Serialize};

/// The top-level configuration file structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub global: GlobalConf,
    #[serde(default, rename = "profile")]
    pub profiles: Vec<GameConf>,
}

/// Global configuration shared across all profiles
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalConf {
    /// Optional override path to Lossless.dll
    pub dll: Option<String>,
    /// Whether to allow FP16 acceleration
    #[serde(default = "default_true")]
    pub allow_fp16: bool,
}

/// Per-game profile configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConf {
    /// Human-readable profile name
    #[serde(default = "default_name")]
    pub name: String,
    /// List of executables/process names that activate this profile.
    /// Can be a string or array of strings in TOML.
    #[serde(default = "default_active_in")]
    pub active_in: toml::Value,
    /// Optional GPU selection
    #[serde(default)]
    pub gpu: Option<String>,
    /// Frame generation multiplier (must be > 1)
    #[serde(default = "default_multiplier")]
    pub multiplier: u32,
    /// Flow scale (0.25 - 1.0)
    #[serde(default = "default_flow_scale")]
    pub flow_scale: f32,
    /// Use performance mode (lighter model)
    #[serde(default)]
    pub performance_mode: bool,
    /// Pacing method (currently only "none")
    #[serde(default = "default_pacing")]
    pub pacing: String,
    /// Optional target output FPS cap (custom addition)
    #[serde(default)]
    pub target_fps: Option<u32>,
}

fn default_true() -> bool {
    true
}
fn default_name() -> String {
    "Profile".into()
}
fn default_active_in() -> toml::Value {
    toml::Value::Array(vec![])
}
fn default_multiplier() -> u32 {
    2
}
fn default_flow_scale() -> f32 {
    1.0
}
fn default_pacing() -> String {
    "none".into()
}

impl Default for GameConf {
    fn default() -> Self {
        Self {
            name: default_name(),
            active_in: default_active_in(),
            gpu: None,
            multiplier: default_multiplier(),
            flow_scale: default_flow_scale(),
            performance_mode: false,
            pacing: default_pacing(),
            target_fps: None,
        }
    }
}

impl GameConf {
    /// Get active_in as a list of strings
    pub fn active_in_list(&self) -> Vec<String> {
        match &self.active_in {
            toml::Value::String(s) => vec![s.clone()],
            toml::Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => vec![],
        }
    }

    /// Set active_in from a list of strings.
    pub fn set_active_in(&mut self, list: Vec<String>) {
        if list.len() == 1 {
            self.active_in = toml::Value::String(list.into_iter().next().unwrap());
        } else {
            self.active_in =
                toml::Value::Array(list.into_iter().map(toml::Value::String).collect());
        }
    }

    /// Validate this profile's settings
    pub fn validate(&self) -> Result<(), String> {
        if self.multiplier < 2 {
            return Err(format!(
                "Profile '{}': multiplier must be >= 2 (got {})",
                self.name, self.multiplier
            ));
        }
        if self.multiplier > 8 {
            return Err(format!(
                "Profile '{}': multiplier must be <= 8 (got {})",
                self.name, self.multiplier
            ));
        }
        if !(0.25..=1.0).contains(&self.flow_scale) {
            return Err(format!(
                "Profile '{}': flow_scale must be 0.25-1.0 (got {:.2})",
                self.name, self.flow_scale
            ));
        }
        if !matches!(self.pacing.as_str(), "none") {
            return Err(format!(
                "Profile '{}': unknown pacing '{}'",
                self.name, self.pacing
            ));
        }
        Ok(())
    }
}

/// Find the configuration file path following XDG conventions
pub fn find_config_path() -> std::path::PathBuf {
    // Honor LSFGVK_CONFIG first
    if let Ok(p) = std::env::var("LSFGVK_CONFIG") {
        if !p.is_empty() {
            return p.into();
        }
    }
    // XDG_CONFIG_HOME
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return std::path::PathBuf::from(xdg)
                .join("lsfg-vk")
                .join("conf.toml");
        }
    }
    // Default: ~/.config/lsfg-vk/conf.toml
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("lsfg-vk")
        .join("conf.toml")
}

/// Load config from disk, creating a default if not found
pub fn load_config() -> Result<Config, String> {
    let path = find_config_path();
    if !path.exists() {
        let config = default_config();
        save_config(&config)?;
        return Ok(config);
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read config at {}: {}", path.display(), e))?;
    let config: Config = toml::from_str(&content)
        .map_err(|e| format!("Failed to parse config at {}: {}", path.display(), e))?;
    if config.version != 2 {
        return Err(format!(
            "Unsupported config version: {} (expected 2)",
            config.version
        ));
    }
    Ok(config)
}

/// Save config to disk
pub fn save_config(config: &Config) -> Result<(), String> {
    let path = find_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config dir {}: {}", parent.display(), e))?;
    }
    let content =
        toml::to_string_pretty(config).map_err(|e| format!("Failed to serialize config: {}", e))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write config to {}: {}", path.display(), e))?;
    Ok(())
}

/// Generate a default configuration
pub fn default_config() -> Config {
    Config {
        version: 2,
        global: GlobalConf {
            dll: None,
            allow_fp16: true,
        },
        profiles: vec![GameConf {
            name: "4x FG / 85% [Performance]".into(),
            active_in: toml::Value::Array(vec![
                toml::Value::String("vkcube".into()),
                toml::Value::String("vkcubepp".into()),
            ]),
            gpu: None,
            multiplier: 4,
            flow_scale: 0.85,
            performance_mode: true,
            pacing: "none".into(),
            target_fps: None,
        }],
    }
}

/// Available GPUs from vulkan
pub fn detect_gpus() -> Vec<String> {
    let mut gpus = vec!["Default".to_string()];
    if let Ok(output) = std::process::Command::new("vulkaninfo")
        .args(["--summary"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let line = line.trim();
            if line.starts_with("deviceName") {
                if let Some(name) = line.split('=').nth(1) {
                    let name = name.trim().trim_matches('"').to_string();
                    if !name.is_empty() && !gpus.contains(&name) {
                        gpus.push(name);
                    }
                }
            }
        }
    }
    gpus
}

/// Validate the entire config
pub fn validate_config(config: &Config) -> Result<(), Vec<String>> {
    let mut errors = vec![];
    for profile in &config.profiles {
        if let Err(e) = profile.validate() {
            errors.push(e);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
