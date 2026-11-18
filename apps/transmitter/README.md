# Seance — Local Linux Transmitter (`apps/transmitter`)

Native Rust (`eframe`/`egui`) + GStreamer (`0.25.4`) desktop control panel for capturing 10-bit SDI from a Blackmagic DeckLink Mini Recorder 4K, encoding `P010_10LE` `bt709` HEVC `main-10` via `nvh265enc`, and pushing via interleaved **RTSP-over-TCP** to MediaMTX on Railway.

## 1. Install System Headers (Fedora / Nobara)
```bash
sudo dnf install gstreamer1-devel gstreamer1-plugins-base-devel \
  gstreamer1-plugins-bad-free-devel gstreamer1-plugins-good \
  gstreamer1-plugins-bad-free gstreamer1-rtsp-server-devel
```

## 2. Build & Test Without DeckLink Hardware
```bash
cd apps/transmitter
cargo run -- --test-pattern --rtsp-url "rtsp://seance_ingest:secret@127.0.0.1:8554/cleanfeed"
```

## 3. Stream Deck / OpenDeck Webhook
While running, the transmitter exposes a local HTTP control endpoint on `127.0.0.1:9944`:
- `http://127.0.0.1:9944/toggle` — toggle stream ON/OFF (sends clean EOS before releasing DeckLink)
- `http://127.0.0.1:9944/slate` — toggle Privacy Slate ON/OFF without dropping the RTSP connection
- `http://127.0.0.1:9944/status` — JSON telemetry snapshot
