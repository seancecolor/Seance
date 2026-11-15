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
	import type { PageData } from './$types';

	let { data }: { data: PageData } = $props();

	let zoom = $state<ZoomBridgeState>({
		inZoom: false,
		runningContext: 'browser',
		user: null,
		collaborating: false
	});

	let userId = $state<string>(crypto.randomUUID());
	let userName = $state('Colorist / Client');
	let wsConnected = $state(false);
	let participants = $state<Participant[]>([]);
	let messages = $state<ChatMessage[]>([]);
	let strokes = $state<DrawingStroke[]>([]);
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
		if (destroyed) return;
		const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
		const socket = new WebSocket(`${proto}//${location.host}/ws`);
		ws = socket;

		socket.onopen = () => {
			wsConnected = true;
			sendWs({
				type: 'join',
				roomId: data.roomId,
				userId,
				userName,
				role: zoom.user?.role || 'attendee',
				inZoom: zoom.inZoom
			});
		};

		socket.onmessage = (event) => {
			const msg = JSON.parse(event.data) as ServerMessage;
			if (msg.type === 'room_state') {
				participants = msg.participants;
				messages = msg.messages;
				strokes = msg.strokes;
			} else if (msg.type === 'presence') {
				participants = msg.participants;
			} else if (msg.type === 'chat') {
				messages = [...messages, msg.message];
			} else if (msg.type === 'stroke_add') {
				strokes = [...strokes, msg.stroke];
			} else if (msg.type === 'stroke_undo') {
				if (msg.strokeId) {
					strokes = strokes.filter((s) => s.id !== msg.strokeId);
				}
			} else if (msg.type === 'stroke_clear') {
				strokes = [];
			}
		};

		socket.onclose = () => {
			wsConnected = false;
			if (!destroyed) {
				reconnectTimer = setTimeout(connectWebSocket, 1500);
			}
		};
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
		sendWs({ type: 'stroke_clear' });
	}

	function handleSendChat(text: string) {
		sendWs({ type: 'chat', text });
	}

	async function copyShareLink() {
		await navigator.clipboard.writeText(data.shareUrl);
		copied = true;
		setTimeout(() => (copied = false), 2000);
	}

	onMount(() => {
		void (async () => {
			const bridge = await initZoomBridge((collaborating) => {
				zoom.collaborating = collaborating;
			});
			zoom = bridge;
			if (bridge.user) {
				userId = bridge.user.participantUUID;
				userName = bridge.user.screenName;
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

<div class="app-layout">
	<header class="top-bar">
		<div class="brand">
			<strong>SEANCE v2</strong>
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
		</div>
	</header>

	<main class="workspace">
		<section class="video-pane">
			<VideoPlayer
				hlsUrl={data.hlsUrl}
				{strokes}
				{userId}
				{userName}
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
				onSendChat={handleSendChat}
			/>
		</section>
	</main>
</div>

<style>
	:global(html, body) {
		margin: 0;
		padding: 0;
		height: 100%;
		background: #0b0d10;
		color: #e2e8f0;
		font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
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
