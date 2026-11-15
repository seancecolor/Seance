import http from 'node:http';
import { handler } from './build/handler.js';
import { attachSeanceWebSocketServer } from './ws-server.js';

const port = Number(process.env.PORT || 3000);
const host = process.env.HOST || '0.0.0.0';

const server = http.createServer(handler);
attachSeanceWebSocketServer(server);

server.listen(port, host, () => {
	console.log(`[seance-web] listening on http://${host}:${port} (WS at /ws)`);
});
