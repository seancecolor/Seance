import { WebSocketServer } from 'ws';

/**
 * @typedef {import('node:events').EventEmitter} EventEmitter
 * @typedef {import('node:http').IncomingMessage} IncomingMessage
 * @typedef {import('node:stream').Duplex} Duplex
 * @typedef {import('ws').WebSocket} WsSocket
 * @typedef {{ clients: Map<WsSocket, any>, messages: any[], strokes: any[] }} RoomState
 */

/**
 * Shared WebSocket room server for chat, presence, and normalized (0.0..1.0)
 * telestration strokes. Used by both `vite.config.ts` (dev) and `server.js` (prod).
 * @param {EventEmitter & { __seanceWssAttached?: boolean }} httpServer
 */
export function attachSeanceWebSocketServer(httpServer) {
	if (httpServer.__seanceWssAttached) return;
	httpServer.__seanceWssAttached = true;

	const wss = new WebSocketServer({ noServer: true });

	/** @type {Map<string, RoomState>} */
	const rooms = new Map();

	/** @param {string} roomId */
	function getRoom(roomId) {
		let room = rooms.get(roomId);
		if (!room) {
			room = { clients: new Map(), messages: [], strokes: [] };
			rooms.set(roomId, room);
		}
		return room;
	}

	/**
	 * @param {RoomState} room
	 * @param {any} payload
	 * @param {WsSocket | null} [excludeWs]
	 */
	function broadcast(room, payload, excludeWs = null) {
		const data = JSON.stringify(payload);
		for (const clientWs of room.clients.keys()) {
			if (clientWs !== excludeWs && clientWs.readyState === 1) {
				clientWs.send(data);
			}
		}
	}

	/** @param {RoomState} room */
	function listParticipants(room) {
		return Array.from(room.clients.values());
	}

	httpServer.on(
		'upgrade',
		(/** @type {IncomingMessage} */ req, /** @type {Duplex} */ socket, /** @type {Buffer} */ head) => {
			const url = new URL(req.url || '/', 'http://localhost');
			if (url.pathname !== '/ws') return;

			wss.handleUpgrade(req, socket, head, (ws) => {
				wss.emit('connection', ws, req);
			});
		}
	);

	wss.on('connection', (/** @type {WsSocket} */ ws) => {
		/** @type {string | null} */
		let currentRoomId = null;
		/** @type {any} */
		let currentUser = null;

		ws.on('message', (raw) => {
			let msg;
			try {
				msg = JSON.parse(raw.toString('utf8'));
			} catch {
				ws.send(JSON.stringify({ type: 'error', message: 'Invalid JSON payload' }));
				return;
			}

			if (msg.type === 'join') {
				const roomId = String(msg.roomId || 'default').slice(0, 128);
				if (currentRoomId) {
					const prev = rooms.get(currentRoomId);
					if (prev) {
						prev.clients.delete(ws);
						broadcast(prev, { type: 'presence', participants: listParticipants(prev) });
					}
				}

				currentRoomId = roomId;
				currentUser = {
					userId: String(msg.userId || crypto.randomUUID()),
					userName: String(msg.userName || 'Guest').slice(0, 64),
					role: String(msg.role || 'attendee'),
					inZoom: Boolean(msg.inZoom)
				};

				const room = getRoom(roomId);
				room.clients.set(ws, currentUser);

				ws.send(
					JSON.stringify({
						type: 'room_state',
						roomId,
						participants: listParticipants(room),
						messages: room.messages.slice(-200),
						strokes: room.strokes
					})
				);
				broadcast(room, { type: 'presence', participants: listParticipants(room) }, ws);
				return;
			}

			if (!currentRoomId || !currentUser) {
				ws.send(JSON.stringify({ type: 'error', message: 'Join a room first' }));
				return;
			}

			const room = getRoom(currentRoomId);

			if (msg.type === 'chat') {
				const text = String(msg.text || '').trim().slice(0, 2000);
				if (!text) return;
				const chatMessage = {
					id: crypto.randomUUID(),
					userId: currentUser.userId,
					userName: currentUser.userName,
					text,
					timestamp: Date.now()
				};
				room.messages.push(chatMessage);
				if (room.messages.length > 500) room.messages.shift();
				broadcast(room, { type: 'chat', message: chatMessage });
			} else if (msg.type === 'stroke_add' && msg.stroke) {
				const stroke = {
					...msg.stroke,
					id: String(msg.stroke.id || crypto.randomUUID()),
					userId: currentUser.userId,
					userName: currentUser.userName,
					timestamp: Date.now()
				};
				room.strokes.push(stroke);
				if (room.strokes.length > 1000) room.strokes.shift();
				broadcast(room, { type: 'stroke_add', stroke }, ws);
			} else if (msg.type === 'stroke_undo') {
				let removedId = msg.strokeId;
				if (removedId) {
					room.strokes = room.strokes.filter((s) => s.id !== removedId);
				} else {
					for (let i = room.strokes.length - 1; i >= 0; i--) {
						if (room.strokes[i].userId === currentUser.userId) {
							removedId = room.strokes[i].id;
							room.strokes.splice(i, 1);
							break;
						}
					}
				}
				broadcast(room, {
					type: 'stroke_undo',
					strokeId: removedId,
					userId: currentUser.userId
				});
			} else if (msg.type === 'stroke_clear') {
				room.strokes = [];
				broadcast(room, { type: 'stroke_clear', userId: currentUser.userId });
			}
		});

		ws.on('close', () => {
			if (currentRoomId) {
				const room = rooms.get(currentRoomId);
				if (room) {
					room.clients.delete(ws);
					broadcast(room, { type: 'presence', participants: listParticipants(room) });
				}
			}
		});
	});
}
