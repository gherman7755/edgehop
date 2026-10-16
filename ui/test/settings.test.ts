import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { DEFAULT_SETTINGS } from '../src/shared/types';
import { sanitize, SettingsStore, type Vault } from '../src/main/settings';

/** "Encrypts" by reversing and tagging, so the plaintext is visibly not what is stored. */
const vault = (available = true): Vault => ({
	available: async () => available,
	encrypt: async text => Buffer.from(`enc:${[...text].reverse().join('')}`),
	decrypt: async data => {
		const text = data.toString();
		if (!text.startsWith('enc:')) throw new Error('not ours');
		return [...text.slice(4)].reverse().join('');
	},
});

async function path(): Promise<string> {
	return join(await mkdtemp(join(tmpdir(), 'edgehop-settings-')), 'sub', 'settings.json');
}

describe('SettingsStore', () => {
	it('starts from the defaults', async () => {
		const store = new SettingsStore(await path(), vault());
		expect(await store.load()).toEqual({ settings: DEFAULT_SETTINGS, tokensPersist: true });
	});

	it('remembers settings and tokens across runs, with the tokens encrypted on disk', async () => {
		const file = await path();
		const first = new SettingsStore(file, vault());
		await first.save({ host: '192.168.0.128', side: 'left', debug: true, peerToken: 'PEERTOKEN123456', ownToken: 'OWNTOKEN123456789' });
		const text = await readFile(file, 'utf8');
		expect(text).not.toContain('PEERTOKEN123456');
		expect(text).not.toContain('OWNTOKEN123456789');

		const second = await new SettingsStore(file, vault()).load();
		expect(second.settings).toMatchObject({ host: '192.168.0.128', side: 'left', debug: true, peerToken: 'PEERTOKEN123456', ownToken: 'OWNTOKEN123456789' });
	});

	it('never writes tokens when the system cannot encrypt them, but keeps them for the run', async () => {
		const file = await path();
		const store = new SettingsStore(file, vault(false));
		await store.save({ host: 'pc', peerToken: 'SECRETSECRET12' });
		expect(await readFile(file, 'utf8')).not.toContain('SECRETSECRET12');
		const loaded = await store.load();
		expect(loaded.tokensPersist).toBe(false);
		expect(loaded.settings.peerToken).toBe('SECRETSECRET12');
		expect((await new SettingsStore(file, vault(false)).load()).settings.peerToken).toBe('');
	});

	it('survives an unreadable file and tokens that cannot be decrypted', async () => {
		const file = await path();
		await new SettingsStore(file, vault()).save({ host: 'x' }); // creates the folder
		await writeFile(file, '{ nope');
		expect((await new SettingsStore(file, vault()).load()).settings).toEqual(DEFAULT_SETTINGS);
		await writeFile(file, JSON.stringify({ version: 1, values: { host: 'h' }, secrets: { peerToken: Buffer.from('foreign').toString('base64') } }));
		expect((await new SettingsStore(file, vault()).load()).settings).toMatchObject({ host: 'h', peerToken: '' });
	});
});

describe('sanitize', () => {
	it('replaces values of the wrong type or outside the allowed set with the defaults', () => {
		expect(sanitize({ mode: 'nope', side: 'diagonal', host: 5, switching: 'yes', debug: 1 })).toEqual({
			mode: 'controller',
			side: 'right',
			host: '',
			switching: true,
			forwardKeys: false,
			allowKeys: false,
			debug: false,
		});
	});

	it('keeps valid values', () => {
		expect(sanitize({ mode: 'receiver', side: 'top', host: '10.0.0.2', switching: false, forwardKeys: true, allowKeys: true, debug: true })).toEqual({
			mode: 'receiver',
			side: 'top',
			host: '10.0.0.2',
			switching: false,
			forwardKeys: true,
			allowKeys: true,
			debug: true,
		});
	});
});
