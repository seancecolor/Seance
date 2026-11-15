import { env } from '$env/dynamic/private';
import { env as publicEnv } from '$env/dynamic/public';
import { decryptZoomAppContext } from '$lib/server/zoomAuth';
import type { Handle } from '@sveltejs/kit';

export const handle: Handle = async ({ event, resolve }) => {
	const zoomHeader = event.request.headers.get('x-zoom-app-context');
	if (zoomHeader && env.ZOOM_CLIENT_SECRET) {
		event.locals.zoomContext = decryptZoomAppContext(zoomHeader, env.ZOOM_CLIENT_SECRET);
	} else {
		event.locals.zoomContext = null;
	}

	const response = await resolve(event);

	const hlsOrigin = (publicEnv.PUBLIC_MEDIAMTX_HLS_ORIGIN || 'https://stream.visitantmedia.com').replace(
		/\/+$/,
		''
	);

	// Mandatory Zoom App OWASP security headers
	response.headers.set(
		'Strict-Transport-Security',
		'max-age=31536000; includeSubDomains'
	);
	response.headers.set('X-Content-Type-Options', 'nosniff');
	response.headers.set('Referrer-Policy', 'strict-origin-when-cross-origin');
	response.headers.set(
		'Content-Security-Policy',
		[
			"default-src 'self'",
			"script-src 'self' 'unsafe-inline' https://appssdk.zoom.us",
			"style-src 'self' 'unsafe-inline'",
			"img-src 'self' data: blob:",
			`media-src 'self' blob: ${hlsOrigin}`,
			`connect-src 'self' ws: wss: ${hlsOrigin} https://appssdk.zoom.us`,
			"worker-src 'self' blob:"
		].join('; ')
	);

	return response;
};
