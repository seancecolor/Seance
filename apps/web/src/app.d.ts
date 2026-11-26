import type { ZoomAppContext } from '$lib/server/zoomAuth';

export interface SeanceSession {
	roomId: string;
	role: 'host' | 'attendee';
	exp: number;
}

declare global {
	namespace App {
		interface Locals {
			zoomContext: ZoomAppContext | null;
			session: SeanceSession | null;
		}
	}
}

export {};
