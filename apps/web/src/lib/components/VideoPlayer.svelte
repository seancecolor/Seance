<script lang="ts">
	import Hls from 'hls.js';
	import { onMount } from 'svelte';
	import type { DrawingStroke, NormalizedPoint } from '$lib/shared/protocol';

	interface Props {
		hlsUrl: string;
		strokes: DrawingStroke[];
		remotePreviews: Record<string, DrawingStroke>;
		userId: string;
		userName: string;
		onStrokeProgress: (stroke: DrawingStroke) => void;
		onAddStroke: (stroke: DrawingStroke) => void;
		onUndoStroke: () => void;
		onClearStrokes: () => void;
	}

	let {
		hlsUrl,
		strokes,
		remotePreviews,
		userId,
		userName,
		onStrokeProgress,
		onAddStroke,
		onUndoStroke,
		onClearStrokes
	}: Props = $props();

	let shellEl: HTMLDivElement | undefined = $state();
	let containerEl: HTMLDivElement | undefined = $state();
	let videoEl: HTMLVideoElement | undefined = $state();
	let canvasEl: HTMLCanvasElement | undefined = $state();

	let hlsInstance: Hls | null = null;
	let playbackMode = $state<'native' | 'hls.js' | 'unsupported'>('hls.js');
	let streamState = $state<'waiting' | 'live' | 'error'>('waiting');
	let statusText = $state('Waiting for signal…');
	let latencySec = $state<number | null>(null);
	let resolutionText = $state<string>('');
	let isFullscreen = $state(false);
	let muted = $state(true);
	let volume = $state(0.8);

	// Drawing state
	let drawEnabled = $state(true);
	let brushColor = $state('#ff3b30');
	let brushWidth = $state(0.0045);
	let isDrawing = $state(false);
	let activeStrokeId = '';
	let activePoints = $state<NormalizedPoint[]>([]);
	let lastProgressSentAt = 0;

	// Exact rendered video content box inside containerEl
	let overlayBox = $state({ left: 0, top: 0, width: 0, height: 0 });

	const COLORS = ['#ff3b30', '#ffcc00', '#34c759', '#00c7be', '#ffffff'];
	const SIZES = [
		{ label: 'S', value: 0.0025 },
		{ label: 'M', value: 0.0045 },
		{ label: 'L', value: 0.008 }
	];

	function updateOverlayRect() {
		if (!containerEl || !videoEl || !canvasEl) return;
		const cw = containerEl.clientWidth;
		const ch = containerEl.clientHeight;
		const vw = videoEl.videoWidth || 1920;
		const vh = videoEl.videoHeight || 1080;

		if (videoEl.videoWidth > 0 && videoEl.videoHeight > 0) {
			resolutionText = `${videoEl.videoWidth}×${videoEl.videoHeight}`;
		}

		const containerAspect = cw / Math.max(1, ch);
		const videoAspect = vw / Math.max(1, vh);

		let width = cw;
		let height = ch;
		let left = 0;
		let top = 0;

		if (containerAspect > videoAspect) {
			height = ch;
			width = ch * videoAspect;
			left = (cw - width) / 2;
		} else {
			width = cw;
			height = cw / videoAspect;
			top = (ch - height) / 2;
		}

		overlayBox = { left, top, width, height };

		const dpr = window.devicePixelRatio || 1;
		canvasEl.width = Math.max(1, Math.round(width * dpr));
		canvasEl.height = Math.max(1, Math.round(height * dpr));
		redrawCanvas();
	}

	function drawStrokePath(
		ctx: CanvasRenderingContext2D,
		points: NormalizedPoint[],
		color: string,
		normWidth: number,
		w: number,
		h: number,
		alpha = 1
	) {
		if (points.length === 0) return;
		ctx.save();
		ctx.globalAlpha = alpha;
		ctx.strokeStyle = color;
		ctx.fillStyle = color;
		ctx.lineWidth = Math.max(2, normWidth * h);
		ctx.lineCap = 'round';
		ctx.lineJoin = 'round';

		if (points.length === 1) {
			ctx.beginPath();
			ctx.arc(points[0].x * w, points[0].y * h, ctx.lineWidth / 2, 0, Math.PI * 2);
			ctx.fill();
		} else {
			ctx.beginPath();
			ctx.moveTo(points[0].x * w, points[0].y * h);
			for (let i = 1; i < points.length; i++) {
				ctx.lineTo(points[i].x * w, points[i].y * h);
			}
			ctx.stroke();
		}
		ctx.restore();
	}

	function redrawCanvas() {
		if (!canvasEl) return;
		const ctx = canvasEl.getContext('2d');
		if (!ctx) return;

		const w = canvasEl.width;
		const h = canvasEl.height;
		ctx.clearRect(0, 0, w, h);

		for (const stroke of strokes) {
			drawStrokePath(ctx, stroke.points, stroke.color, stroke.width, w, h);
		}
		for (const preview of Object.values(remotePreviews)) {
			drawStrokePath(ctx, preview.points, preview.color, preview.width, w, h, 0.85);
		}
		if (activePoints.length > 0) {
			drawStrokePath(ctx, activePoints, brushColor, brushWidth, w, h);
		}
	}

	$effect(() => {
		void strokes;
		void remotePreviews;
		void activePoints;
		redrawCanvas();
	});

	function toNormalizedPoint(e: PointerEvent): NormalizedPoint | null {
		if (!canvasEl) return null;
		const rect = canvasEl.getBoundingClientRect();
		if (rect.width <= 0 || rect.height <= 0) return null;
		return {
			x: Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width)),
			y: Math.min(1, Math.max(0, (e.clientY - rect.top) / rect.height))
		};
	}

	function buildActiveStroke(): DrawingStroke {
		return {
			id: activeStrokeId,
			userId,
			userName,
			color: brushColor,
			width: brushWidth,
			points: activePoints,
			timestamp: Date.now(),
			frameTime: videoEl?.currentTime
		};
	}

	function handlePointerDown(e: PointerEvent) {
		if (!drawEnabled || !canvasEl) return;
		const pt = toNormalizedPoint(e);
		if (!pt) return;
		canvasEl.setPointerCapture(e.pointerId);
		isDrawing = true;
		activeStrokeId = crypto.randomUUID();
		activePoints = [pt];
		lastProgressSentAt = performance.now();
		onStrokeProgress(buildActiveStroke());
	}

	function handlePointerMove(e: PointerEvent) {
		if (!isDrawing) return;
		const pt = toNormalizedPoint(e);
		if (!pt) return;
		activePoints = [...activePoints, pt];

		const now = performance.now();
		if (now - lastProgressSentAt >= 40) {
			lastProgressSentAt = now;
			onStrokeProgress(buildActiveStroke());
		}
	}

	function handlePointerUp(e: PointerEvent) {
		if (!isDrawing || !canvasEl) return;
		if (canvasEl.hasPointerCapture(e.pointerId)) {
			canvasEl.releasePointerCapture(e.pointerId);
		}
		isDrawing = false;
		if (activePoints.length > 0) {
			onAddStroke(buildActiveStroke());
		}
		activePoints = [];
	}

	export function resyncToLiveEdge() {
		if (!videoEl) return;
		if (hlsInstance && typeof hlsInstance.liveSyncPosition === 'number') {
			videoEl.currentTime = hlsInstance.liveSyncPosition;
		} else if (videoEl.seekable.length > 0) {
			videoEl.currentTime = videoEl.seekable.end(videoEl.seekable.length - 1);
		}
		void videoEl.play().catch(() => {});
	}

	function toggleMute() {
		muted = !muted;
		if (videoEl) {
			videoEl.muted = muted;
			if (!muted) videoEl.volume = volume;
		}
	}

	function handleVolumeChange(e: Event) {
		const val = Number((e.currentTarget as HTMLInputElement).value);
		volume = val;
		muted = val === 0;
		if (videoEl) {
			videoEl.volume = val;
			videoEl.muted = muted;
		}
	}

	async function toggleFullscreen() {
		if (!shellEl) return;
		try {
			if (document.fullscreenElement) {
				await document.exitFullscreen();
			} else if (shellEl.requestFullscreen) {
				await shellEl.requestFullscreen();
			} else {
				const webkitEl = shellEl as HTMLDivElement & {
					webkitRequestFullscreen?: () => Promise<void> | void;
				};
				await webkitEl.webkitRequestFullscreen?.();
			}
		} catch {
			// Ignore fullscreen rejection (e.g. inside restricted WebViews)
		}
	}

	onMount(() => {
		if (!videoEl || !containerEl) return;

		let retryTimer: ReturnType<typeof setTimeout> | null = null;
		let destroyed = false;

		const scheduleRetry = (fn: () => void, delayMs = 3000) => {
			if (destroyed || retryTimer) return;
			retryTimer = setTimeout(() => {
				retryTimer = null;
				if (!destroyed) fn();
			}, delayMs);
		};

		const ro = new ResizeObserver(() => updateOverlayRect());
		ro.observe(containerEl);

		const onPlaying = () => {
			streamState = 'live';
			statusText = playbackMode === 'native' ? 'Native 10-Bit LL-HLS' : 'LL-HLS Live';
			updateOverlayRect();
		};
		const onFullscreenChange = () => {
			isFullscreen = Boolean(document.fullscreenElement);
		};

		videoEl.addEventListener('loadedmetadata', updateOverlayRect);
		videoEl.addEventListener('resize', updateOverlayRect);
		videoEl.addEventListener('playing', onPlaying);
		document.addEventListener('fullscreenchange', onFullscreenChange);

		const isAppleNative =
			videoEl.canPlayType('application/vnd.apple.mpegurl') !== '' &&
			/Safari/i.test(navigator.userAgent) &&
			!/Chrome|Chromium/i.test(navigator.userAgent);

		let onNativeError: (() => void) | null = null;

		if (isAppleNative) {
			playbackMode = 'native';
			const loadNative = () => {
				if (!videoEl || destroyed) return;
				streamState = 'waiting';
				statusText = 'Waiting for signal…';
				videoEl.src = hlsUrl;
				videoEl.load();
				void videoEl.play().catch(() => {});
			};
			onNativeError = () => {
				streamState = 'waiting';
				statusText = 'Waiting for signal…';
				scheduleRetry(loadNative, 3000);
			};
			videoEl.addEventListener('error', onNativeError);
			loadNative();
		} else if (Hls.isSupported()) {
			playbackMode = 'hls.js';
			const hls = new Hls({
				lowLatencyMode: true,
				maxLiveSyncPlaybackRate: 1.5,
				backBufferLength: 10
			});
			hlsInstance = hls;

			const loadHls = () => {
				if (destroyed) return;
				hls.loadSource(hlsUrl);
			};

			hls.attachMedia(videoEl);
			loadHls();

			hls.on(Hls.Events.MANIFEST_PARSED, () => {
				streamState = 'live';
				statusText = 'LL-HLS Live';
				void videoEl?.play().catch(() => {});
			});

			hls.on(Hls.Events.FRAG_BUFFERED, () => {
				if (streamState !== 'live') {
					streamState = 'live';
					statusText = 'LL-HLS Live';
				}
			});

			hls.on(Hls.Events.ERROR, (_event, data) => {
				const code = data.response?.code;

				// MediaMTX invalidates its HLS session cookie whenever the publisher restarts,
				// returning 401 on child playlists (video1_stream.m3u8 / audio2_stream.m3u8).
				// Reloading the master playlist obtains a fresh MediaMTX session automatically.
				if (code === 401) {
					streamState = 'waiting';
					statusText = 'Reconnecting stream…';
					scheduleRetry(loadHls, 1500);
					return;
				}

				if (!data.fatal) return;

				if (
					code === 404 ||
					code === 500 ||
					data.details === Hls.ErrorDetails.MANIFEST_LOAD_ERROR ||
					data.details === Hls.ErrorDetails.LEVEL_LOAD_ERROR ||
					data.details === Hls.ErrorDetails.AUDIO_TRACK_LOAD_ERROR
				) {
					streamState = 'waiting';
					statusText = 'Waiting for signal…';
					scheduleRetry(loadHls, 2500);
				} else if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
					streamState = 'waiting';
					statusText = 'Reconnecting stream…';
					scheduleRetry(loadHls, 2000);
				} else if (data.type === Hls.ErrorTypes.MEDIA_ERROR) {
					statusText = 'Recovering media…';
					hls.recoverMediaError();
				} else {
					streamState = 'error';
					statusText = `Stream error (${data.details})`;
					scheduleRetry(loadHls, 3000);
				}
			});
		} else {
			playbackMode = 'unsupported';
			streamState = 'error';
			statusText = 'HLS playback unsupported in this browser';
		}

		const latencyInterval = setInterval(() => {
			if (!videoEl || streamState !== 'live') {
				latencySec = null;
				return;
			}
			if (hlsInstance && typeof hlsInstance.latency === 'number' && hlsInstance.latency > 0) {
				latencySec = hlsInstance.latency;
			} else if (videoEl.seekable.length > 0) {
				const liveEdge = videoEl.seekable.end(videoEl.seekable.length - 1);
				const diff = liveEdge - videoEl.currentTime;
				latencySec = diff >= 0 && diff < 60 ? diff : null;
			}
		}, 500);

		updateOverlayRect();

		return () => {
			destroyed = true;
			if (retryTimer) clearTimeout(retryTimer);
			clearInterval(latencyInterval);
			ro.disconnect();
			videoEl?.removeEventListener('loadedmetadata', updateOverlayRect);
			videoEl?.removeEventListener('resize', updateOverlayRect);
			videoEl?.removeEventListener('playing', onPlaying);
			if (onNativeError) videoEl?.removeEventListener('error', onNativeError);
			document.removeEventListener('fullscreenchange', onFullscreenChange);
			hlsInstance?.destroy();
		};
	});
