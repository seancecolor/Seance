# Project: Seance v2 — 10-Bit Remote Color Grading Suite (DeckLink -> Railway -> Zoom App)

## Architectural Constraints (Non-Negotiable)
1. **No WebRTC SFUs / No Public UDP on Railway:** Railway only exposes HTTP/WebSockets (`443`) and a single raw TCP Proxy port. Do NOT configure SRT or WebRTC/WHEP over public UDP.
2. **10-Bit Color Pipeline:** Capture uncompressed 10-bit SDI from a Blackmagic DeckLink Mini Recorder 4K (fed via SDI loop from a DeckLink Mini Monitor 4K), preserve `bt709` colorimetry and `P010_10LE` 10-bit pixel format, encode to H.265 `main-10` via NVIDIA NVENC (`nvh265enc`), and push via interleaved **RTSP-over-TCP** to MediaMTX on Railway.
3. **Client Playback:** MediaMTX remuxes (zero transcode) into **Low-Latency HLS (`fMP4` 10-bit HEVC + AAC)** served over HTTPS. Safari/iPadOS uses native `<video>` playback; Chromium/Windows uses `hls.js` (`^1.7.4` full build, never light build).
4. **Decoupled State Sync:** Chat, room presence, and frame-locked canvas telestration (`DrawingStroke` vectors normalized `0.0..1.0`) run over standard TCP WebSockets (`wss://`) in the web backend—completely independent of video transport.
5. **Dual Runtime (Standalone Browser + Zoom App):** The SvelteKit frontend must detect `@zoom/appssdk` (`^0.16.43`) at startup via `zoomSdk.config()` and gracefully fall back to standalone browser mode when opened via a normal URL in Safari/Chrome/iPadOS.

---

## Monorepo Layout

```text
seance-v2/
├── apps/
│   ├── transmitter/      # Local Linux Rust GUI + GStreamer 10-bit DeckLink transmitter
│   └── web/              # SvelteKit (Svelte 5) web player, WS chat/canvas server, Zoom SDK bridge
└── infra/
    └── mediamtx/         # Dockerfile & Railway config for bluenviron/mediamtx:1.21.3
```

---

## Component 1: `apps/transmitter` (Rust Desktop GUI on Linux Wayland)
- **Stack:** Rust (1.92+), `eframe`/`egui` (native Wayland dark UI), `gstreamer = "0.25.4"`, `gstreamer-video = "0.25.4"`, `tokio`, `serde`.
- **GStreamer Pipeline Graph:**
  - **Branch A (Live SDI):** `decklinkvideosrc device-number=1 connection=sdi mode=auto` + `decklinkaudiosrc device-number=1` (or `videotestsrc` / `audiotestsrc` with `--test-pattern`).
  - **Branch B (Hold/Privacy Slate):** `videotestsrc` ("Grading Setup — Stand By") / `imagefreeze` + silent `audiotestsrc wave=silence`.
  - **Switcher:** `input-selector` for video and audio allowing zero-drop switching between Live SDI and Privacy Slate without disconnecting the RTSP stream.
  - **Encode & Sink:** `videoconvert ! video/x-raw,format=P010_10LE,colorimetry=bt709 ! nvh265enc bitrate=25000 rc-mode=cbr gop-size=24 zerolatency=true ! video/x-h265,profile=main-10 ! h265parse config-interval=-1 ! rtspclientsink protocols=tcp location="rtsp://..."` alongside AAC audio (`voaacenc` / `fdkaacenc` at 256 kbps).
- **GUI Controls:**
  - Master `STREAM ON / OFF` switch (sends clean EOS before setting pipeline to `Null` so `libDeckLinkAPI.so` releases the PCIe card cleanly).
  - `PRIVACY SLATE` toggle button.
  - Dropdowns for DeckLink `device-number`, SDI mode (`auto`, `1080p23.98`, `1080p24`, `2160p23.98`, `2160p24`), and target bitrate (`15–45` Mbps).
  - Live telemetry readout (pipeline state, negotiated caps/fps, reconnect backoff) + localhost HTTP webhook (`127.0.0.1:9944/toggle` and `/slate`) for hardware Stream Deck / OpenDeck triggers.

## Component 2: `infra/mediamtx` (Railway Media Router)
- **Base Image:** `bluenviron/mediamtx:1.21.3`
- **Configuration (`mediamtx.yml` + env overrides):**
  - Disable UDP listeners: `srt: no`, `webrtc: no`, `rtmp: no`.
  - Enable TCP ingest: `rtsp: yes`, `rtspTransports: [tcp]`, `rtspAddress: :8554` (exposed via Railway TCP Proxy).
  - Enable LL-HLS egress: `hls: yes`, `hlsAddress: :8888`, `hlsVariant: lowLatency`, `hlsSegmentCount: 7`, `hlsSegmentDuration: 1s`, `hlsPartDuration: 200ms`, `hlsAlwaysRemux: yes`, `hlsAllowOrigin: '*'`.
  - Publish authentication via env vars (`MTX_AUTHINTERNALUSERS_0_USER` / `PASS`).

## Component 3: `apps/web` (SvelteKit + WebSockets + `@zoom/appssdk`)
- **Video Player:**
  - Check native HEVC LL-HLS support first (Safari / iPadOS Reference Mode); otherwise attach `hls.js` (`lowLatencyMode: true`, `maxLiveSyncPlaybackRate: 1.5`).
  - Include a 1-click "Re-sync to Live Edge" button, audio mute/volume slider, and aspect-ratio-locked `<canvas>` overlay directly bound to the rendered `<video>` bounding rect (`ResizeObserver`).
