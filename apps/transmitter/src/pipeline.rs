use crate::config::{SdiMode, TelemetrySnapshot, TransmitterConfig};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_video as _;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum PipelineCommand {
    Start(TransmitterConfig),
    Stop,
    ToggleStream(TransmitterConfig),
    SetSlate(bool),
    ToggleSlate,
    Shutdown(SyncSender<()>),
}

struct ActivePipeline {
    pipeline: gst::Pipeline,
    video_sel: gst::Element,
    audio_sel: gst::Element,
    v_pad_live: gst::Pad,
    v_pad_slate: gst::Pad,
    a_pad_live: gst::Pad,
    a_pad_slate: gst::Pad,
    probe_bytes: Arc<AtomicU64>,
    probe_frames: Arc<AtomicU64>,
    started_at: tokio::time::Instant,
    slate_mode: String,
}

impl ActivePipeline {
    fn set_slate(&self, slate: bool) {
        let v_target = if slate {
            &self.v_pad_slate
        } else {
            &self.v_pad_live
        };
        let a_target = if slate {
            &self.a_pad_slate
        } else {
            &self.a_pad_live
        };
        self.video_sel.set_property("active-pad", v_target);
        self.audio_sel.set_property("active-pad", a_target);
    }

    /// Sends EOS only when the pipeline has actually reached `Playing` so
    /// `libDeckLinkAPI.so` flushes and releases the PCIe device cleanly without
    /// deadlocking on a broken or mid-preroll pipeline.
    fn stop_cleanly(self) {
        if self.pipeline.current_state() == gst::State::Playing {
            let _ = self.pipeline.send_event(gst::event::Eos::new());
            if let Some(bus) = self.pipeline.bus() {
                let _ = bus.timed_pop_filtered(
                    Some(gst::ClockTime::from_mseconds(1500)),
                    &[gst::MessageType::Eos, gst::MessageType::Error],
                );
            }
        }
        let _ = self.pipeline.set_state(gst::State::Null);
    }

    /// Immediate teardown to `Null` without sending EOS — used on bus errors
    /// where streaming threads or `rtspclientsink` are already failing/prerolling.
    fn stop_immediately(self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

impl Drop for ActivePipeline {
    fn drop(&mut self) {
        if self.pipeline.current_state() != gst::State::Null {
            let _ = self.pipeline.set_state(gst::State::Null);
        }
    }
}

fn make_elem(factory: &str, name: &str) -> Result<gst::Element, String> {
    gst::ElementFactory::make(factory)
        .name(name)
        .build()
        .map_err(|e| format!("Missing GStreamer element `{factory}`: {e}"))
}

fn make_queue(name: &str, max_buffers: u32, leaky: bool) -> Result<gst::Element, String> {
    let q = make_elem("queue", name)?;
    q.set_property("max-size-buffers", max_buffers);
    q.set_property("max-size-bytes", 0u32);
    q.set_property("max-size-time", 0u64);
    if leaky {
        q.set_property_from_str("leaky", "downstream");
    }
    Ok(q)
}

fn first_available_factory<'a>(candidates: &[&'a str]) -> Option<&'a str> {
    for &name in candidates {
        if gst::ElementFactory::find(name).is_some() {
            return Some(name);
        }
    }
    None
}