</script>

<div class="player-shell" bind:this={shellEl}>
	<div class="stage" bind:this={containerEl}>
		<!-- svelte-ignore a11y_media_has_caption -->
		<video
			bind:this={videoEl}
			class="video-el"
			playsinline
			autoplay
			{muted}
		></video>

		{#if streamState === 'waiting'}
			<div class="offline-overlay">
				<div class="offline-badge">
					<span class="pulse-dot"></span>
					<span>Waiting for transmitter signal…</span>
				</div>
				<p class="offline-hint">Start the Seance transmitter on your grading workstation. Playback starts automatically.</p>
			</div>
		{/if}

		<canvas
			bind:this={canvasEl}
			class="telestration-canvas"
			class:drawing-active={drawEnabled}
			style="left: {overlayBox.left}px; top: {overlayBox.top}px; width: {overlayBox.width}px; height: {overlayBox.height}px;"
			onpointerdown={handlePointerDown}
			onpointermove={handlePointerMove}
			onpointerup={handlePointerUp}
			onpointercancel={handlePointerUp}
		></canvas>
	</div>

	<div class="controls-bar">
		<div class="controls-group">
			<span class="status-pill" data-state={streamState}>
				<span class="state-dot" data-state={streamState}></span>
				{statusText}
			</span>
			{#if resolutionText && streamState === 'live'}
				<span class="meta-pill">{resolutionText}</span>
			{/if}
			{#if latencySec !== null}
				<span class="meta-pill">{latencySec.toFixed(1)}s</span>
			{/if}
			<button type="button" class="btn" onclick={resyncToLiveEdge}>
				Re-sync Live
			</button>
		</div>

		<div class="controls-group">
			<button type="button" class="btn" class:btn-active={drawEnabled} onclick={() => (drawEnabled = !drawEnabled)}>
				{drawEnabled ? 'Draw: ON' : 'Draw: OFF'}
			</button>

			{#each COLORS as c}
				<button
					type="button"
					aria-label="Brush color {c}"
					class="swatch"
					class:swatch-selected={brushColor === c}
					style="background-color: {c};"
					onclick={() => (brushColor = c)}
				></button>
			{/each}

			<div class="size-group" role="group" aria-label="Brush size">
				{#each SIZES as sz}
					<button
						type="button"
						class="size-btn"
						class:size-selected={brushWidth === sz.value}
						onclick={() => (brushWidth = sz.value)}
					>
						{sz.label}
					</button>
				{/each}
			</div>

			<button type="button" class="btn" onclick={onUndoStroke}>Undo</button>
			<button type="button" class="btn" onclick={onClearStrokes}>Clear</button>
		</div>

		<div class="controls-group">
			<button type="button" class="btn" onclick={toggleMute}>
				{muted ? 'Unmute' : 'Mute'}
			</button>
			<input
				type="range"
				min="0"
				max="1"
				step="0.05"
				value={muted ? 0 : volume}
				oninput={handleVolumeChange}
				aria-label="Program audio volume"
				class="vol-slider"
			/>
			<button type="button" class="btn" onclick={toggleFullscreen}>
				{isFullscreen ? 'Exit Full' : 'Fullscreen'}
			</button>
		</div>
	</div>
</div>

<style>
	.player-shell {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
		background: #08090c;
		border: 1px solid #1e232d;
		border-radius: 8px;
		overflow: hidden;
	}

	.stage {
		position: relative;
		flex: 1;
		min-height: 0;
		background: #000;
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.video-el {
		width: 100%;
		height: 100%;
		object-fit: contain;
		display: block;
	}

	.offline-overlay {
		position: absolute;
		inset: 0;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 0.65rem;
		background: radial-gradient(circle at center, rgba(17, 20, 26, 0.85), rgba(8, 9, 12, 0.96));
		pointer-events: none;
		padding: 1.5rem;
		text-align: center;
	}

	.offline-badge {
		display: inline-flex;
		align-items: center;
		gap: 0.55rem;
		padding: 0.45rem 0.9rem;
		border-radius: 999px;
		background: #141820;
		border: 1px solid #262e3d;
		font-size: 0.88rem;
		font-weight: 500;
		color: #e2e8f0;
	}

	.offline-hint {
		margin: 0;
		font-size: 0.78rem;
		color: #64748b;
		max-width: 380px;
	}

	.pulse-dot {
		width: 8px;
		height: 8px;
		border-radius: 999px;
		background: #f59e0b;
		animation: pulse 1.6s ease-in-out infinite;
	}

	@keyframes pulse {
		0%,
		100% {
			opacity: 0.35;
			transform: scale(0.9);
		}
		50% {
			opacity: 1;
			transform: scale(1.15);
		}
	}

	.telestration-canvas {
		position: absolute;
		pointer-events: none;
		touch-action: none;
	}

	.telestration-canvas.drawing-active {
		pointer-events: auto;
		cursor: crosshair;
	}

	.controls-bar {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 0.65rem;
		padding: 0.55rem 0.85rem;
		background: #12151b;
		border-top: 1px solid #1e232d;
	}

	.controls-group {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		flex-wrap: wrap;
	}

	.status-pill,
	.meta-pill {
		display: inline-flex;
		align-items: center;
		gap: 0.35rem;
		font-size: 0.73rem;
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		padding: 0.2rem 0.5rem;
		border-radius: 4px;
		background: #1c212b;
		color: #a7b4c8;
	}

	.state-dot {
		width: 7px;
		height: 7px;
		border-radius: 999px;
		background: #f59e0b;
	}

	.state-dot[data-state='live'] {
		background: #22c55e;
	}

	.state-dot[data-state='error'] {
		background: #ef4444;
	}

	.btn {
		background: #1d222c;
		color: #e2e8f0;
		border: 1px solid #2e3646;
		border-radius: 5px;
		padding: 0.3rem 0.6rem;
		font-size: 0.76rem;
		cursor: pointer;
	}

	.btn:hover {
		background: #28303e;
	}

	.btn-active {
		background: #1d4ed8;
		border-color: #3b82f6;
		color: #fff;
	}

	.swatch {
		width: 18px;
		height: 18px;
		border-radius: 999px;
		border: 2px solid transparent;
		cursor: pointer;
	}

	.swatch-selected {
		border-color: #fff;
		transform: scale(1.12);
	}

	.size-group {
		display: inline-flex;
		border: 1px solid #2e3646;
		border-radius: 5px;
		overflow: hidden;
	}

	.size-btn {
		background: #161b23;
		color: #94a3b8;
		border: none;
		padding: 0.25rem 0.45rem;
		font-size: 0.7rem;
		font-weight: 600;
		cursor: pointer;
	}

	.size-btn.size-selected {
		background: #28303e;
		color: #fff;
	}

	.vol-slider {
		width: 76px;
		accent-color: #3b82f6;
	}
</style>
