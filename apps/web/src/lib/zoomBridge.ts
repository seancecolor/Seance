import zoomSdk from '@zoom/appssdk';

export interface ZoomUser {
	screenName: string;
	role: string;
	participantUUID: string;
}

export interface ZoomBridgeState {
	inZoom: boolean;
	runningContext: string;
	user: ZoomUser | null;
	collaborating: boolean;
}

export async function initZoomBridge(
	onCollaborateChange?: (collaborating: boolean) => void
): Promise<ZoomBridgeState> {
	try {
		const config = await zoomSdk.config({
			popoutSize: { width: 1920, height: 1080 },
			capabilities: [
				'getRunningContext',
				'getUserContext',
				'startCollaborate',
				'endCollaborate',
				'appPopout',
				'openUrl',
				'onCollaborateChange'
			]
		});

		const userCtx = await zoomSdk.getUserContext();

		if (onCollaborateChange) {
			zoomSdk.addEventListener('onCollaborateChange', (event: { action?: string }) => {
				onCollaborateChange(event.action === 'start');
			});
		}

		return {
			inZoom: true,
			runningContext: config.runningContext || 'inMeeting',
			user: {
				screenName: userCtx.screenName || 'Zoom Participant',
				role: userCtx.role || 'attendee',
				participantUUID: userCtx.participantUUID || crypto.randomUUID()
			},
			collaborating: false
		};
	} catch {
		return {
			inZoom: false,
			runningContext: 'browser',
			user: null,
			collaborating: false
		};
	}
}

export async function startZoomCollaborate(): Promise<void> {
	await zoomSdk.startCollaborate({});
}

export async function endZoomCollaborate(): Promise<void> {
	await zoomSdk.endCollaborate();
}

export async function popoutZoomApp(): Promise<void> {
	await zoomSdk.appPopout({ action: 'start' });
}
