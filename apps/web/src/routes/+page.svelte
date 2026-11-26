<script lang="ts">
	import ChatPanel from '$lib/components/ChatPanel.svelte';
	import VideoPlayer from '$lib/components/VideoPlayer.svelte';
	import type {
		ChatMessage,
		ClientMessage,
		DrawingStroke,
		Participant,
		ServerMessage
	} from '$lib/shared/protocol';
	import {
		initZoomBridge,
		popoutZoomApp,
		startZoomCollaborate,
		endZoomCollaborate,
		type ZoomBridgeState
	} from '$lib/zoomBridge';
	import { onMount } from 'svelte';
	import type { ActionData, PageData } from './$types';

	let { data, form }: { data: PageData; form: ActionData } = $props();

	const NAME_STORAGE_KEY = 'seance_user_name';

	let zoom = $state<ZoomBridgeState>({
		inZoom: false,
		runningContext: 'browser',
		user: null,
		collaborating: false
	});

	let userId = $state<string>(crypto.randomUUID());
	let userName = $state('Colorist');
	let wsConnected = $state(false);
	let participants = $state<Participant[]>([]);
	let messages = $state<ChatMessage[]>([]);
	let strokes = $state<DrawingStroke[]>([]);
	let remotePreviews = $state<Record<string, DrawingStroke>>({});
	let copied = $state(false);

	let ws: WebSocket | null = null;
	let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
	let destroyed = false;

	function sendWs(msg: ClientMessage) {
		if (ws && ws.readyState === WebSocket.OPEN) {
			ws.send(JSON.stringify(msg));
		}
	}

	function connectWebSocket() {
		if (destroyed || !data.authorized || !data.roomId) return;
		const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
		const socket = new WebSocket(`${proto}//${location.host}/ws`);
		ws = socket;

		socket.onopen = () => {
			wsConnected = true;
			sendWs({
				type: 'join',
				roomId: data.roomId!,
				userId,
				userName,
				role: zoom.user?.role || data.role || 'attendee',
				inZoom: zoom.inZoom
			});
		};

		socket.onmessage = (event) => {
			const msg = JSON.parse(event.data) as ServerMessage;
			if (msg.type === 'room_state') {
				participants = msg.participants;
				messages = msg.messages;
				strokes = msg.strokes;
				remotePreviews = {};
			} else if (msg.type === 'presence') {
				participants = msg.participants;
			} else if (msg.type === 'chat') {
				messages = [...messages, msg.message];
			} else if (msg.type === 'stroke_progress') {
				remotePreviews = { ...remotePreviews, [msg.stroke.userId]: msg.stroke };
			} else if (msg.type === 'stroke_progress_end') {
				const next = { ...remotePreviews };
				delete next[msg.userId];
				remotePreviews = next;
			} else if (msg.type === 'stroke_add') {
				const next = { ...remotePreviews };
				delete next[msg.stroke.userId];
				remotePreviews = next;
				strokes = [...strokes, msg.stroke];
			} else if (msg.type === 'stroke_undo') {
				if (msg.strokeId) {
					strokes = strokes.filter((s) => s.id !== msg.strokeId);
				}
			} else if (msg.type === 'stroke_clear') {
				strokes = [];
				remotePreviews = {};
			}
		};

		socket.onclose = () => {
			wsConnected = false;
			if (!destroyed) {
				reconnectTimer = setTimeout(connectWebSocket, 1500);
			}
		};
	}

	function handleStrokeProgress(stroke: DrawingStroke) {
		sendWs({ type: 'stroke_progress', stroke });
	}

	function handleAddStroke(stroke: DrawingStroke) {
		strokes = [...strokes, stroke];
		sendWs({ type: 'stroke_add', stroke });
	}

	function handleUndoStroke() {
		sendWs({ type: 'stroke_undo' });
	}

	function handleClearStrokes() {
		strokes = [];
		remotePreviews = {};
		sendWs({ type: 'stroke_clear' });
	}

	function handleRename(nextName: string) {
		userName = nextName;
		try {
			localStorage.setItem(NAME_STORAGE_KEY, nextName);
		} catch {
			// Ignore storage errors in private browsing
		}
		sendWs({ type: 'rename', userName: nextName });
	}

	function handleSendChat(text: string) {
		sendWs({ type: 'chat', text });
	}

	async function copyShareLink() {
		if (!data.shareUrl) return;
		await navigator.clipboard.writeText(data.shareUrl);
		copied = true;
		setTimeout(() => (copied = false), 2000);
	}

	onMount(() => {
		if (!data.authorized) return;

		void (async () => {
			const bridge = await initZoomBridge((collaborating) => {
				zoom.collaborating = collaborating;
			});
			zoom = bridge;
			if (bridge.user) {
				userId = bridge.user.participantUUID;
				userName = bridge.user.screenName;
			} else {
				const fallback = data.role === 'host' ? 'Colorist' : 'Client';
				try {
					userName = localStorage.getItem(NAME_STORAGE_KEY)?.trim() || fallback;
				} catch {
					userName = fallback;
				}
			}
			connectWebSocket();
		})();

		return () => {
			destroyed = true;
			if (reconnectTimer) clearTimeout(reconnectTimer);
			ws?.close();
		};
	});
