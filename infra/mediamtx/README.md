# Seance — MediaMTX on Railway

Zero-transcode media router (`bluenviron/mediamtx:1.21.3`) accepting 10-bit HEVC + AAC over **RTSP-over-TCP** (`:8554`) and serving **Low-Latency HLS (`fMP4`)** over HTTP (`:8888`).

## Railway Setup
1. Create a service pointing to this repo with Root Directory `/` and Config-as-code `infra/mediamtx/railway.toml` (or Dockerfile `infra/mediamtx/Dockerfile`).
2. Add Environment Variables from `.env.example` (set a strong `MTX_AUTHINTERNALUSERS_0_PASS`).
3. In **Settings → Networking**:
   - **Public HTTP Domain**: Target port `8888` (e.g. `stream.visitantmedia.com`). Clients read `https://stream.visitantmedia.com/cleanfeed/index.m3u8`.
   - **TCP Proxy**: Target port `8554` (e.g. `monorail.proxy.rlwy.net:24910`). Your workstation pushes to `rtsp://seance_ingest:<PASS>@monorail.proxy.rlwy.net:24910/cleanfeed`.
