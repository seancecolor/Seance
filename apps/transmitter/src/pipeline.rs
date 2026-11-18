use crate::config::{TelemetrySnapshot, TransmitterConfig};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_video as _;
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
}

struct ActivePipeline {
    pipeline: gst::Pipeline,
    video_sel: gst::Element,
    audio_sel: gst::Element,
    v_pad_live: gst::Pad,
    v_pad_slate: gst::Pad,
    a_pad_live: gst::Pad,
    a_pad_slate: gst::Pad,
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

    /// Sends EOS so `libDeckLinkAPI.so` flushes and releases the PCIe device cleanly
    /// before transitioning the pipeline to `Null`.
    fn stop_cleanly(self) {
        let _ = self.pipeline.send_event(gst::event::Eos::new());
        if let Some(bus) = self.pipeline.bus() {
            let _ = bus.timed_pop_filtered(
                Some(gst::ClockTime::from_mseconds(1500)),
                &[gst::MessageType::Eos, gst::MessageType::Error],
            );
        }
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

fn make_elem(factory: &str, name: &str) -> Result<gst::Element, String> {
    gst::ElementFactory::make(factory)
        .name(name)
        .build()
        .map_err(|e| format!("Missing GStreamer element `{factory}`: {e}"))
}

fn first_available_factory<'a>(candidates: &[&'a str]) -> Option<&'a str> {
    for &name in candidates {
        if gst::ElementFactory::find(name).is_some() {
            return Some(name);
        }
    }
    None
}

