import http from 'node:http';
import https from 'node:https';
import { isAuthEnforced, verifyRequestSession } from './session-auth.js';

const httpAgent = new http.Agent({ keepAlive: true, maxSockets: 64 });
const httpsAgent = new https.Agent({ keepAlive: true, maxSockets: 64 });

/**
 * Streaming same-origin reverse proxy for `/hls/*` -> MediaMTX (`:8888`).
 * Requires a valid signed `seance_session` cookie whenever `ROOM_SIGNING_SECRET`
 * is configured, and restricts requests to the configured MediaMTX stream path.
 *
 * @param {import('node:http').IncomingMessage} req
 * @param {import('node:http').ServerResponse} res
 * @returns {boolean} true if the request was handled by the proxy
 */
export function handleHlsProxy(req, res) {
	if (!req.url || !req.url.startsWith('/hls/')) {
		return false;
	}

	if (isAuthEnforced()) {
		const session = verifyRequestSession(req.headers.cookie);
		if (!session) {
			res.writeHead(401, { 'Content-Type': 'text/plain', 'Cache-Control': 'no-store' });
			res.end('Unauthorized');
			return true;
		}
	}

	const allowedStream = (process.env.PUBLIC_MEDIAMTX_STREAM_PATH || 'cleanfeed').replace(
		/^\/+|\/+$/g,
		''
	);
	const upstreamPath = req.url.slice('/hls'.length);

	// Prevent path traversal or probing arbitrary MediaMTX paths
	let parsedPath;
	try {
		parsedPath = new URL(upstreamPath, 'http://mediamtx.local');
	} catch {
		res.writeHead(400, { 'Content-Type': 'text/plain' });
		res.end('Bad Request');
		return true;
	}
	if (
		parsedPath.pathname.includes('..') ||
		!(
			parsedPath.pathname === `/${allowedStream}` ||
			parsedPath.pathname.startsWith(`/${allowedStream}/`)
		)
	) {
		res.writeHead(403, { 'Content-Type': 'text/plain' });
		res.end('Forbidden');
		return true;
	}

	const baseUrl = (
		process.env.MEDIAMTX_INTERNAL_URL ||
		process.env.PUBLIC_MEDIAMTX_HLS_ORIGIN ||
		'http://mediamtx.railway.internal:8888'
	).replace(/\/+$/, '');

	const targetUrl = new URL(`${parsedPath.pathname}${parsedPath.search}`, baseUrl);
	const isHttps = targetUrl.protocol === 'https:';
	const client = isHttps ? https : http;

	const headers = { ...req.headers, host: targetUrl.host };
	delete headers.connection;

	const proxyReq = client.request(
		targetUrl,
		{
			method: req.method,
			headers,
			agent: isHttps ? httpsAgent : httpAgent
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
		if (!res.writableEnded) {
			res.end(`HLS upstream error: ${err.message}`);
		}
	});

	res.on('close', () => {
		if (!proxyReq.destroyed) {
			proxyReq.destroy();
		}
	});

	req.pipe(proxyReq);
	return true;
}