fn build_live_video_branch(
    pipeline: &gst::Pipeline,
    cfg: &TransmitterConfig,
    v_live_caps: &gst::Element,
) -> Result<(), String> {
    let (width, height, fps_n, fps_d) = cfg.sdi_mode.dimensions_and_fps();
    let v_live_q = make_queue("v_live_q", 3, true)?;

    if cfg.test_pattern {
        let v_live_src = make_elem("videotestsrc", "test_v_src")?;
        v_live_src.set_property("is-live", true);
        v_live_src.set_property_from_str("pattern", "smpte");

        let v_live_conv = make_elem("videoconvert", "v_live_conv")?;
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "P010_10LE")
            .field("width", width)
            .field("height", height)
            .field("framerate", gst::Fraction::new(fps_n, fps_d))
            .field("colorimetry", "bt709")
            .build();
        v_live_caps.set_property("caps", &caps);

        pipeline
            .add_many([&v_live_src, &v_live_q, &v_live_conv, v_live_caps])
            .map_err(|e| format!("Failed to add test video elements: {e}"))?;
        gst::Element::link_many([&v_live_src, &v_live_q, &v_live_conv, v_live_caps])
            .map_err(|e| format!("Failed to link test video branch: {e}"))?;
        return Ok(());
    }

    // Live DeckLink SDI capture.
    // `video-format=10bit-yuv` (v210) is locked for explicit modes; `auto` is required
    // when `mode=auto` per GStreamer's decklinkvideosrc documentation.
    let v_live_src = make_elem("decklinkvideosrc", "dl_v_src")?;
    v_live_src.set_property("device-number", cfg.device_number);
    v_live_src.set_property_from_str("connection", "sdi");
    v_live_src.set_property_from_str("mode", cfg.sdi_mode.decklink_mode_str());
    let video_fmt = if cfg.sdi_mode == SdiMode::Auto {
        "auto"
    } else {
        "10bit-yuv"
    };
    v_live_src.set_property_from_str("video-format", video_fmt);

    // Step v210 (packed 10-bit 4:2:2, 128-byte aligned stride) -> I422_10LE (planar 10-bit 4:2:2)
    // -> P010_10LE (semi-planar 10-bit 4:2:0) to avoid direct v210->P010 stride/padding edge
    // cases in libgstvideoconvertscale.
    let v_live_conv1 = make_elem("videoconvert", "v_live_conv1")?;
    let v_live_mid_caps = make_elem("capsfilter", "v_live_mid_caps")?;
    v_live_mid_caps.set_property(
        "caps",
        &gst::Caps::builder("video/x-raw")
            .field("format", "I422_10LE")
            .build(),
    );
    let v_live_conv2 = make_elem("videoconvert", "v_live_conv2")?;
    v_live_caps.set_property(
        "caps",
        &gst::Caps::builder("video/x-raw")
            .field("format", "P010_10LE")
            .field("colorimetry", "bt709")
            .build(),
    );

    pipeline
        .add_many([
            &v_live_src,
            &v_live_q,
            &v_live_conv1,
            &v_live_mid_caps,
            &v_live_conv2,
            v_live_caps,
        ])
        .map_err(|e| format!("Failed to add live SDI video elements: {e}"))?;
    gst::Element::link_many([
        &v_live_src,
        &v_live_q,
        &v_live_conv1,
        &v_live_mid_caps,
        &v_live_conv2,
        v_live_caps,
    ])
    .map_err(|e| format!("Failed to link live SDI video branch: {e}"))?;

    Ok(())
}

