import http from 'node:http';
import https from 'node:https';

/**
 * Streaming same-origin reverse proxy for `/hls/*` -> MediaMTX (`:8888`).
 * Keeps LL-HLS playlists, cookies, and fMP4 segments same-origin so Safari,
 * Zoom WebView, and hls.js never hit third-party cookie or CORS 401 issues.
 *
 * @param {import('node:http').IncomingMessage} req
 * @param {import('node:http').ServerResponse} res
 * @returns {boolean} true if the request was handled by the proxy
 */
export function handleHlsProxy(req, res) {
	if (!req.url || !req.url.startsWith('/hls/')) {
		return false;
	}

	const baseUrl = (
		process.env.MEDIAMTX_INTERNAL_URL ||
		process.env.PUBLIC_MEDIAMTX_HLS_ORIGIN ||
		'http://mediamtx.railway.internal:8888'
	).replace(/\/+$/, '');

	const upstreamPath = req.url.slice('/hls'.length);
	const targetUrl = new URL(upstreamPath, baseUrl);
	const client = targetUrl.protocol === 'https:' ? https : http;

	const headers = { ...req.headers, host: targetUrl.host };
	delete headers.connection;

	const proxyReq = client.request(
		targetUrl,
		{
			method: req.method,
			headers
		},
		(proxyRes) => {
			const resHeaders = { ...proxyRes.headers };
			if (typeof resHeaders.location === 'string' && resHeaders.location.startsWith('/')) {
				resHeaders.location = `/hls${resHeaders.location}`;
			}
			res.writeHead(proxyRes.statusCode || 502, resHeaders);
			proxyRes.pipe(res);
		}
	);

	proxyReq.on('error', (err) => {
		if (!res.headersSent) {
			res.writeHead(502, { 'Content-Type': 'text/plain' });
		}
		res.end(`HLS upstream error: ${err.message}`);
	});

	req.pipe(proxyReq);
	return true;
}
