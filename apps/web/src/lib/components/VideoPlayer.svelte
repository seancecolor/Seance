<script lang="ts">
	import Hls from 'hls.js';
	import { onMount } from 'svelte';
	import type { DrawingStroke, NormalizedPoint } from '$lib/shared/protocol';

	interface Props {
		hlsUrl: string;
		strokes: DrawingStroke[];
		userId: string;
		userName: string;
		onAddStroke: (stroke: DrawingStroke) => void;
		onUndoStroke: () => void;
		onClearStrokes: () => void;
	}

	let {
		hlsUrl,
		strokes,
		userId,
		userName,
		onAddStroke,
		onUndoStroke,
		onClearStrokes
	}: Props = $props();

	let containerEl: HTMLDivElement | undefined = $state();
	let videoEl: HTMLVideoElement | undefined = $state();
	let canvasEl: HTMLCanvasElement | undefined = $state();

	let hlsInstance: Hls | null = null;
	let playbackMode = $state<'native' | 'hls.js' | 'unsupported'>('hls.js');
	let statusText = $state('Connecting...');
	let muted = $state(true);
	let volume = $state(0.8);

	// Drawing state
	let drawEnabled = $state(true);
	let brushColor = $state('#ff3b30');
	let isDrawing = $state(false);
	let activePoints = $state<NormalizedPoint[]>([]);

	// Exact rendered video content box inside containerEl
	let overlayBox = $state({ left: 0, top: 0, width: 0, height: 0 });

	const COLORS = ['#ff3b30', '#ffcc00', '#34c759', '#00c7be', '#ffffff'];

	function updateOverlayRect() {
		if (!containerEl || !videoEl || !canvasEl) return;
		const cw = containerEl.clientWidth;
		const ch = containerEl.clientHeight;
		const vw = videoEl.videoWidth || 1920;
		const vh = videoEl.videoHeight || 1080;

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
		h: number
	) {
		if (points.length === 0) return;
		ctx.save();
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
		if (activePoints.length > 0) {
			drawStrokePath(ctx, activePoints, brushColor, 0.004, w, h);
		}
	}

	$effect(() => {
		// Re-render whenever remote strokes or active stroke change
		void strokes;
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

	function handlePointerDown(e: PointerEvent) {
		if (!drawEnabled || !canvasEl) return;
		const pt = toNormalizedPoint(e);
		if (!pt) return;
		canvasEl.setPointerCapture(e.pointerId);
		isDrawing = true;
		activePoints = [pt];
	}

	function handlePointerMove(e: PointerEvent) {
		if (!isDrawing) return;
		const pt = toNormalizedPoint(e);
		if (!pt) return;
		activePoints = [...activePoints, pt];
	}

	function handlePointerUp(e: PointerEvent) {
		if (!isDrawing || !canvasEl) return;
		if (canvasEl.hasPointerCapture(e.pointerId)) {
			canvasEl.releasePointerCapture(e.pointerId);
		}
		isDrawing = false;
		if (activePoints.length > 0) {
			onAddStroke({
				id: crypto.randomUUID(),
				userId,
				userName,
				color: brushColor,
				width: 0.004,
				points: activePoints,
				timestamp: Date.now(),
				frameTime: videoEl?.currentTime
			});
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

	onMount(() => {
		if (!videoEl || !containerEl) return;

		const ro = new ResizeObserver(() => updateOverlayRect());
		ro.observe(containerEl);
		videoEl.addEventListener('loadedmetadata', updateOverlayRect);
		videoEl.addEventListener('resize', updateOverlayRect);

		const isAppleNative =
			videoEl.canPlayType('application/vnd.apple.mpegurl') !== '' &&
			/Safari/i.test(navigator.userAgent) &&
			!/Chrome|Chromium/i.test(navigator.userAgent);

		if (isAppleNative) {
			playbackMode = 'native';
			videoEl.src = hlsUrl;
			statusText = 'Native 10-Bit LL-HLS';
			void videoEl.play().catch(() => {});
		} else if (Hls.isSupported()) {
			playbackMode = 'hls.js';
			const hls = new Hls({
				lowLatencyMode: true,
				maxLiveSyncPlaybackRate: 1.5,
				backBufferLength: 10
			});
			hlsInstance = hls;
			hls.loadSource(hlsUrl);
			hls.attachMedia(videoEl);
			hls.on(Hls.Events.MANIFEST_PARSED, () => {
				statusText = 'LL-HLS Live (hls.js)';
				void videoEl?.play().catch(() => {});
			});
			hls.on(Hls.Events.ERROR, (_event, data) => {
				if (data.fatal) {
					statusText = `Stream error (${data.details})`;
					if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
						setTimeout(() => hls.startLoad(), 1500);
					} else if (data.type === Hls.ErrorTypes.MEDIA_ERROR) {
						hls.recoverMediaError();
					}
				}
			});
		} else {
			playbackMode = 'unsupported';
			statusText = 'HLS playback unsupported in this browser';
		}

		updateOverlayRect();

		return () => {
			ro.disconnect();
			videoEl?.removeEventListener('loadedmetadata', updateOverlayRect);
			videoEl?.removeEventListener('resize', updateOverlayRect);
			hlsInstance?.destroy();
		};
	});
</script>

<div class="player-shell">
	<div class="stage" bind:this={containerEl}>
		<!-- svelte-ignore a11y_media_has_caption -->
		<video
			bind:this={videoEl}
			class="video-el"
			playsinline
			autoplay
			{muted}
		></video>

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
			<span class="status-pill" data-mode={playbackMode}>{statusText}</span>
			<button type="button" class="btn" onclick={resyncToLiveEdge}>
				Re-sync to Live Edge
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
		gap: 0.75rem;
		padding: 0.55rem 0.85rem;
		background: #12151b;
		border-top: 1px solid #1e232d;
	}

	.controls-group {
		display: flex;
		align-items: center;
		gap: 0.45rem;
	}

	.status-pill {
		font-size: 0.75rem;
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		padding: 0.2rem 0.55rem;
		border-radius: 4px;
		background: #1c212b;
		color: #a7b4c8;
	}

	.btn {
		background: #1d222c;
		color: #e2e8f0;
		border: 1px solid #2e3646;
		border-radius: 5px;
		padding: 0.3rem 0.65rem;
		font-size: 0.78rem;
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

	.vol-slider {
		width: 84px;
		accent-color: #3b82f6;
	}
</style>
