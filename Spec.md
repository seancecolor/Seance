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