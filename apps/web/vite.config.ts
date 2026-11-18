import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import { handleHlsProxy } from './hls-proxy.js';
import { attachSeanceWebSocketServer } from './ws-server.js';

export default defineConfig({
	plugins: [
		sveltekit(),
		{
			name: 'seance-ws-and-hls-dev',
			configureServer(server) {
				server.middlewares.use((req, res, next) => {
					if (handleHlsProxy(req, res)) return;
					next();
				});
				if (server.httpServer) {
					attachSeanceWebSocketServer(server.httpServer);
				}
			}
		}
	]
});