fn build_slate_video_branch(
    pipeline: &gst::Pipeline,
    slate_image_path: &str,
    v_slate_caps: &gst::Element,
) -> Result<String, String> {
    let v_slate_q = make_queue("v_slate_q", 2, true)?;
    let trimmed = slate_image_path.trim();
    if !trimmed.is_empty() && Path::new(trimmed).is_file() {
        let ext = Path::new(trimmed)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let decoder_factory = match ext.as_str() {
            "png" => Some("pngdec"),
            "jpg" | "jpeg" => Some("jpegdec"),
            _ => None,
        };

        if let Some(dec_name) = decoder_factory
            && gst::ElementFactory::find(dec_name).is_some()
            && gst::ElementFactory::find("imagefreeze").is_some()
        {
            let filesrc = make_elem("filesrc", "slate_file_src")?;
            filesrc.set_property("location", trimmed);
            let dec = make_elem(dec_name, "slate_img_dec")?;
            let freeze = make_elem("imagefreeze", "slate_freeze")?;
            freeze.set_property("is-live", true);
            let conv = make_elem("videoconvert", "v_slate_conv")?;
            let scale = make_elem("videoscale", "v_slate_scale")?;

            pipeline
                .add_many([&filesrc, &dec, &freeze, &conv, &scale, &v_slate_q, v_slate_caps])
                .map_err(|e| format!("Failed to add image slate elements: {e}"))?;
            gst::Element::link_many([&filesrc, &dec, &freeze, &conv, &scale, &v_slate_q, v_slate_caps])
                .map_err(|e| format!("Failed to link image slate branch: {e}"))?;

            return Ok(format!(
                "Custom Image ({})",
                Path::new(trimmed)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("slate")
            ));
        }
    }

    // Default black slate with centered text overlay
    let v_slate_src = make_elem("videotestsrc", "slate_v_src")?;
    v_slate_src.set_property("is-live", true);
    v_slate_src.set_property_from_str("pattern", "black");

    let v_slate_overlay = gst::ElementFactory::make("textoverlay")
        .name("slate_text")
        .build()
        .ok();
    if let Some(ref ov) = v_slate_overlay {
        ov.set_property("text", "GRADING SETUP — STAND BY");
        ov.set_property_from_str("valignment", "center");
        ov.set_property_from_str("halignment", "center");
        ov.set_property_from_str("font-desc", "Sans Bold 36");
    }

    let v_slate_conv = make_elem("videoconvert", "v_slate_conv")?;
    pipeline
        .add_many([&v_slate_src, &v_slate_conv, &v_slate_q, v_slate_caps])
        .map_err(|e| format!("Failed to add default slate elements: {e}"))?;

    if let Some(ref ov) = v_slate_overlay {
        pipeline
            .add(ov)
            .map_err(|e| format!("Failed to add textoverlay: {e}"))?;
        gst::Element::link_many([&v_slate_src, ov, &v_slate_conv, &v_slate_q, v_slate_caps])
            .map_err(|e| format!("Failed to link slate video branch: {e}"))?;
    } else {
        gst::Element::link_many([&v_slate_src, &v_slate_conv, &v_slate_q, v_slate_caps])
            .map_err(|e| format!("Failed to link slate video branch: {e}"))?;
    }

    Ok("Default Text Slate".to_string())
}