fn build_pipeline(cfg: &TransmitterConfig, slate_active: bool) -> Result<ActivePipeline, String> {
    let pipeline = gst::Pipeline::new();
    let (width, height, fps_n, fps_d) = cfg.sdi_mode.dimensions_and_fps();

    // --- Branch A: Live SDI (or --test-pattern) ---
    let (v_live_src, a_live_src) = if cfg.test_pattern {
        let v = make_elem("videotestsrc", "test_v_src")?;
        v.set_property("is-live", true);
        v.set_property_from_str("pattern", "smpte");

        let a = make_elem("audiotestsrc", "test_a_src")?;
        a.set_property("is-live", true);
        a.set_property_from_str("wave", "ticks");
        a.set_property("volume", 0.08_f64);
        (v, a)
    } else {
        let v = make_elem("decklinkvideosrc", "dl_v_src")?;
        v.set_property("device-number", cfg.device_number);
        v.set_property_from_str("connection", "sdi");
        v.set_property_from_str("mode", cfg.sdi_mode.decklink_mode_str());

        let a = make_elem("decklinkaudiosrc", "dl_a_src")?;
        a.set_property("device-number", cfg.device_number);
        (v, a)
    };

    let v_live_conv = make_elem("videoconvert", "v_live_conv")?;
    let v_live_caps = make_elem("capsfilter", "v_live_caps")?;
    if cfg.test_pattern {
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "P010_10LE")
            .field("width", width)
            .field("height", height)
            .field("framerate", gst::Fraction::new(fps_n, fps_d))
            .field("colorimetry", "bt709")
            .build();
        v_live_caps.set_property("caps", &caps);
    } else {
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "P010_10LE")
            .field("colorimetry", "bt709")
            .build();
        v_live_caps.set_property("caps", &caps);
    }

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

    // --- Branch B: Hold / Privacy Slate ---
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

    let a_slate_src = make_elem("audiotestsrc", "slate_a_src")?;
    a_slate_src.set_property("is-live", true);
    a_slate_src.set_property_from_str("wave", "silence");
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
    let video_sel = make_elem("input-selector", "video_sel")?;
    let audio_sel = make_elem("input-selector", "audio_sel")?;

    // --- 10-bit HEVC NVENC Encode + AAC + RTSP-over-TCP Sink ---
    // Lock resolution/fps before nvh265enc so caps changes from DeckLink or
    // input-selector never trigger gst_nv_encoder_reset() inside libnvidia-encode.
    let v_enc_conv = make_elem("videoconvert", "v_enc_conv")?;
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
    v_enc.set_property("gop-size", 24i32);
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

    let aac_factory = first_available_factory(&["voaacenc", "fdkaacenc", "avenc_aac"])
        .ok_or_else(|| "Missing AAC encoder (`voaacenc`, `fdkaacenc`, or `avenc_aac`)".to_string())?;
    let a_enc = make_elem(aac_factory, "aac_enc")?;
    a_enc.set_property("bitrate", 256_000i32);
    let aac_parse = make_elem("aacparse", "aac_parse")?;

    let sink = make_elem("rtspclientsink", "rtsp_sink")?;
    sink.set_property("location", &cfg.rtsp_url);
    sink.set_property_from_str("protocols", "tcp");

    // Add all elements to pipeline
    pipeline
        .add_many([
            &v_live_src,
            &v_live_conv,
            &v_live_caps,
            &a_live_src,
            &a_live_conv,
            &a_live_resamp,
            &a_live_caps,
            &v_slate_src,
            &v_slate_conv,
            &v_slate_caps,
            &a_slate_src,
            &a_slate_conv,
            &a_slate_caps,
            &video_sel,
            &audio_sel,
            &v_enc_conv,
            &v_enc_scale,
            &v_enc_rate,
            &v_enc_caps,
            &v_enc,
            &h265_caps,
            &h265_parse,
            &a_enc,
            &aac_parse,
            &sink,
        ])
        .map_err(|e| format!("Failed to add elements to pipeline: {e}"))?;

    if let Some(ref ov) = v_slate_overlay {
        pipeline
            .add(ov)
            .map_err(|e| format!("Failed to add textoverlay: {e}"))?;
    }

    // Link Branch A
    gst::Element::link_many([&v_live_src, &v_live_conv, &v_live_caps])
        .map_err(|e| format!("Failed to link live video branch: {e}"))?;
    gst::Element::link_many([&a_live_src, &a_live_conv, &a_live_resamp, &a_live_caps])
        .map_err(|e| format!("Failed to link live audio branch: {e}"))?;

    // Link Branch B
    if let Some(ref ov) = v_slate_overlay {
        gst::Element::link_many([&v_slate_src, ov, &v_slate_conv, &v_slate_caps])
            .map_err(|e| format!("Failed to link slate video branch: {e}"))?;
    } else {
        gst::Element::link_many([&v_slate_src, &v_slate_conv, &v_slate_caps])
            .map_err(|e| format!("Failed to link slate video branch: {e}"))?;
    }
    gst::Element::link_many([&a_slate_src, &a_slate_conv, &a_slate_caps])
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

    // Link selectors -> encoders -> rtspclientsink
    gst::Element::link_many([
        &video_sel,
        &v_enc_conv,
        &v_enc_scale,
        &v_enc_rate,
        &v_enc_caps,
        &v_enc,
        &h265_caps,
        &h265_parse,
        &sink,
    ])
    .map_err(|e| format!("Failed to link video encode -> rtspclientsink: {e}"))?;

    gst::Element::link_many([&audio_sel, &a_enc, &aac_parse, &sink])
        .map_err(|e| format!("Failed to link audio encode -> rtspclientsink: {e}"))?;

    let active = ActivePipeline {
        pipeline,
        video_sel,
        audio_sel,
        v_pad_live,
        v_pad_slate,
        a_pad_live,
        a_pad_slate,
    };
    active.set_slate(slate_active);

    active
        .pipeline
        .set_state(gst::State::Playing)
        .map_err(|e| format!("Failed to set pipeline to Playing: {e}"))?;

    Ok(active)
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
                                active = Some(p);
                                let mut t = telemetry.lock().unwrap();
                                t.streaming = true;
                                t.pipeline_state = "Playing".to_string();
                                t.last_error = None;
                                t.reconnect_attempt = 0;
                            }
                            Err(err) => {
                                let mut t = telemetry.lock().unwrap();
                                t.streaming = false;
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
                        t.streaming = false;
                        t.pipeline_state = "Null".to_string();
                        t.negotiated_caps = "—".to_string();
                        t.reconnect_attempt = 0;
                    }
                    PipelineCommand::ToggleStream(cfg) => {
                        if desired_streaming {
                            desired_streaming = false;
                            next_reconnect = None;
                            if let Some(prev) = active.take() {
                                prev.stop_cleanly();
                            }
                            let mut t = telemetry.lock().unwrap();
                            t.streaming = false;
                            t.pipeline_state = "Null".to_string();
                            t.negotiated_caps = "—".to_string();
                        } else {
                            current_cfg = cfg;
                            desired_streaming = true;
                            reconnect_attempt = 0;
                            match build_pipeline(&current_cfg, slate_active) {
                                Ok(p) => {
                                    active = Some(p);
                                    let mut t = telemetry.lock().unwrap();
                                    t.streaming = true;
                                    t.pipeline_state = "Playing".to_string();
                                    t.last_error = None;
                                }
                                Err(err) => {
                                    let mut t = telemetry.lock().unwrap();
                                    t.streaming = false;
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
                }
            }
            _ = tick.tick() => {
                // Poll GStreamer bus messages & negotiated caps
                let mut bus_error: Option<String> = None;
                if let Some(ref p) = active {
                    if let Some(bus) = p.pipeline.bus() {
                        while let Some(msg) = bus.pop() {
                            match msg.view() {
                                gst::MessageView::Error(err) => {
                                    bus_error = Some(format!(
                                        "{}: {}",
                                        err.error(),
                                        err.debug().unwrap_or_default()
                                    ));
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
                }

                if let Some(err) = bus_error {
                    if let Some(prev) = active.take() {
                        prev.stop_cleanly();
                    }
                    reconnect_attempt = reconnect_attempt.saturating_add(1);
                    let backoff_secs = (1u64 << reconnect_attempt.min(4)).min(15);
                    next_reconnect = Some(tokio::time::Instant::now() + Duration::from_secs(backoff_secs));
                    let mut t = telemetry.lock().unwrap();
                    t.streaming = false;
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
                            active = Some(p);
                            let mut t = telemetry.lock().unwrap();
                            t.streaming = true;
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
