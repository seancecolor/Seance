import { WebSocketServer } from 'ws';
import { isAuthEnforced, verifyRequestSession } from './session-auth.js';

/**
 * @typedef {import('node:events').EventEmitter} EventEmitter
 * @typedef {import('node:http').IncomingMessage} IncomingMessage
 * @typedef {import('node:stream').Duplex} Duplex
 * @typedef {import('ws').WebSocket & { isAlive?: boolean }} WsSocket
 * @typedef {import('./session-auth.js').SessionPayload} SessionPayload
 * @typedef {{ clients: Map<WsSocket, any>, messages: any[], strokes: any[] }} RoomState
 */

const HEARTBEAT_INTERVAL_MS = 25_000;

/**
 * Shared WebSocket room server for chat, presence, and normalized (0.0..1.0)
 * telestration strokes (including live stroke previews). Used by both
 * `vite.config.ts` (dev) and `server.js` (prod).
 * Enforces signed `seance_session` cookie on upgrade and pins each socket to its
 * authorized room.
 * @param {EventEmitter & { __seanceWssAttached?: boolean }} httpServer
 */
export function attachSeanceWebSocketServer(httpServer) {
	if (httpServer.__seanceWssAttached) return;
	httpServer.__seanceWssAttached = true;

	const wss = new WebSocketServer({ noServer: true, maxPayload: 256 * 1024 });

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

	/** @param {any} rawStroke */
	function sanitizeStroke(rawStroke) {
		if (!rawStroke || !Array.isArray(rawStroke.points) || rawStroke.points.length === 0) {
			return null;
		}
		const points = rawStroke.points
			.slice(0, 2000)
			.filter(
				(/** @type {any} */ p) =>
					p &&
					typeof p.x === 'number' &&
					typeof p.y === 'number' &&
					Number.isFinite(p.x) &&
					Number.isFinite(p.y)
			)
			.map((/** @type {any} */ p) => ({
				x: Math.min(1, Math.max(0, p.x)),
				y: Math.min(1, Math.max(0, p.y))
			}));
		if (points.length === 0) return null;

		const color =
			typeof rawStroke.color === 'string' && /^#[0-9a-fA-F]{3,8}$/.test(rawStroke.color)
				? rawStroke.color
				: '#ff3b30';
		const width =
			typeof rawStroke.width === 'number' && Number.isFinite(rawStroke.width)
				? Math.min(0.05, Math.max(0.001, rawStroke.width))
				: 0.0045;

		return {
			id: String(rawStroke.id || crypto.randomUUID()).slice(0, 64),
			color,
			width,
			points,
			frameTime:
				typeof rawStroke.frameTime === 'number' && Number.isFinite(rawStroke.frameTime)
					? rawStroke.frameTime
					: undefined
		};
	}

	// 25s ping/pong heartbeat keeps connections alive through Railway's edge proxy
	// and prunes dead/ghost participants cleanly.
	const heartbeat = setInterval(() => {
		for (const client of wss.clients) {
			const ws = /** @type {WsSocket} */ (client);
			if (ws.isAlive === false) {
				ws.terminate();
				continue;
			}
			ws.isAlive = false;
			try {
				ws.ping();
			} catch {
				ws.terminate();
			}
		}
	}, HEARTBEAT_INTERVAL_MS);
	heartbeat.unref();

	httpServer.on('close', () => {
		clearInterval(heartbeat);
	});

	httpServer.on(
		'upgrade',
		(/** @type {IncomingMessage} */ req, /** @type {Duplex} */ socket, /** @type {Buffer} */ head) => {
			const url = new URL(req.url || '/', 'http://localhost');
			if (url.pathname !== '/ws') return;

			/** @type {SessionPayload | null} */
			let session = null;
			if (isAuthEnforced()) {
				session = verifyRequestSession(req.headers.cookie);
				if (!session) {
					socket.write('HTTP/1.1 401 Unauthorized\r\nConnection: close\r\n\r\n');
					socket.destroy();
					return;
				}
			}

			wss.handleUpgrade(req, socket, head, (ws) => {
				wss.emit('connection', ws, req, session);
			});
		}
	);

	wss.on(
		'connection',
		(
			/** @type {WsSocket} */ ws,
			/** @type {IncomingMessage} */ _req,
			/** @type {SessionPayload | null} */ session
		) => {
			ws.isAlive = true;
			ws.on('pong', () => {
				ws.isAlive = true;
			});

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
					const roomId = session
						? session.roomId
						: String(msg.roomId || 'seance-studio').slice(0, 128);

					if (currentRoomId && currentRoomId !== roomId) {
						const prev = rooms.get(currentRoomId);
						if (prev) {
							prev.clients.delete(ws);
							broadcast(prev, { type: 'presence', participants: listParticipants(prev) });
						}
					}

					currentRoomId = roomId;
					currentUser = {
						userId: String(msg.userId || crypto.randomUUID()).slice(0, 64),
						userName: String(msg.userName || 'Guest').trim().slice(0, 64) || 'Guest',
						role: session ? session.role : String(msg.role || 'attendee').slice(0, 32),
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

				if (msg.type === 'rename') {
					const nextName = String(msg.userName || '').trim().slice(0, 64);
					if (!nextName || nextName === currentUser.userName) return;
					currentUser = { ...currentUser, userName: nextName };
					room.clients.set(ws, currentUser);
					broadcast(room, { type: 'presence', participants: listParticipants(room) });
				} else if (msg.type === 'chat') {
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
				} else if (msg.type === 'stroke_progress' && msg.stroke) {
					const clean = sanitizeStroke(msg.stroke);
					if (!clean) return;
					const stroke = {
						...clean,
						userId: currentUser.userId,
						userName: currentUser.userName,
						timestamp: Date.now()
					};
					broadcast(room, { type: 'stroke_progress', stroke }, ws);
				} else if (msg.type === 'stroke_add' && msg.stroke) {
					const clean = sanitizeStroke(msg.stroke);
					if (!clean) return;
					const stroke = {
						...clean,
						userId: currentUser.userId,
						userName: currentUser.userName,
						timestamp: Date.now()
					};
					room.strokes.push(stroke);
					if (room.strokes.length > 1000) room.strokes.shift();
					broadcast(room, { type: 'stroke_add', stroke }, ws);
				} else if (msg.type === 'stroke_undo') {
					let removedId = typeof msg.strokeId === 'string' ? msg.strokeId : undefined;
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
						if (currentUser) {
							broadcast(room, { type: 'stroke_progress_end', userId: currentUser.userId });
						}
						broadcast(room, { type: 'presence', participants: listParticipants(room) });
						if (room.clients.size === 0 && room.messages.length === 0 && room.strokes.length === 0) {
							rooms.delete(currentRoomId);
						}
					}
				}
			});
		}
	);
}
