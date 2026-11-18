FROM node:24-alpine AS builder
WORKDIR /app

RUN corepack enable

COPY apps/web/package.json apps/web/pnpm-lock.yaml* apps/web/pnpm-workspace.yaml* ./
RUN pnpm install --frozen-lockfile || pnpm install

COPY apps/web/ ./
RUN pnpm build

FROM node:24-alpine
WORKDIR /app
ENV NODE_ENV=production

COPY --from=builder /app/package.json ./
COPY --from=builder /app/node_modules ./node_modules
COPY --from=builder /app/build ./build
COPY --from=builder /app/server.js ./
COPY --from=builder /app/ws-server.js ./
COPY --from=builder /app/hls-proxy.js ./

EXPOSE 3000
CMD ["node", "server.js"]
