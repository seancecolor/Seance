import { env } from '$env/dynamic/private';
import { decryptZoomAppContext } from '$lib/server/zoomAuth';
import type { Handle } from '@sveltejs/kit';
import { verifyRequestSession } from '../session-auth.js';

const SCANNER_PATH_RE =
	/(?:\.(?:php\d*|asp|aspx|jsp|cgi|env|git|sql|bak)$|^\/(?:wp-admin|wp-content|wp-includes|wordpress|xmlrpc\.php|cgi-bin|phpmyadmin|\.env|\.git)(?:\/|$))/i;

export const handle: Handle = async ({ event, resolve }) => {
	// Quietly drop automated vulnerability scanner probes before SvelteKit logs 404 errors
	if (SCANNER_PATH_RE.test(event.url.pathname)) {
		return new Response('Not found', {
			status: 404,
			headers: { 'Cache-Control': 'no-store' }
		});
	}

	const zoomHeader = event.request.headers.get('x-zoom-app-context');
	if (zoomHeader && env.ZOOM_CLIENT_SECRET) {
		event.locals.zoomContext = decryptZoomAppContext(zoomHeader, env.ZOOM_CLIENT_SECRET);
	} else {
		event.locals.zoomContext = null;
	}

	event.locals.session = verifyRequestSession(
		event.request.headers.get('cookie'),
		env.ROOM_SIGNING_SECRET
	);

	const response = await resolve(event);

	// Mandatory Zoom App OWASP security headers (HLS is served same-origin via /hls/*)
	response.headers.set('Strict-Transport-Security', 'max-age=31536000; includeSubDomains');
	response.headers.set('X-Content-Type-Options', 'nosniff');
	response.headers.set('Referrer-Policy', 'strict-origin-when-cross-origin');
	response.headers.set(
		'Content-Security-Policy',
		[
			"default-src 'self'",
			"script-src 'self' 'unsafe-inline' https://appssdk.zoom.us",
			"style-src 'self' 'unsafe-inline'",
			"img-src 'self' data: blob:",
			"media-src 'self' blob:",
			"connect-src 'self' ws: wss: https://appssdk.zoom.us",
			"worker-src 'self' blob:"
		].join('; ')
	);

	return response;
};