</script>

{#if !data.authorized}
	<div class="lock-shell">
		<div class="lock-card">
			<div class="lock-badge">PRIVATE GRADING SUITE</div>
			<h1>SEANCE</h1>
			<p class="lock-copy">
				This session is protected. Clients should open the signed reference link shared in chat or
				join via Zoom Collaborate.
			</p>

			<form method="POST" action="?/unlock" class="unlock-form">
				<label class="field">
					<span>Session Room</span>
					<input
						type="text"
						name="room"
						value={form?.room || 'seance-studio'}
						placeholder="seance-studio"
						autocomplete="off"
					/>
				</label>

				<label class="field">
					<span>Host Passcode</span>
					<input
						type="password"
						name="passcode"
						placeholder="Enter host passcode"
						required
						autocomplete="current-password"
					/>
				</label>

				{#if form?.error}
					<div class="lock-error">{form.error}</div>
				{/if}

				<button type="submit" class="unlock-btn">Unlock Session</button>
			</form>
		</div>
	</div>
{:else}
	<div class="app-layout">
		<header class="top-bar">
			<div class="brand">
				<strong>SEANCE</strong>
				<span class="room-pill">Room: {data.roomId}</span>
				<span class="mode-pill">{zoom.inZoom ? 'Zoom App' : 'Standalone Browser'}</span>
			</div>

			<div class="top-actions">
				{#if zoom.inZoom}
					<button
						type="button"
						class="action-btn"
						onclick={() => (zoom.collaborating ? endZoomCollaborate() : startZoomCollaborate())}
					>
						{zoom.collaborating ? 'End Collaborate' : 'Collaborate'}
					</button>
					<button type="button" class="action-btn" onclick={() => popoutZoomApp()}>
						Pop Out 1080p
					</button>
				{/if}
				<button type="button" class="action-btn" onclick={copyShareLink}>
					{copied ? 'Copied Reference Link!' : 'Copy Reference Link'}
				</button>
				{#if data.role === 'host' && !zoom.inZoom}
					<form method="POST" action="?/lock" class="inline-form">
						<button type="submit" class="action-btn action-btn-muted">Lock</button>
					</form>
				{/if}
			</div>
		</header>

		<main class="workspace">
			<section class="video-pane">
				<VideoPlayer
					hlsUrl={data.hlsUrl!}
					{strokes}
					{remotePreviews}
					{userId}
					{userName}
					onStrokeProgress={handleStrokeProgress}
					onAddStroke={handleAddStroke}
					onUndoStroke={handleUndoStroke}
					onClearStrokes={handleClearStrokes}
				/>
			</section>

			<section class="side-pane">
				<ChatPanel
					{participants}
					{messages}
					connected={wsConnected}
					{userName}
					canEditName={!zoom.inZoom}
					onRename={handleRename}
					onSendChat={handleSendChat}
				/>
			</section>
		</main>
	</div>
{/if}

<style>
	:global(html, body) {
		margin: 0;
		padding: 0;
		height: 100%;
		background: #0b0d10;
		color: #e2e8f0;
		font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
	}

	.lock-shell {
		min-height: 100vh;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 1.5rem;
		box-sizing: border-box;
	}

	.lock-card {
		width: 100%;
		max-width: 390px;
		background: #11141a;
		border: 1px solid #1e232d;
		border-radius: 12px;
		padding: 1.75rem;
		box-shadow: 0 20px 50px rgba(0, 0, 0, 0.5);
	}

	.lock-badge {
		display: inline-block;
		font-size: 0.68rem;
		letter-spacing: 0.08em;
		padding: 0.2rem 0.5rem;
		border-radius: 4px;
		background: #1b202a;
		color: #94a3b8;
		margin-bottom: 0.65rem;
	}

	.lock-card h1 {
		margin: 0 0 0.5rem 0;
		font-size: 1.45rem;
		letter-spacing: 0.06em;
	}

	.lock-copy {
		margin: 0 0 1.25rem 0;
		font-size: 0.85rem;
		line-height: 1.45;
		color: #94a3b8;
	}

	.unlock-form {
		display: flex;
		flex-direction: column;
		gap: 0.85rem;
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: 0.35rem;
		font-size: 0.78rem;
		color: #cbd5e1;
	}

	.field input {
		background: #0b0d10;
		color: #f8fafc;
		border: 1px solid #262e3d;
		border-radius: 6px;
		padding: 0.55rem 0.7rem;
		font-size: 0.88rem;
	}

	.lock-error {
		font-size: 0.8rem;
		color: #fca5a5;
		background: #3f1d1d;
		border: 1px solid #7f1d1d;
		border-radius: 6px;
		padding: 0.45rem 0.65rem;
	}

	.unlock-btn {
		background: #2563eb;
		color: #fff;
		border: none;
		border-radius: 6px;
		padding: 0.6rem 0.9rem;
		font-size: 0.88rem;
		font-weight: 600;
		cursor: pointer;
	}

	.unlock-btn:hover {
		background: #1d4ed8;
	}

	.inline-form {
		margin: 0;
	}

	.app-layout {
		display: flex;
		flex-direction: column;
		height: 100vh;
		padding: 0.65rem;
		box-sizing: border-box;
		gap: 0.65rem;
	}

	.top-bar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 0.75rem;
		padding: 0.5rem 0.85rem;
		background: #11141a;
		border: 1px solid #1e232d;
		border-radius: 8px;
	}

	.brand {
		display: flex;
		align-items: center;
		gap: 0.65rem;
		font-size: 0.9rem;
	}

	.room-pill,
	.mode-pill {
		font-size: 0.72rem;
		padding: 0.15rem 0.5rem;
		border-radius: 4px;
		background: #1b202a;
		color: #94a3b8;
	}

	.top-actions {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.action-btn {
		background: #1d222c;
		color: #e2e8f0;
		border: 1px solid #2e3646;
		border-radius: 5px;
		padding: 0.35rem 0.7rem;
		font-size: 0.78rem;
		cursor: pointer;
	}

	.action-btn:hover {
		background: #28303e;
	}

	.action-btn-muted {
		color: #94a3b8;
	}

	.workspace {
		flex: 1;
		min-height: 0;
		display: grid;
		grid-template-columns: 1fr 320px;
		gap: 0.65rem;
	}

	.video-pane,
	.side-pane {
		min-height: 0;
		height: 100%;
	}

	@media (max-width: 900px) {
		.workspace {
			grid-template-columns: 1fr;
			grid-template-rows: 1fr 260px;
		}
	}
</style>
