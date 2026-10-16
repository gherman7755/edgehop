import { networkInterfaces } from 'node:os';

/** The usual private ranges first, so the address worth typing in is at the top of the list. */
export function localAddresses(interfaces: ReturnType<typeof networkInterfaces> = networkInterfaces()): string[] {
	const found: string[] = [];
	for (const addresses of Object.values(interfaces)) {
		for (const address of addresses ?? []) {
			if (address.family === 'IPv4' && !address.internal) found.push(address.address);
		}
	}
	const rank = (ip: string): number => (ip.startsWith('192.168.') ? 0 : ip.startsWith('10.') ? 1 : /^172\.(1[6-9]|2\d|3[01])\./.test(ip) ? 2 : 3);
	return [...new Set(found)].sort((a, b) => rank(a) - rank(b));
}
