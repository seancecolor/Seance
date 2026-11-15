use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SdiMode {
    Auto,
    P1080_2398,
    P1080_24,
    P2160_2398,
    P2160_24,
}

impl SdiMode {
    pub const ALL: [SdiMode; 5] = [
        SdiMode::Auto,
        SdiMode::P1080_2398,
        SdiMode::P1080_24,
        SdiMode::P2160_2398,
        SdiMode::P2160_24,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SdiMode::Auto => "auto",
            SdiMode::P1080_2398 => "1080p23.98",
            SdiMode::P1080_24 => "1080p24",
            SdiMode::P2160_2398 => "2160p23.98",
            SdiMode::P2160_24 => "2160p24",
        }
    }

    pub fn decklink_mode_str(self) -> &'static str {
        match self {
            SdiMode::Auto => "auto",
            SdiMode::P1080_2398 => "1080p2398",
            SdiMode::P1080_24 => "1080p24",
            SdiMode::P2160_2398 => "2160p2398",
            SdiMode::P2160_24 => "2160p24",
        }
    }

    pub fn dimensions_and_fps(self) -> (i32, i32, i32, i32) {
        match self {
            SdiMode::Auto | SdiMode::P1080_24 => (1920, 1080, 24, 1),
            SdiMode::P1080_2398 => (1920, 1080, 24000, 1001),
            SdiMode::P2160_24 => (3840, 2160, 24, 1),
            SdiMode::P2160_2398 => (3840, 2160, 24000, 1001),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransmitterConfig {
    pub rtsp_url: String,
    pub device_number: i32,
    pub sdi_mode: SdiMode,
    pub bitrate_mbps: u32,
    pub test_pattern: bool,
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
            webhook_addr: "127.0.0.1:9944".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySnapshot {
    pub streaming: bool,
    pub slate_active: bool,
    pub pipeline_state: String,
    pub negotiated_caps: String,
    pub reconnect_attempt: u32,
    pub last_error: Option<String>,
}

impl Default for TelemetrySnapshot {
    fn default() -> Self {
        Self {
            streaming: false,
            slate_active: false,
            pipeline_state: "Null".to_string(),
            negotiated_caps: "—".to_string(),
            reconnect_attempt: 0,
            last_error: None,
        }
    }
}
