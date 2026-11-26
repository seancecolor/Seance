use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SdiMode {
    Auto,
    P1080_2398,
    P1080_24,
    P1080_25,
    P1080_2997,
    P2160_2398,
    P2160_24,
    P2160_25,
    P2160_2997,
}

impl SdiMode {
    pub const ALL: [SdiMode; 9] = [
        SdiMode::Auto,
        SdiMode::P1080_2398,
        SdiMode::P1080_24,
        SdiMode::P1080_25,
        SdiMode::P1080_2997,
        SdiMode::P2160_2398,
        SdiMode::P2160_24,
        SdiMode::P2160_25,
        SdiMode::P2160_2997,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SdiMode::Auto => "auto (1080p24)",
            SdiMode::P1080_2398 => "1080p23.98",
            SdiMode::P1080_24 => "1080p24",
            SdiMode::P1080_25 => "1080p25",
            SdiMode::P1080_2997 => "1080p29.97",
            SdiMode::P2160_2398 => "2160p23.98",
            SdiMode::P2160_24 => "2160p24",
            SdiMode::P2160_25 => "2160p25",
            SdiMode::P2160_2997 => "2160p29.97",
        }
    }

    pub fn decklink_mode_str(self) -> &'static str {
        match self {
            SdiMode::Auto => "auto",
            SdiMode::P1080_2398 => "1080p2398",
            SdiMode::P1080_24 => "1080p24",
            SdiMode::P1080_25 => "1080p25",
            SdiMode::P1080_2997 => "1080p2997",
            SdiMode::P2160_2398 => "2160p2398",
            SdiMode::P2160_24 => "2160p24",
            SdiMode::P2160_25 => "2160p25",
            SdiMode::P2160_2997 => "2160p2997",
        }
    }

    pub fn dimensions_and_fps(self) -> (i32, i32, i32, i32) {
        match self {
            SdiMode::Auto | SdiMode::P1080_24 => (1920, 1080, 24, 1),
            SdiMode::P1080_2398 => (1920, 1080, 24000, 1001),
            SdiMode::P1080_25 => (1920, 1080, 25, 1),
            SdiMode::P1080_2997 => (1920, 1080, 30000, 1001),
            SdiMode::P2160_24 => (3840, 2160, 24, 1),
            SdiMode::P2160_2398 => (3840, 2160, 24000, 1001),
            SdiMode::P2160_25 => (3840, 2160, 25, 1),
            SdiMode::P2160_2997 => (3840, 2160, 30000, 1001),
        }
    }

    /// 1-second IDR keyframe interval aligned with MediaMTX `hlsSegmentDuration: 1s`.
    pub fn gop_size(self) -> i32 {
        match self {
            SdiMode::Auto
            | SdiMode::P1080_2398
            | SdiMode::P1080_24
            | SdiMode::P2160_2398
            | SdiMode::P2160_24 => 24,
            SdiMode::P1080_25 | SdiMode::P2160_25 => 25,
            SdiMode::P1080_2997 | SdiMode::P2160_2997 => 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TransmitterConfig {
    pub rtsp_url: String,
    pub device_number: i32,
    pub sdi_mode: SdiMode,
    pub bitrate_mbps: u32,
    pub test_pattern: bool,
    pub slate_image_path: String,
    pub webhook_addr: String,
}

impl Default for TransmitterConfig {
    fn default() -> Self {
        Self {
            rtsp_url: "rtsp://seance_ingest:change_me_in_railway@127.0.0.1:8554/cleanfeed"
                .to_string(),
            device_number: 1,
            sdi_mode: SdiMode::Auto,
            bitrate_mbps: 25,
            test_pattern: false,
            slate_image_path: String::new(),
            webhook_addr: "127.0.0.1:9944".to_string(),
        }
    }
}

impl TransmitterConfig {
    pub fn config_path() -> Option<PathBuf> {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
            && !xdg.trim().is_empty()
        {
            return Some(PathBuf::from(xdg).join("seance-transmitter").join("config.json"));
        }
        std::env::var("HOME")
            .ok()
            .filter(|h| !h.trim().is_empty())
            .map(|home| {
                PathBuf::from(home)
                    .join(".config")
                    .join("seance-transmitter")
                    .join("config.json")
            })
    }

    pub fn load_or_default() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    pub fn save(&self) -> Result<PathBuf, String> {
        let path = Self::config_path().ok_or("Could not determine config directory")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config dir: {e}"))?;
        }
        let body = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {e}"))?;
        std::fs::write(&path, body).map_err(|e| format!("Failed to write config: {e}"))?;
        Ok(path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySnapshot {
    pub streaming: bool,
    pub slate_active: bool,
    pub slate_mode: String,
    pub pipeline_state: String,
    pub negotiated_caps: String,
    pub outbound_mbps: f64,
    pub actual_fps: f64,
    pub uptime_secs: u64,
    pub reconnect_attempt: u32,
    pub last_error: Option<String>,
}

impl Default for TelemetrySnapshot {
    fn default() -> Self {
        Self {
            streaming: false,
            slate_active: false,
            slate_mode: "Default Text Slate".to_string(),
            pipeline_state: "Null".to_string(),
            negotiated_caps: "—".to_string(),
            outbound_mbps: 0.0,
            actual_fps: 0.0,
            uptime_secs: 0,
            reconnect_attempt: 0,
            last_error: None,
        }
    }
}
