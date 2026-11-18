mod config;
mod pipeline;
mod webhook;

use clap::Parser;
use config::{SdiMode, TelemetrySnapshot, TransmitterConfig};
use eframe::egui;
use pipeline::{PipelineCommand, run_pipeline_manager};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use webhook::run_webhook_server;

#[derive(Parser, Debug)]
#[command(name = "seance-transmitter", about = "Seance 10-bit DeckLink -> Railway transmitter")]
struct CliArgs {
    /// Use GStreamer videotestsrc/audiotestsrc instead of physical DeckLink hardware
    #[arg(long, default_value_t = false)]
    test_pattern: bool,

    /// RTSP-over-TCP target URL on Railway MediaMTX
    #[arg(long, env = "SEANCE_RTSP_URL")]
    rtsp_url: Option<String>,
}

struct TransmitterApp {
    cfg: TransmitterConfig,
    latest_cfg: Arc<Mutex<TransmitterConfig>>,
    telemetry: Arc<Mutex<TelemetrySnapshot>>,
    cmd_tx: mpsc::Sender<PipelineCommand>,
}

impl eframe::App for TransmitterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        let snap = self.telemetry.lock().unwrap().clone();
        *self.latest_cfg.lock().unwrap() = self.cfg.clone();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("SEANCE // 10-BIT TRANSMITTER");
            ui.label("DeckLink SDI -> P010 BT.709 -> NVENC HEVC Main10 -> RTSP-over-TCP");
            ui.add_space(10.0);

            // Master Controls
            ui.horizontal(|ui| {
                let stream_btn_text = if snap.streaming {
                    "STOP STREAM (EOS)"
                } else {
                    "START STREAM"
                };
                let stream_color = if snap.streaming {
                    egui::Color32::from_rgb(200, 50, 50)
                } else {
                    egui::Color32::from_rgb(35, 150, 85)
                };
                if ui
                    .add_sized(
                        [180.0, 42.0],
                        egui::Button::new(egui::RichText::new(stream_btn_text).strong().color(egui::Color32::WHITE))
                            .fill(stream_color),
                    )
                    .clicked()
                {
                    if snap.streaming {
                        let _ = self.cmd_tx.try_send(PipelineCommand::Stop);
                    } else {
                        let _ = self.cmd_tx.try_send(PipelineCommand::Start(self.cfg.clone()));
                    }
                }

                let slate_text = if snap.slate_active {
                    "PRIVACY SLATE: ON"
                } else {
                    "PRIVACY SLATE: OFF"
                };
                let slate_color = if snap.slate_active {
                    egui::Color32::from_rgb(200, 140, 20)
                } else {
                    egui::Color32::from_rgb(55, 60, 72)
                };
                if ui
                    .add_sized(
                        [180.0, 42.0],
                        egui::Button::new(egui::RichText::new(slate_text).strong().color(egui::Color32::WHITE))
                            .fill(slate_color),
                    )
                    .clicked()
                {
                    let _ = self
                        .cmd_tx
                        .try_send(PipelineCommand::SetSlate(!snap.slate_active));
                }
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // Configuration
            egui::Grid::new("cfg_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("RTSP Target URL:");
                ui.add_enabled(
                    !snap.streaming,
                    egui::TextEdit::singleline(&mut self.cfg.rtsp_url).desired_width(420.0),
                );
                ui.end_row();

                ui.label("DeckLink Device #:");
                ui.add_enabled_ui(!snap.streaming && !self.cfg.test_pattern, |ui| {
                    egui::ComboBox::from_id_salt("device_num")
                        .selected_text(format!("{}", self.cfg.device_number))
                        .show_ui(ui, |ui| {
                            for idx in 0..=3 {
                                ui.selectable_value(&mut self.cfg.device_number, idx, format!("{idx}"));
                            }
                        });
                });
                ui.end_row();

                ui.label("SDI Mode:");
                ui.add_enabled_ui(!snap.streaming, |ui| {
                    egui::ComboBox::from_id_salt("sdi_mode")
                        .selected_text(self.cfg.sdi_mode.label())
                        .show_ui(ui, |ui| {
                            for mode in SdiMode::ALL {
                                ui.selectable_value(&mut self.cfg.sdi_mode, mode, mode.label());
                            }
                        });
                });
                ui.end_row();

                ui.label("Target Bitrate (Mbps):");
                ui.add_enabled(
                    !snap.streaming,
                    egui::Slider::new(&mut self.cfg.bitrate_mbps, 15..=45).suffix(" Mbps"),
                );
                ui.end_row();

                ui.label("Test Pattern (--test-pattern):");
                ui.add_enabled(!snap.streaming, egui::Checkbox::new(&mut self.cfg.test_pattern, "Use videotestsrc instead of DeckLink"));
                ui.end_row();
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // Telemetry
            ui.label(egui::RichText::new("Telemetry").strong());
            ui.monospace(format!("Pipeline State : {}", snap.pipeline_state));
            ui.monospace(format!("Active Branch  : {}", if snap.slate_active { "PRIVACY SLATE" } else if self.cfg.test_pattern { "TEST PATTERN" } else { "LIVE SDI" }));
            ui.monospace(format!("Negotiated Caps: {}", snap.negotiated_caps));
            ui.monospace(format!("Webhook        : http://{}/{{toggle,slate,status}}", self.cfg.webhook_addr));
            if snap.reconnect_attempt > 0 {
                ui.monospace(format!("Reconnects     : {}", snap.reconnect_attempt));
            }
            if let Some(ref err) = snap.last_error {
                ui.colored_label(egui::Color32::from_rgb(240, 90, 90), format!("Error: {err}"));
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let args = CliArgs::parse();
    gstreamer::init().expect("Failed to initialize GStreamer");

    let mut cfg = TransmitterConfig::default();
    cfg.test_pattern = args.test_pattern;
    if let Some(url) = args.rtsp_url {
        cfg.rtsp_url = url;
    }

    let telemetry = Arc::new(Mutex::new(TelemetrySnapshot::default()));
    let latest_cfg = Arc::new(Mutex::new(cfg.clone()));
    let (cmd_tx, cmd_rx) = mpsc::channel::<PipelineCommand>(32);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create Tokio runtime");

    rt.spawn(run_pipeline_manager(cmd_rx, telemetry.clone(), cfg.clone()));
    rt.spawn(run_webhook_server(
        cfg.webhook_addr.clone(),
        cmd_tx.clone(),
        telemetry.clone(),
        latest_cfg.clone(),
    ));

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([640.0, 460.0])
            .with_title("Seance Transmitter"),
        ..Default::default()
    };

    eframe::run_native(
        "Seance Transmitter",
        native_options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(TransmitterApp {
                cfg,
                latest_cfg,
                telemetry,
                cmd_tx,
            }))
        }),
    )
}
