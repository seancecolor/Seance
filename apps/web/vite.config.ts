import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import { attachSeanceWebSocketServer } from './ws-server.js';

export default defineConfig({
	plugins: [
		sveltekit(),
		{
			name: 'seance-ws-dev',
			configureServer(server) {
				if (server.httpServer) {
					attachSeanceWebSocketServer(server.httpServer);
				}
			}
		}
	]
});
