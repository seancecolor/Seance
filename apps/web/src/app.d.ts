import type { ZoomAppContext } from '$lib/server/zoomAuth';

declare global {
	namespace App {
		interface Locals {
			zoomContext: ZoomAppContext | null;
		}
	}
}

export {};
