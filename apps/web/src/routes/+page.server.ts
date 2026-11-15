import { env } from '$env/dynamic/private';
import { env as publicEnv } from '$env/dynamic/public';
import { signRoomToken, verifyRoomToken } from '$lib/server/zoomAuth';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const hlsOrigin = (
		publicEnv.PUBLIC_MEDIAMTX_HLS_ORIGIN || 'https://stream.visitantmedia.com'
	).replace(/\/+$/, '');
	const streamPath = (publicEnv.PUBLIC_MEDIAMTX_STREAM_PATH || 'cleanfeed').replace(
		/^\/+|\/+$/g,
		''
	);
	const hlsUrl = `${hlsOrigin}/${streamPath}/index.m3u8`;

	const signingSecret = env.ROOM_SIGNING_SECRET || 'dev_local_signing_secret_change_in_prod';
	const queryRoom = url.searchParams.get('room');
	const querySig = url.searchParams.get('sig');

	let roomId = 'seance-studio';
	if (locals.zoomContext?.mid) {
		roomId = `zoom-${locals.zoomContext.mid}`;
	} else if (queryRoom) {
		const isValid = !env.ROOM_SIGNING_SECRET || (querySig ? verifyRoomToken(queryRoom, querySig, signingSecret) : false);
		if (isValid) {
			roomId = queryRoom;
		}
	}

	const sig = signRoomToken(roomId, signingSecret);
	const shareUrl = `${url.origin}/?room=${encodeURIComponent(roomId)}&sig=${encodeURIComponent(sig)}`;

	return {
		roomId,
		hlsUrl,
		shareUrl,
		zoomContext: locals.zoomContext
	};
};