fn build_pipeline(cfg: &TransmitterConfig, slate_active: bool) -> Result<ActivePipeline, String> {
    let pipeline = gst::Pipeline::new();
    let (width, height, fps_n, fps_d) = cfg.sdi_mode.dimensions_and_fps();

    // --- Branch A: Live SDI (or --test-pattern) ---
    let v_live_caps = make_elem("capsfilter", "v_live_caps")?;
    build_live_video_branch(&pipeline, cfg, &v_live_caps)?;

    let a_live_src = if cfg.test_pattern {
        let a = make_elem("audiotestsrc", "test_a_src")?;
        a.set_property("is-live", true);
        a.set_property_from_str("wave", "ticks");
        a.set_property("volume", 0.08_f64);
        a
    } else {
        let a = make_elem("decklinkaudiosrc", "dl_a_src")?;
        a.set_property("device-number", cfg.device_number);
        a
    };

    let a_live_q = make_queue("a_live_q", 8, true)?;
    let a_live_conv = make_elem("audioconvert", "a_live_conv")?;
    let a_live_resamp = make_elem("audioresample", "a_live_resamp")?;
    let a_live_caps = make_elem("capsfilter", "a_live_caps")?;
    a_live_caps.set_property(
        "caps",
        &gst::Caps::builder("audio/x-raw")
            .field("rate", 48000i32)
            .field("channels", 2i32)
            .build(),
    );

    // --- Branch B: Hold / Privacy Slate (Custom PNG/JPG or Default Text Slate) ---
    let v_slate_caps = make_elem("capsfilter", "v_slate_caps")?;
    v_slate_caps.set_property(
        "caps",
        &gst::Caps::builder("video/x-raw")
            .field("format", "P010_10LE")
            .field("width", width)
            .field("height", height)
            .field("framerate", gst::Fraction::new(fps_n, fps_d))
            .field("colorimetry", "bt709")
            .build(),
    );
    let slate_mode = build_slate_video_branch(&pipeline, &cfg.slate_image_path, &v_slate_caps)?;

    let a_slate_src = make_elem("audiotestsrc", "slate_a_src")?;
    a_slate_src.set_property("is-live", true);
    a_slate_src.set_property_from_str("wave", "silence");
    let a_slate_q = make_queue("a_slate_q", 8, true)?;
    let a_slate_conv = make_elem("audioconvert", "a_slate_conv")?;
    let a_slate_caps = make_elem("capsfilter", "a_slate_caps")?;
    a_slate_caps.set_property(
        "caps",
        &gst::Caps::builder("audio/x-raw")
            .field("rate", 48000i32)
            .field("channels", 2i32)
            .build(),
    );

    // --- Input Selectors ---
    // Disable sync-streams & cache-buffers so inactive live branches drop frames
    // immediately without stalling or deadlocking DeckLink capture threads.
    let video_sel = make_elem("input-selector", "video_sel")?;
    video_sel.set_property("sync-streams", false);
    video_sel.set_property("cache-buffers", false);

    let audio_sel = make_elem("input-selector", "audio_sel")?;
    audio_sel.set_property("sync-streams", false);
    audio_sel.set_property("cache-buffers", false);

    // --- 10-bit HEVC NVENC Encode + AAC + RTSP-over-TCP Sink ---
    // Both branches already convert to P010_10LE bt709 before video_sel, avoiding
    // a redundant second CPU videoconvert pass (significant savings at 4K).
    // Queues decouple selector threads from encoders/RTSP; videoscale + videorate
    // lock resolution/fps so nvh265enc never resets mid-stream.
    let v_enc_q = make_queue("v_enc_q", 4, false)?;
    let v_enc_scale = make_elem("videoscale", "v_enc_scale")?;
    let v_enc_rate = make_elem("videorate", "v_enc_rate")?;
    let v_enc_caps = make_elem("capsfilter", "v_enc_caps")?;
    v_enc_caps.set_property(
        "caps",
        &gst::Caps::builder("video/x-raw")
            .field("format", "P010_10LE")
            .field("width", width)
            .field("height", height)
            .field("framerate", gst::Fraction::new(fps_n, fps_d))
            .field("colorimetry", "bt709")
            .build(),
    );

    let nvenc_factory = first_available_factory(&["nvh265enc", "nvautogpuhevcenc"])
        .ok_or_else(|| "Missing NVIDIA HEVC encoder (`nvh265enc` or `nvautogpuhevcenc`)".to_string())?;
    let v_enc = make_elem(nvenc_factory, "hevc_enc")?;
    v_enc.set_property("bitrate", cfg.bitrate_mbps * 1000);
    v_enc.set_property_from_str("rc-mode", "cbr");
    v_enc.set_property("gop-size", cfg.sdi_mode.gop_size());
    v_enc.set_property("zerolatency", true);

    let h265_caps = make_elem("capsfilter", "h265_caps")?;
    h265_caps.set_property(
        "caps",
        &gst::Caps::builder("video/x-h265")
            .field("profile", "main-10")
            .build(),
    );

    let h265_parse = make_elem("h265parse", "h265_parse")?;
    h265_parse.set_property("config-interval", -1i32);

    // Lightweight pad probe for live outbound Mbps & FPS telemetry
    let probe_bytes = Arc::new(AtomicU64::new(0));
    let probe_frames = Arc::new(AtomicU64::new(0));
    if let Some(src_pad) = h265_parse.static_pad("src") {
        let bytes_counter = probe_bytes.clone();
        let frames_counter = probe_frames.clone();
        src_pad.add_probe(gst::PadProbeType::BUFFER, move |_pad, info| {
            if let Some(gst::PadProbeData::Buffer(ref buf)) = info.data {
                bytes_counter.fetch_add(buf.size() as u64, Ordering::Relaxed);
                frames_counter.fetch_add(1, Ordering::Relaxed);
            }
            gst::PadProbeReturn::Ok
        });
    }

    let a_enc_q = make_queue("a_enc_q", 16, false)?;
    let aac_factory = first_available_factory(&["voaacenc", "fdkaacenc", "avenc_aac"])
        .ok_or_else(|| "Missing AAC encoder (`voaacenc`, `fdkaacenc`, or `avenc_aac`)".to_string())?;
    let a_enc = make_elem(aac_factory, "aac_enc")?;
    a_enc.set_property("bitrate", 256_000i32);
    let aac_parse = make_elem("aacparse", "aac_parse")?;

    let sink = make_elem("rtspclientsink", "rtsp_sink")?;
    sink.set_property("location", &cfg.rtsp_url);
    sink.set_property_from_str("protocols", "tcp");

    // Add remaining elements to pipeline (v_live_* and v_slate_* already added by helpers)
    pipeline
        .add_many([
            &a_live_src,
            &a_live_q,
            &a_live_conv,
            &a_live_resamp,
            &a_live_caps,
            &a_slate_src,
            &a_slate_q,
            &a_slate_conv,
            &a_slate_caps,
            &video_sel,
            &audio_sel,
            &v_enc_q,
            &v_enc_scale,
            &v_enc_rate,
            &v_enc_caps,
            &v_enc,
            &h265_caps,
            &h265_parse,
            &a_enc_q,
            &a_enc,
            &aac_parse,
            &sink,
        ])
        .map_err(|e| format!("Failed to add elements to pipeline: {e}"))?;

    // Link Branch A audio
    gst::Element::link_many([
        &a_live_src,
        &a_live_q,
        &a_live_conv,
        &a_live_resamp,
        &a_live_caps,
    ])
    .map_err(|e| format!("Failed to link live audio branch: {e}"))?;

    // Link Branch B audio
    gst::Element::link_many([&a_slate_src, &a_slate_q, &a_slate_conv, &a_slate_caps])
        .map_err(|e| format!("Failed to link slate audio branch: {e}"))?;

    // Request selector sink pads and link branches
    let v_pad_live = video_sel
        .request_pad_simple("sink_%u")
        .ok_or("Failed to request video_sel live pad")?;
    let v_pad_slate = video_sel
        .request_pad_simple("sink_%u")
        .ok_or("Failed to request video_sel slate pad")?;
    v_live_caps
        .static_pad("src")
        .unwrap()
        .link(&v_pad_live)
        .map_err(|e| format!("Link live video -> selector failed: {e}"))?;
    v_slate_caps
        .static_pad("src")
        .unwrap()
        .link(&v_pad_slate)
        .map_err(|e| format!("Link slate video -> selector failed: {e}"))?;

    let a_pad_live = audio_sel
        .request_pad_simple("sink_%u")
        .ok_or("Failed to request audio_sel live pad")?;
    let a_pad_slate = audio_sel
        .request_pad_simple("sink_%u")
        .ok_or("Failed to request audio_sel slate pad")?;
    a_live_caps
        .static_pad("src")
        .unwrap()
        .link(&a_pad_live)
        .map_err(|e| format!("Link live audio -> selector failed: {e}"))?;
    a_slate_caps
        .static_pad("src")
        .unwrap()
        .link(&a_pad_slate)
        .map_err(|e| format!("Link slate audio -> selector failed: {e}"))?;

    // Link selectors -> queues -> encoders -> rtspclientsink
    gst::Element::link_many([
        &video_sel,
        &v_enc_q,
        &v_enc_scale,
        &v_enc_rate,
        &v_enc_caps,
        &v_enc,
        &h265_caps,
        &h265_parse,
        &sink,
    ])
    .map_err(|e| format!("Failed to link video encode -> rtspclientsink: {e}"))?;

    gst::Element::link_many([&audio_sel, &a_enc_q, &a_enc, &aac_parse, &sink])
        .map_err(|e| format!("Failed to link audio encode -> rtspclientsink: {e}"))?;

    let active = ActivePipeline {
        pipeline,
        video_sel,
        audio_sel,
        v_pad_live,
        v_pad_slate,
        a_pad_live,
        a_pad_slate,
        probe_bytes,
        probe_frames,
        started_at: tokio::time::Instant::now(),
        slate_mode,
    };
    active.set_slate(slate_active);

    if let Err(e) = active.pipeline.set_state(gst::State::Playing) {
        let _ = active.pipeline.set_state(gst::State::Null);
        return Err(format!("Failed to set pipeline to Playing: {e}"));
    }

    Ok(active)
}

