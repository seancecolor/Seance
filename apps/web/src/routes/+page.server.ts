import { env } from '$env/dynamic/private';
import { env as publicEnv } from '$env/dynamic/public';
import { signRoomToken, verifyRoomToken } from '$lib/server/zoomAuth';
import { fail, redirect } from '@sveltejs/kit';
import {
	createSessionToken,
	SESSION_COOKIE_NAME,
	verifyHostPasscode
} from '../../session-auth.js';
import type { Actions, PageServerLoad } from './$types';

function isSecureRequest(request: Request, url: URL): boolean {
	return (
		url.protocol === 'https:' ||
		request.headers.get('x-forwarded-proto')?.split(',')[0]?.trim() === 'https'
	);
}

function getPublicOrigin(request: Request, url: URL): string {
	const proto = isSecureRequest(request, url) ? 'https' : 'http';
	const host =
		request.headers.get('x-forwarded-host')?.split(',')[0]?.trim() ||
		request.headers.get('host') ||
		url.host;
	return `${proto}://${host}`;
}

function sanitizeRoomId(raw: string | null | undefined): string {
	const cleaned = (raw || '')
		.trim()
		.replace(/[^a-zA-Z0-9_-]/g, '-')
		.replace(/-+/g, '-')
		.replace(/^-|-$/g, '')
		.slice(0, 64);
	return cleaned || 'seance-studio';
}

export const load: PageServerLoad = async ({ locals, url, request, cookies }) => {
	const streamPath = (publicEnv.PUBLIC_MEDIAMTX_STREAM_PATH || 'cleanfeed').replace(
		/^\/+|\/+$/g,
		''
	);
	const hlsUrl = `/hls/${streamPath}/index.m3u8`;
	const signingSecret = env.ROOM_SIGNING_SECRET || '';
	const isSecure = isSecureRequest(request, url);
	const publicOrigin = getPublicOrigin(request, url);

	const queryRoom = url.searchParams.get('room');
	const querySig = url.searchParams.get('sig');

	let roomId: string | null = null;
	let role: 'host' | 'attendee' = 'attendee';
	let shouldIssueCookie = false;

	if (locals.zoomContext?.mid) {
		roomId = `zoom-${locals.zoomContext.mid}`;
		role = locals.zoomContext.typ === 'host' ? 'host' : 'attendee';
		shouldIssueCookie = true;
	} else if (queryRoom && signingSecret && querySig && verifyRoomToken(queryRoom, querySig, signingSecret)) {
		roomId = queryRoom;
		role = locals.session?.roomId === queryRoom && locals.session.role === 'host' ? 'host' : 'attendee';
		shouldIssueCookie = true;
	} else if (locals.session) {
		roomId = locals.session.roomId;
		role = locals.session.role;
	} else if (!signingSecret) {
		// Zero-config local dev fallback when ROOM_SIGNING_SECRET is not set
		roomId = sanitizeRoomId(queryRoom);
		role = 'host';
	}

	if (!roomId) {
		return {
			authorized: false as const,
			roomId: null,
			role: null,
			hlsUrl: null,
			shareUrl: null,
			zoomContext: null
		};
	}

	if (shouldIssueCookie && signingSecret) {
		const token = createSessionToken(roomId, role, signingSecret);
		cookies.set(SESSION_COOKIE_NAME, token, {
			path: '/',
			httpOnly: true,
			maxAge: 60 * 60 * 24,
			sameSite: isSecure ? 'none' : 'lax',
			secure: isSecure
		});
	}

	const effectiveSecret = signingSecret || 'dev_local_signing_secret_change_in_prod';
	const sig = signRoomToken(roomId, effectiveSecret);
	const shareUrl = `${publicOrigin}/?room=${encodeURIComponent(roomId)}&sig=${encodeURIComponent(sig)}`;

	return {
		authorized: true as const,
		roomId,
		role,
		hlsUrl,
		shareUrl,
		zoomContext: locals.zoomContext
	};
};

export const actions: Actions = {
	unlock: async ({ request, cookies, url }) => {
		const form = await request.formData();
		const passcode = String(form.get('passcode') || '');
		const room = sanitizeRoomId(String(form.get('room') || 'seance-studio'));

		const signingSecret = env.ROOM_SIGNING_SECRET || '';
		if (!signingSecret) {
			throw redirect(303, `/?room=${encodeURIComponent(room)}`);
		}

		if (!verifyHostPasscode(passcode, env.HOST_PASSCODE, signingSecret)) {
			return fail(401, { error: 'Invalid host passcode.', room });
		}

		const isSecure = isSecureRequest(request, url);
		const token = createSessionToken(room, 'host', signingSecret);
		cookies.set(SESSION_COOKIE_NAME, token, {
			path: '/',
			httpOnly: true,
			maxAge: 60 * 60 * 24,
			sameSite: isSecure ? 'none' : 'lax',
			secure: isSecure
		});

		throw redirect(303, '/');
	},

	lock: async ({ cookies, request, url }) => {
		const isSecure = isSecureRequest(request, url);
		cookies.delete(SESSION_COOKIE_NAME, {
			path: '/',
			httpOnly: true,
			sameSite: isSecure ? 'none' : 'lax',
			secure: isSecure
		});
		throw redirect(303, '/');
	}
};
