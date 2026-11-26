import { describe, expect, it } from 'vitest';
import {
	createSessionToken,
	parseCookieHeader,
	SESSION_COOKIE_NAME,
	verifyHostPasscode,
	verifyRequestSession,
	verifySessionToken
} from '../../../session-auth.js';

describe('session-auth', () => {
	const secret = 'test_room_signing_secret_123';

	it('creates and verifies a valid session token', () => {
		const token = createSessionToken('client-grade-1', 'host', secret);
		const session = verifySessionToken(token, secret);
		expect(session).not.toBeNull();
		expect(session?.roomId).toBe('client-grade-1');
		expect(session?.role).toBe('host');
	});

	it('rejects tampered or wrong-secret tokens', () => {
		const token = createSessionToken('client-grade-1', 'attendee', secret);
		expect(verifySessionToken(token, 'wrong_secret')).toBeNull();
		expect(verifySessionToken(`${token}x`, secret)).toBeNull();
		expect(verifySessionToken('invalid.token', secret)).toBeNull();
	});

	it('rejects expired tokens', () => {
		const expired = createSessionToken('client-grade-1', 'attendee', secret, -1000);
		expect(verifySessionToken(expired, secret)).toBeNull();
	});

	it('extracts and verifies cookie from HTTP Cookie header', () => {
		const token = createSessionToken('zoom-999', 'attendee', secret);
		const header = `other=1; ${SESSION_COOKIE_NAME}=${encodeURIComponent(token)}; foo=bar`;
		expect(parseCookieHeader(header)[SESSION_COOKIE_NAME]).toBe(token);

		const session = verifyRequestSession(header, secret);
		expect(session?.roomId).toBe('zoom-999');
		expect(session?.role).toBe('attendee');
	});

	it('verifies host passcode with fallback to signing secret', () => {
		expect(verifyHostPasscode('my-pass', 'my-pass', secret)).toBe(true);
		expect(verifyHostPasscode('wrong', 'my-pass', secret)).toBe(false);
		expect(verifyHostPasscode(secret, '', secret)).toBe(true);
	});
});
