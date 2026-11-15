export interface NormalizedPoint {
	/** Normalized X coordinate (0.0 .. 1.0) relative to rendered video frame */
	x: number;
	/** Normalized Y coordinate (0.0 .. 1.0) relative to rendered video frame */
	y: number;
}

export interface DrawingStroke {
	id: string;
	userId: string;
	userName: string;
	color: string;
	/** Normalized stroke width relative to video height (e.g. 0.004) */
	width: number;
	points: NormalizedPoint[];
	timestamp: number;
	/** Optional video currentTime in seconds when stroke was started */
	frameTime?: number;
}

export interface ChatMessage {
	id: string;
	userId: string;
	userName: string;
	text: string;
	timestamp: number;
}

export interface Participant {
	userId: string;
	userName: string;
	role: string;
	inZoom: boolean;
}

export type ClientMessage =
	| {
			type: 'join';
			roomId: string;
			userId: string;
			userName: string;
			role: string;
			inZoom: boolean;
	  }
	| { type: 'chat'; text: string }
	| { type: 'stroke_add'; stroke: DrawingStroke }
	| { type: 'stroke_undo'; strokeId?: string }
	| { type: 'stroke_clear' };

export type ServerMessage =
	| {
			type: 'room_state';
			roomId: string;
			participants: Participant[];
			messages: ChatMessage[];
			strokes: DrawingStroke[];
	  }
	| { type: 'presence'; participants: Participant[] }
	| { type: 'chat'; message: ChatMessage }
	| { type: 'stroke_add'; stroke: DrawingStroke }
	| { type: 'stroke_undo'; strokeId?: string; userId: string }
	| { type: 'stroke_clear'; userId: string }
	| { type: 'error'; message: string };
