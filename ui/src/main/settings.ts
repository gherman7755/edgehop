import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import { DEFAULT_SETTINGS, type AppMode, type Settings, type Side } from '../shared/types';

/** Encrypts secrets with what the operating system offers (Electron's safeStorage). */
export interface Vault {
	available(): Promise<boolean>;
	encrypt(text: string): Promise<Buffer>;
	decrypt(data: Buffer): Promise<string>;
}

const SECRET_KEYS = ['peerToken', 'ownToken'] as const;
type SecretKey = (typeof SECRET_KEYS)[number];

interface FileShape {
	version: 1;
	values: Record<string, unknown>;
	/** Base64 of the vault's ciphertext. */
	secrets: Partial<Record<SecretKey, string>>;
}

const SIDES: readonly Side[] = ['left', 'right', 'top', 'bottom'];
const MODES: readonly AppMode[] = ['controller', 'receiver'];

/** Turns whatever was on disk into valid settings: anything missing or of the wrong type becomes the default. */
export function sanitize(values: Record<string, unknown>): Omit<Settings, SecretKey> {
	const bool = (key: keyof Settings): boolean => (typeof values[key] === 'boolean' ? (values[key] as boolean) : (DEFAULT_SETTINGS[key] as boolean));
	return {
		mode: MODES.includes(values.mode as AppMode) ? (values.mode as AppMode) : DEFAULT_SETTINGS.mode,
		host: typeof values.host === 'string' ? values.host.slice(0, 253) : DEFAULT_SETTINGS.host,
		side: SIDES.includes(values.side as Side) ? (values.side as Side) : DEFAULT_SETTINGS.side,
		switching: bool('switching'),
		forwardKeys: bool('forwardKeys'),
		allowKeys: bool('allowKeys'),
		debug: bool('debug'),
	};
}

/**
 * Settings in one JSON file. Tokens are stored encrypted; if the system cannot encrypt, they are kept for this run
 * only and never written (`tokensPersist` is false so the UI can say so).
 */
export class SettingsStore {
	private settings: Settings = { ...DEFAULT_SETTINGS };
	private tokensPersist = false;
	private loaded = false;
	private writing: Promise<void> = Promise.resolve();

	constructor(
		private readonly path: string,
		private readonly vault: Vault,
	) {}

	async load(): Promise<{ settings: Settings; tokensPersist: boolean }> {
		if (!this.loaded) {
			this.loaded = true;
			this.tokensPersist = await this.vault.available();
			let file: Partial<FileShape> = {};
			try {
				file = JSON.parse(await readFile(this.path, 'utf8')) as Partial<FileShape>;
			} catch {
				// first run, or an unreadable file: start from the defaults
			}
			const secrets: Partial<Settings> = {};
			if (this.tokensPersist && file.secrets) {
				for (const key of SECRET_KEYS) {
					const stored = file.secrets[key];
					if (typeof stored !== 'string') continue;
					try {
						secrets[key] = await this.vault.decrypt(Buffer.from(stored, 'base64'));
					} catch {
						// encrypted for another user or machine: ask again
					}
				}
			}
			this.settings = { ...DEFAULT_SETTINGS, ...sanitize(file.values ?? {}), ...secrets };
		}
		return { settings: { ...this.settings }, tokensPersist: this.tokensPersist };
	}

	async save(patch: Partial<Settings>): Promise<void> {
		await this.load();
		this.settings = { ...this.settings, ...sanitize({ ...this.settings, ...patch }) };
		for (const key of SECRET_KEYS) {
			if (typeof patch[key] === 'string') this.settings[key] = patch[key].slice(0, 200);
		}
		const snapshot = { ...this.settings };
		const persist = this.tokensPersist;
		this.writing = this.writing.then(() => this.write(snapshot, persist)).catch(() => undefined);
		await this.writing;
	}

	private async write(settings: Settings, persistTokens: boolean): Promise<void> {
		const { peerToken, ownToken, ...values } = settings;
		const secrets: FileShape['secrets'] = {};
		if (persistTokens) {
			if (peerToken) secrets.peerToken = (await this.vault.encrypt(peerToken)).toString('base64');
			if (ownToken) secrets.ownToken = (await this.vault.encrypt(ownToken)).toString('base64');
		}
		const file: FileShape = { version: 1, values, secrets };
		await mkdir(dirname(this.path), { recursive: true });
		const temporary = `${this.path}.tmp`;
		await writeFile(temporary, JSON.stringify(file, null, 2), { mode: 0o600 });
		await rename(temporary, this.path);
	}
}