fn reset_stopped_telemetry(t: &mut TelemetrySnapshot) {
    t.streaming = false;
    t.pipeline_state = "Null".to_string();
    t.negotiated_caps = "—".to_string();
    t.outbound_mbps = 0.0;
    t.actual_fps = 0.0;
    t.uptime_secs = 0;
    t.reconnect_attempt = 0;
}

pub async fn run_pipeline_manager(
    mut cmd_rx: mpsc::Receiver<PipelineCommand>,
    telemetry: Arc<Mutex<TelemetrySnapshot>>,
    initial_cfg: TransmitterConfig,
) {
    let mut active: Option<ActivePipeline> = None;
    let mut desired_streaming = false;
    let mut slate_active = false;
    let mut current_cfg = initial_cfg;
    let mut reconnect_attempt: u32 = 0;
    let mut next_reconnect: Option<tokio::time::Instant> = None;
    let mut last_stats_at = tokio::time::Instant::now();

    let mut tick = tokio::time::interval(Duration::from_millis(200));

    loop {
        tokio::select! {
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    PipelineCommand::Start(cfg) => {
                        current_cfg = cfg;
                        desired_streaming = true;
                        reconnect_attempt = 0;
                        next_reconnect = None;
                        if let Some(prev) = active.take() {
                            prev.stop_cleanly();
                        }
                        match build_pipeline(&current_cfg, slate_active) {
                            Ok(p) => {
                                let slate_mode = p.slate_mode.clone();
                                active = Some(p);
                                last_stats_at = tokio::time::Instant::now();
                                let mut t = telemetry.lock().unwrap();
                                t.streaming = true;
                                t.slate_mode = slate_mode;
                                t.pipeline_state = "Playing".to_string();
                                t.last_error = None;
                                t.reconnect_attempt = 0;
                            }
                            Err(err) => {
                                let mut t = telemetry.lock().unwrap();
                                reset_stopped_telemetry(&mut t);
                                t.pipeline_state = "Error".to_string();
                                t.last_error = Some(err);
                            }
                        }
                    }
                    PipelineCommand::Stop => {
                        desired_streaming = false;
                        next_reconnect = None;
                        reconnect_attempt = 0;
                        if let Some(prev) = active.take() {
                            prev.stop_cleanly();
                        }
                        let mut t = telemetry.lock().unwrap();
                        reset_stopped_telemetry(&mut t);
                    }
                    PipelineCommand::ToggleStream(cfg) => {
                        if desired_streaming {
                            desired_streaming = false;
                            next_reconnect = None;
                            if let Some(prev) = active.take() {
                                prev.stop_cleanly();
                            }
                            let mut t = telemetry.lock().unwrap();
                            reset_stopped_telemetry(&mut t);
                        } else {
                            current_cfg = cfg;
                            desired_streaming = true;
                            reconnect_attempt = 0;
                            match build_pipeline(&current_cfg, slate_active) {
                                Ok(p) => {
                                    let slate_mode = p.slate_mode.clone();
                                    active = Some(p);
                                    last_stats_at = tokio::time::Instant::now();
                                    let mut t = telemetry.lock().unwrap();
                                    t.streaming = true;
                                    t.slate_mode = slate_mode;
                                    t.pipeline_state = "Playing".to_string();
                                    t.last_error = None;
                                }
                                Err(err) => {
                                    let mut t = telemetry.lock().unwrap();
                                    reset_stopped_telemetry(&mut t);
                                    t.pipeline_state = "Error".to_string();
                                    t.last_error = Some(err);
                                }
                            }
                        }
                    }
                    PipelineCommand::SetSlate(val) => {
                        slate_active = val;
                        if let Some(ref p) = active {
                            p.set_slate(slate_active);
                        }
                        telemetry.lock().unwrap().slate_active = slate_active;
                    }
                    PipelineCommand::ToggleSlate => {
                        slate_active = !slate_active;
                        if let Some(ref p) = active {
                            p.set_slate(slate_active);
                        }
                        telemetry.lock().unwrap().slate_active = slate_active;
                    }
                    PipelineCommand::Shutdown(done_tx) => {
                        if let Some(prev) = active.take() {
                            prev.stop_cleanly();
                        }
                        let _ = done_tx.send(());
                        break;
                    }
                }
            }
            _ = tick.tick() => {
                // Poll GStreamer bus messages, negotiated caps, and live Mbps/FPS
                let mut bus_error: Option<String> = None;
                if let Some(ref p) = active {
                    if let Some(bus) = p.pipeline.bus() {
                        while let Some(msg) = bus.pop() {
                            match msg.view() {
                                gst::MessageView::Error(err) => {
                                    let src_name = err
                                        .src()
                                        .map(|s| s.name().to_string())
                                        .unwrap_or_else(|| "pipeline".to_string());
                                    let msg_text = format!(
                                        "[{src_name}] {}: {}",
                                        err.error(),
                                        err.debug().unwrap_or_default()
                                    );
                                    eprintln!("[seance-transmitter] GStreamer ERROR: {msg_text}");
                                    bus_error = Some(msg_text);
                                }
                                gst::MessageView::Warning(warn) => {
                                    let src_name = warn
                                        .src()
                                        .map(|s| s.name().to_string())
                                        .unwrap_or_else(|| "pipeline".to_string());
                                    eprintln!(
                                        "[seance-transmitter] GStreamer WARNING [{src_name}]: {} ({})",
                                        warn.error(),
                                        warn.debug().unwrap_or_default()
                                    );
                                }
                                gst::MessageView::StateChanged(sc) => {
                                    if sc.src().map(|s| s == &p.pipeline).unwrap_or(false) {
                                        telemetry.lock().unwrap().pipeline_state =
                                            format!("{:?}", sc.current());
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    if let Some(pad) = p.video_sel.static_pad("src")
                        && let Some(caps) = pad.current_caps()
                    {
                        telemetry.lock().unwrap().negotiated_caps = caps.to_string();
                    }

                    let now = tokio::time::Instant::now();
                    let elapsed = now.duration_since(last_stats_at).as_secs_f64();
                    if elapsed >= 1.0 {
                        let bytes = p.probe_bytes.swap(0, Ordering::Relaxed);
                        let frames = p.probe_frames.swap(0, Ordering::Relaxed);
                        last_stats_at = now;
                        let mut t = telemetry.lock().unwrap();
                        t.outbound_mbps = (bytes as f64 * 8.0) / (elapsed * 1_000_000.0);
                        t.actual_fps = frames as f64 / elapsed;
                        t.uptime_secs = p.started_at.elapsed().as_secs();
                    }
                }

                if let Some(err) = bus_error {
                    if let Some(prev) = active.take() {
                        prev.stop_immediately();
                    }
                    reconnect_attempt = reconnect_attempt.saturating_add(1);
                    let backoff_secs = (1u64 << reconnect_attempt.min(4)).min(15);
                    next_reconnect = Some(tokio::time::Instant::now() + Duration::from_secs(backoff_secs));
                    let mut t = telemetry.lock().unwrap();
                    t.streaming = false;
                    t.outbound_mbps = 0.0;
                    t.actual_fps = 0.0;
                    t.uptime_secs = 0;
                    t.pipeline_state = format!("Reconnecting in {backoff_secs}s");
                    t.reconnect_attempt = reconnect_attempt;
                    t.last_error = Some(err);
                } else if desired_streaming
                    && active.is_none()
                    && next_reconnect.is_some_and(|when| tokio::time::Instant::now() >= when)
                {
                    next_reconnect = None;
                    match build_pipeline(&current_cfg, slate_active) {
                        Ok(p) => {
                            let slate_mode = p.slate_mode.clone();
                            active = Some(p);
                            last_stats_at = tokio::time::Instant::now();
                            let mut t = telemetry.lock().unwrap();
                            t.streaming = true;
                            t.slate_mode = slate_mode;
                            t.pipeline_state = "Playing".to_string();
                            t.last_error = None;
                        }
                        Err(err) => {
                            reconnect_attempt = reconnect_attempt.saturating_add(1);
                            let backoff_secs = (1u64 << reconnect_attempt.min(4)).min(15);
                            next_reconnect =
                                Some(tokio::time::Instant::now() + Duration::from_secs(backoff_secs));
                            let mut t = telemetry.lock().unwrap();
                            t.last_error = Some(err);
                            t.reconnect_attempt = reconnect_attempt;
                        }
                    }
                }
            }
        }
    }
}