- **WebSocket Room Server:**
  - Key rooms by `roomId` (derived either from decrypted `X-Zoom-App-Context` meeting UUID via AES-GCM with `ZOOM_CLIENT_SECRET`, or a signed URL token for browser/iPad links).
  - Broadcast chat messages, participant presence, and live telestration strokes (`draw`, `undo`, `clear`).
- **Zoom Apps SDK & OWASP Headers:**
  - Initialize `zoomSdk.config()` with capabilities: `getRunningContext`, `getUserContext`, `startCollaborate`, `endCollaborate`, `appPopout`, `openUrl`, `onCollaborateChange`.
  - Inject mandatory Zoom OWASP response headers in SvelteKit `hooks.server.ts`: `Strict-Transport-Security`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: strict-origin-when-cross-origin`, and a strict `Content-Security-Policy` whitelisting the Railway MediaMTX domain in `media-src` and `connect-src`.
```

---

## Component 1: `apps/transmitter` (Rust Desktop GUI on Linux Wayland)
- **Stack:** Rust (1.92+), `eframe`/`egui` (native Wayland dark UI), `gstreamer = "0.25.4"`, `gstreamer-video = "0.25.4"`, `tokio`, `serde`.
- **GStreamer Pipeline Graph:**
  - **Branch A (Live SDI):** `decklinkvideosrc device-number=1 connection=sdi mode=auto` + `decklinkaudiosrc device-number=1` (or `videotestsrc` / `audiotestsrc` with `--test-pattern`).
  - **Branch B (Hold/Privacy Slate):** `videotestsrc` ("Grading Setup — Stand By") / `imagefreeze` + silent `audiotestsrc wave=silence`.
  - **Switcher:** `input-selector` for video and audio allowing zero-drop switching between Live SDI and Privacy Slate without disconnecting the RTSP stream.
  - **Encode & Sink:** `videoconvert ! video/x-raw,format=P010_10LE,colorimetry=bt709 ! nvh265enc bitrate=25000 rc-mode=cbr gop-size=24 zerolatency=true ! video/x-h265,profile=main-10 ! h265parse config-interval=-1 ! rtspclientsink protocols=tcp location="rtsp://..."` alongside AAC audio (`voaacenc` / `fdkaacenc` at 256 kbps).
- **GUI Controls:**
  - Master `STREAM ON / OFF` switch (sends clean EOS before setting pipeline to `Null` so `libDeckLinkAPI.so` releases the PCIe card cleanly).
  - `PRIVACY SLATE` toggle button.
  - Dropdowns for DeckLink `device-number`, SDI mode (`auto`, `1080p23.98`, `1080p24`, `2160p23.98`, `2160p24`), and target bitrate (`15–45` Mbps).
  - Live telemetry readout (pipeline state, negotiated caps/fps, reconnect backoff) + localhost HTTP webhook (`127.0.0.1:9944/toggle` and `/slate`) for hardware Stream Deck / OpenDeck triggers.

## Component 2: `infra/mediamtx` (Railway Media Router)
- **Base Image:** `bluenviron/mediamtx:1.21.3`
- **Configuration (`mediamtx.yml` + env overrides):**
  - Disable UDP listeners: `srt: no`, `webrtc: no`, `rtmp: no`.
  - Enable TCP ingest: `rtsp: yes`, `rtspTransports: [tcp]`, `rtspAddress: :8554` (exposed via Railway TCP Proxy).
  - Enable LL-HLS egress: `hls: yes`, `hlsAddress: :8888`, `hlsVariant: lowLatency`, `hlsSegmentCount: 7`, `hlsSegmentDuration: 1s`, `hlsPartDuration: 200ms`, `hlsAlwaysRemux: yes`, `hlsAllowOrigin: '*'`.
  - Publish authentication via env vars (`MTX_AUTHINTERNALUSERS_0_USER` / `PASS`).

## Component 3: `apps/web` (SvelteKit + WebSockets + `@zoom/appssdk`)
- **Video Player:**
  - Check native HEVC LL-HLS support first (Safari / iPadOS Reference Mode); otherwise attach `hls.js` (`lowLatencyMode: true`, `maxLiveSyncPlaybackRate: 1.5`).
  - Include a 1-click "Re-sync to Live Edge" button, audio mute/volume slider, and aspect-ratio-locked `<canvas>` overlay directly bound to the rendered `<video>` bounding rect (`ResizeObserver`).
- **WebSocket Room Server:**
  - Key rooms by `roomId` (derived either from decrypted `X-Zoom-App-Context` meeting UUID via AES-GCM with `ZOOM_CLIENT_SECRET`, or a signed URL token for browser/iPad links).
  - Broadcast chat messages, participant presence, and live telestration strokes (`draw`, `undo`, `clear`).
- **Zoom Apps SDK & OWASP Headers:**
  - Initialize `zoomSdk.config()` with capabilities: `getRunningContext`, `getUserContext`, `startCollaborate`, `endCollaborate`, `appPopout`, `openUrl`, `onCollaborateChange`.
  - Inject mandatory Zoom OWASP response headers in SvelteKit `hooks.server.ts`: `Strict-Transport-Security`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: strict-origin-when-cross-origin`, and a strict `Content-Security-Policy` whitelisting the Railway MediaMTX domain in `media-src` and `connect-src`.
