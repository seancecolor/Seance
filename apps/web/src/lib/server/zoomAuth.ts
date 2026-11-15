import { createDecipheriv, createHash, createHmac, timingSafeEqual } from 'node:crypto';

export interface ZoomAppContext {
	uid?: string;
	mid?: string;
	collaborateUuid?: string;
	typ?: string;
	ts?: number;
	exp?: number;
	[key: string]: unknown;
}

/**
 * Decrypts Zoom's `X-Zoom-App-Context` HTTP header using AES-256-GCM.
 * Format: [ivLen (1B)][iv][aadLen (2B LE)][aad][cipherLen (4B LE)][cipher][tag (16B)]
 * Key: SHA-256(ZOOM_CLIENT_SECRET)
 */
export function decryptZoomAppContext(
	headerValue: string,
	clientSecret: string
): ZoomAppContext | null {
	if (!headerValue || !clientSecret) return null;
	try {
		const raw = Buffer.from(headerValue, 'base64');
		if (raw.length < 1 + 2 + 4 + 16) return null;

		let offset = 0;
		const ivLen = raw.readUInt8(offset);
		offset += 1;
		if (offset + ivLen > raw.length) return null;
		const iv = raw.subarray(offset, offset + ivLen);
		offset += ivLen;

		if (offset + 2 > raw.length) return null;
		const aadLen = raw.readUInt16LE(offset);
		offset += 2;
		if (offset + aadLen > raw.length) return null;
		const aad = raw.subarray(offset, offset + aadLen);
		offset += aadLen;

		if (offset + 4 > raw.length) return null;
		const cipherLen = raw.readUInt32LE(offset);
		offset += 4;
		if (offset + cipherLen + 16 > raw.length) return null;
		const cipherText = raw.subarray(offset, offset + cipherLen);
		offset += cipherLen;
		const authTag = raw.subarray(offset, offset + 16);

		const key = createHash('sha256').update(clientSecret, 'utf8').digest();
		const decipher = createDecipheriv('aes-256-gcm', key, iv);
		if (aadLen > 0) {
			decipher.setAAD(aad);
		}
		decipher.setAuthTag(authTag);

		const decrypted = Buffer.concat([decipher.update(cipherText), decipher.final()]);
		return JSON.parse(decrypted.toString('utf8')) as ZoomAppContext;
	} catch {
		return null;
	}
}

export function signRoomToken(roomId: string, secret: string): string {
	return createHmac('sha256', secret).update(roomId, 'utf8').digest('base64url');
}

export function verifyRoomToken(roomId: string, token: string, secret: string): boolean {
	if (!roomId || !token || !secret) return false;
	const expected = signRoomToken(roomId, secret);
	const a = Buffer.from(expected, 'utf8');
	const b = Buffer.from(token, 'utf8');
	if (a.length !== b.length) return false;
	return timingSafeEqual(a, b);
}
