import { createCipheriv, createHash, randomBytes } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import { decryptZoomAppContext, signRoomToken, verifyRoomToken } from './zoomAuth';

function encodeZoomContextForTest(payload: Record<string, unknown>, secret: string): string {
	const iv = randomBytes(12);
	const aad = Buffer.from('zoom-aad', 'utf8');
	const key = createHash('sha256').update(secret, 'utf8').digest();
	const cipher = createCipheriv('aes-256-gcm', key, iv);
	cipher.setAAD(aad);
	const ciphertext = Buffer.concat([
		cipher.update(JSON.stringify(payload), 'utf8'),
		cipher.final()
	]);
	const tag = cipher.getAuthTag();

	const ivLenBuf = Buffer.alloc(1);
	ivLenBuf.writeUInt8(iv.length, 0);
	const aadLenBuf = Buffer.alloc(2);
	aadLenBuf.writeUInt16LE(aad.length, 0);
	const cipherLenBuf = Buffer.alloc(4);
	cipherLenBuf.writeUInt32LE(ciphertext.length, 0);

	return Buffer.concat([ivLenBuf, iv, aadLenBuf, aad, cipherLenBuf, ciphertext, tag]).toString(
		'base64'
	);
}

describe('zoomAuth', () => {
	it('decrypts a valid X-Zoom-App-Context payload', () => {
		const secret = 'super_secret_zoom_client_secret';
		const header = encodeZoomContextForTest(
			{ uid: 'user-123', mid: 'meeting-abc', collaborateUuid: 'collab-999' },
			secret
		);
		const parsed = decryptZoomAppContext(header, secret);
		expect(parsed).not.toBeNull();
		expect(parsed?.uid).toBe('user-123');
		expect(parsed?.mid).toBe('meeting-abc');
		expect(parsed?.collaborateUuid).toBe('collab-999');
	});

	it('returns null for wrong secret or malformed context', () => {
		const header = encodeZoomContextForTest({ mid: 'meeting-abc' }, 'secret-a');
		expect(decryptZoomAppContext(header, 'secret-b')).toBeNull();
		expect(decryptZoomAppContext('not-valid-base64', 'secret-a')).toBeNull();
	});

	it('signs and verifies room tokens', () => {
		const secret = 'room_signing_secret';
		const token = signRoomToken('grading-room-42', secret);
		expect(verifyRoomToken('grading-room-42', token, secret)).toBe(true);
		expect(verifyRoomToken('other-room', token, secret)).toBe(false);
		expect(verifyRoomToken('grading-room-42', 'tampered', secret)).toBe(false);
	});
});
