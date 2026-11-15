# Seance v2 — Web Player, WebSocket Server & Zoom App (`apps/web`)

SvelteKit (Svelte 5) frontend + Node WebSocket server (`/ws`) + `hls.js` (`^1.7.4` full build) + `@zoom/appssdk` (`^0.16.43`).

## Local Development
```bash
cd apps/web
cp .env.example .env
pnpm install
pnpm dev
```

## Checks & Build
```bash
pnpm check
pnpm test
pnpm build
pnpm start
```

## Railway Deployment
- Config-as-code: `apps/web/railway.toml` (Dockerfile `apps/web/Dockerfile`).
- Set `PUBLIC_MEDIAMTX_HLS_ORIGIN`, `PUBLIC_MEDIAMTX_STREAM_PATH`, `ZOOM_CLIENT_SECRET`, and `ROOM_SIGNING_SECRET` in Railway Variables.
