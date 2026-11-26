import { createHash, createHmac, timingSafeEqual } from 'node:crypto';

export const SESSION_COOKIE_NAME = 'seance_session';
const DEFAULT_TTL_MS = 24 * 60 * 60 * 1000;

/**
 * @typedef {{ roomId: string, role: 'host' | 'attendee', exp: number }} SessionPayload
 */

/**
 * Auth is enforced whenever `ROOM_SIGNING_SECRET` is configured in the environment.
 * @returns {boolean}
 */
export function isAuthEnforced() {
	return Boolean(process.env.ROOM_SIGNING_SECRET);
}

/**
 * @param {string} a
 * @param {string} b
 * @returns {boolean}
 */
export function safeEqualStrings(a, b) {
	if (!a || !b) return false;
	const ha = createHash('sha256').update(a, 'utf8').digest();
	const hb = createHash('sha256').update(b, 'utf8').digest();
	return timingSafeEqual(ha, hb);
}

/**
 * @param {string} roomId
 * @param {'host' | 'attendee'} role
 * @param {string} secret
 * @param {number} [ttlMs]
 * @returns {string}
 */
export function createSessionToken(roomId, role, secret, ttlMs = DEFAULT_TTL_MS) {
	const payload = {
		roomId,
		role: role === 'host' ? 'host' : 'attendee',
		exp: Date.now() + ttlMs
	};
	const encoded = Buffer.from(JSON.stringify(payload), 'utf8').toString('base64url');
	const sig = createHmac('sha256', secret).update(encoded, 'utf8').digest('base64url');
	return `${encoded}.${sig}`;
}

/**
 * @param {string | undefined | null} token
 * @param {string | undefined | null} secret
 * @returns {SessionPayload | null}
 */
export function verifySessionToken(token, secret) {
	if (!token || !secret) return null;
	const dot = token.indexOf('.');
	if (dot <= 0 || dot === token.length - 1) return null;

	const encoded = token.slice(0, dot);
	const sig = token.slice(dot + 1);
	const expected = createHmac('sha256', secret).update(encoded, 'utf8').digest('base64url');

	const a = Buffer.from(sig, 'utf8');
	const b = Buffer.from(expected, 'utf8');
	if (a.length !== b.length || !timingSafeEqual(a, b)) return null;

	try {
		const parsed = JSON.parse(Buffer.from(encoded, 'base64url').toString('utf8'));
		if (
			typeof parsed?.roomId !== 'string' ||
			!parsed.roomId ||
			typeof parsed?.exp !== 'number' ||
			Date.now() > parsed.exp
		) {
			return null;
		}
		return {
			roomId: parsed.roomId.slice(0, 128),
			role: parsed.role === 'host' ? 'host' : 'attendee',
			exp: parsed.exp
		};
	} catch {
		return null;
	}
}

/**
 * @param {string | undefined | null} cookieHeader
 * @returns {Record<string, string>}
 */
export function parseCookieHeader(cookieHeader) {
	/** @type {Record<string, string>} */
	const out = {};
	if (!cookieHeader) return out;
	for (const part of cookieHeader.split(';')) {
		const eq = part.indexOf('=');
		if (eq <= 0) continue;
		const key = part.slice(0, eq).trim();
		const val = part.slice(eq + 1).trim();
		if (key) out[key] = decodeURIComponent(val);
	}
	return out;
}

/**
 * @param {string | undefined | null} cookieHeader
 * @param {string | undefined | null} [secret]
 * @returns {SessionPayload | null}
 */
export function verifyRequestSession(cookieHeader, secret = process.env.ROOM_SIGNING_SECRET) {
	const cookies = parseCookieHeader(cookieHeader);
	return verifySessionToken(cookies[SESSION_COOKIE_NAME], secret);
}

/**
 * @param {string} candidate
 * @param {string | undefined | null} [hostPasscode]
 * @param {string | undefined | null} [fallbackSecret]
 * @returns {boolean}
 */
export function verifyHostPasscode(
	candidate,
	hostPasscode = process.env.HOST_PASSCODE,
	fallbackSecret = process.env.ROOM_SIGNING_SECRET
) {
	const expected = hostPasscode || fallbackSecret;
	if (!expected) return false;
	return safeEqualStrings(candidate, expected);
}
