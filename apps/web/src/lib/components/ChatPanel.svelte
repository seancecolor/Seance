<script lang="ts">
	import type { ChatMessage, Participant } from '$lib/shared/protocol';

	interface Props {
		participants: Participant[];
		messages: ChatMessage[];
		connected: boolean;
		onSendChat: (text: string) => void;
	}

	let { participants, messages, connected, onSendChat }: Props = $props();
	let draft = $state('');

	function submit(e: SubmitEvent) {
		e.preventDefault();
		const text = draft.trim();
		if (!text) return;
		onSendChat(text);
		draft = '';
	}
</script>

<aside class="chat-panel">
	<div class="panel-header">
		<span class="title">Room Session</span>
		<span class="ws-badge" class:ws-ok={connected}>
			{connected ? 'WS Connected' : 'Reconnecting...'}
		</span>
	</div>

	<div class="participants">
		{#each participants as p (p.userId)}
			<span class="participant-chip">
				{p.userName}
				<small>{p.inZoom ? 'Zoom' : 'Web'}</small>
			</span>
		{/each}
	</div>

	<div class="messages">
		{#if messages.length === 0}
			<div class="empty">No notes yet. Draw on the frame or send a message below.</div>
		{/if}
		{#each messages as msg (msg.id)}
			<div class="msg">
				<div class="msg-meta">
					<strong>{msg.userName}</strong>
					<span>{new Date(msg.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</span>
				</div>
				<div class="msg-text">{msg.text}</div>
			</div>
		{/each}
	</div>

	<form class="composer" onsubmit={submit}>
		<input
			type="text"
			bind:value={draft}
			placeholder="Add grading note..."
			disabled={!connected}
		/>
		<button type="submit" disabled={!connected || !draft.trim()}>Send</button>
	</form>
</aside>

<style>
	.chat-panel {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
		background: #101318;
		border: 1px solid #1e232d;
		border-radius: 8px;
		overflow: hidden;
	}

	.panel-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0.65rem 0.85rem;
		border-bottom: 1px solid #1e232d;
		background: #141820;
	}

	.title {
		font-size: 0.85rem;
		font-weight: 600;
		color: #e2e8f0;
	}

	.ws-badge {
		font-size: 0.7rem;
		padding: 0.15rem 0.45rem;
		border-radius: 4px;
		background: #3f1d1d;
		color: #fca5a5;
	}

	.ws-badge.ws-ok {
		background: #133524;
		color: #6ee7b7;
	}

	.participants {
		display: flex;
		flex-wrap: wrap;
		gap: 0.35rem;
		padding: 0.5rem 0.75rem;
		border-bottom: 1px solid #1e232d;
	}

	.participant-chip {
		font-size: 0.72rem;
		padding: 0.15rem 0.45rem;
		border-radius: 999px;
		background: #1b202a;
		color: #cbd5e1;
	}

	.participant-chip small {
		margin-left: 0.25rem;
		color: #64748b;
	}

	.messages {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		padding: 0.75rem;
		display: flex;
		flex-direction: column;
		gap: 0.6rem;
	}

	.empty {
		color: #64748b;
		font-size: 0.8rem;
		text-align: center;
		margin-top: 2rem;
	}

	.msg {
		background: #161b23;
		border: 1px solid #202734;
		border-radius: 6px;
		padding: 0.45rem 0.65rem;
	}

	.msg-meta {
		display: flex;
		justify-content: space-between;
		font-size: 0.72rem;
		color: #94a3b8;
		margin-bottom: 0.2rem;
	}

	.msg-text {
		font-size: 0.84rem;
		color: #f1f5f9;
		white-space: pre-wrap;
		word-break: break-word;
	}

	.composer {
		display: flex;
		gap: 0.45rem;
		padding: 0.6rem;
		border-top: 1px solid #1e232d;
		background: #141820;
	}

	.composer input {
		flex: 1;
		background: #0b0d10;
		color: #f1f5f9;
		border: 1px solid #262e3d;
		border-radius: 5px;
		padding: 0.45rem 0.6rem;
		font-size: 0.84rem;
	}

	.composer button {
		background: #2563eb;
		color: #fff;
		border: none;
		border-radius: 5px;
		padding: 0.45rem 0.85rem;
		font-size: 0.82rem;
		cursor: pointer;
	}

	.composer button:disabled {
		opacity: 0.45;
		cursor: not-allowed;
	}
</style>
