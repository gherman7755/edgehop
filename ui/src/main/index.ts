import { app, BrowserWindow, ipcMain, safeStorage, shell } from 'electron';
import { join } from 'node:path';
import { ALLOWED_METHODS, type ControllerState, type CoreEvent, type ReceiverState, type Settings } from '../shared/types';
import { CoreError } from './core/client';
import { locateCore } from './core/locate';
import { CoreSupervisor } from './core/supervisor';
import { localAddresses } from './network';
import { SettingsStore, type Vault } from './settings';
import { runSmoke } from './smoke';
import { createTray, type TrayHandle } from './tray';

const isMac = process.platform === 'darwin';

// Tests run against a throw-away profile, never the real settings.
if (process.env.EDGEHOP_USER_DATA) app.setPath('userData', process.env.EDGEHOP_USER_DATA);

if (!app.requestSingleInstanceLock()) {
	// Two copies would fight over the port and the input hooks.
	app.quit();
}

const repoRoot = join(__dirname, '..', '..', '..');
const supervisor = new CoreSupervisor({
	locate: () =>
		locateCore({
			platform: process.platform,
			override: process.env.EDGEHOP_CORE,
			resourcesPath: app.isPackaged ? process.resourcesPath : undefined,
			repoRoot,
		}),
});

const vault: Vault = {
	available: () => safeStorage.isAsyncEncryptionAvailable(),
	encrypt: text => safeStorage.encryptStringAsync(text),
	decrypt: async data => (await safeStorage.decryptStringAsync(data)).result,
};

let settings: SettingsStore;
let window: BrowserWindow | null = null;
let tray: TrayHandle | null = null;
let quitting = false;
let controllerState: ControllerState = { state: 'idle' };
let receiverState: ReceiverState = { state: 'stopped' };

/**
 * Ends the process once everything is stopped. `process.exit` and not `app.exit`: on macOS `app.exit` sometimes
 * returned and the process then never ended (about one run in a hundred; a smoke test of the packaged app in CI hung
 * for ten minutes, and the same would have happened to someone choosing Quit). 300 packaged runs in a row with
 * `process.exit` all ended (`ui/scripts/smoke-stress.sh`); settings are written when they change, so nothing is lost.
 */
function leave(code: number): never {
	process.exit(code);
}

function sharing(): boolean {
	return controllerState.state === 'connecting' || controllerState.state === 'connected' || receiverState.state !== 'stopped';
}

function statusLine(): string {
	if (controllerState.state === 'connected') {
		return controllerState.remoteActive ? `Controlling ${controllerState.peer}` : `Connected to ${controllerState.peer}`;
	}
	if (controllerState.state === 'connecting') return 'Connecting…';
	if (receiverState.state === 'connected') return `Being controlled by ${receiverState.peer}`;
	if (receiverState.state === 'listening') return `Waiting for a computer (port ${receiverState.port})`;
	return 'Not sharing';
}

function updateTray(): void {
	tray?.update({ status: statusLine(), sharing: sharing(), active: controllerState.state === 'connected' && controllerState.remoteActive });
}

function send(channel: string, payload: unknown): void {
	if (window && !window.isDestroyed()) window.webContents.send(channel, payload);
}

function track(event: CoreEvent): void {
	if (event.event === 'controller.state') controllerState = event.data;
	else if (event.event === 'receiver.state') receiverState = event.data;
	else return;
	updateTray();
}

async function stopSharing(): Promise<void> {
	const client = supervisor.active;
	if (!client) return;
	await Promise.allSettled([client.request('controller.stop'), client.request('receiver.stop')]);
}

function showWindow(): void {
	if (!window || window.isDestroyed()) createWindow();
	else {
		if (window.isMinimized()) window.restore();
		window.show();
		window.focus();
	}
}

function createWindow(): void {
	window = new BrowserWindow({
		width: 560,
		height: 780,
		minWidth: 480,
		minHeight: 560,
		show: false,
		title: 'EdgeHop',
		backgroundColor: '#0f1115',
		...(isMac ? { titleBarStyle: 'hiddenInset' as const, trafficLightPosition: { x: 18, y: 18 } } : {}),
		webPreferences: {
			preload: join(__dirname, '../preload/index.js'),
			contextIsolation: true,
			sandbox: true,
			nodeIntegration: false,
		},
	});
	window.once('ready-to-show', () => window?.show());
	window.on('close', event => {
		if (!quitting && sharing()) {
			event.preventDefault(); // keep sharing from the tray
			window?.hide();
		}
	});
	window.on('closed', () => {
		window = null;
	});
	// The UI is a single local page: never navigate away, open links in the browser.
	window.webContents.on('will-navigate', event => event.preventDefault());
	window.webContents.setWindowOpenHandler(({ url }) => {
		if (/^https:\/\//.test(url)) void shell.openExternal(url);
		return { action: 'deny' };
	});
	if (process.env.ELECTRON_RENDERER_URL) void window.loadURL(process.env.ELECTRON_RENDERER_URL);
	else void window.loadFile(join(__dirname, '../renderer/index.html'));
}

function registerIpc(): void {
	ipcMain.handle('core:request', async (_event, method: unknown, params: unknown) => {
		try {
			if (typeof method !== 'string' || !(ALLOWED_METHODS as readonly string[]).includes(method)) {
				throw new CoreError('unknown_method', `Not allowed: ${String(method)}`);
			}
			const client = supervisor.active;
			if (!client) throw new CoreError('closed', 'The core is not running.');
			const safeParams = typeof params === 'object' && params !== null ? (params as Record<string, unknown>) : undefined;
			return { ok: true, result: await client.request(method, safeParams) };
		} catch (error) {
			const code = error instanceof CoreError ? error.code : 'internal';
			return { ok: false, error: { code, message: error instanceof Error ? error.message : String(error) } };
		}
	});
	ipcMain.handle('core:get-status', () => supervisor.status);
	ipcMain.handle('core:restart', async () => {
		controllerState = { state: 'idle' };
		receiverState = { state: 'stopped' };
		updateTray();
		await supervisor.start();
	});
	ipcMain.handle('settings:load', () => settings.load());
	ipcMain.handle('settings:save', (_event, patch: Partial<Settings>) => settings.save(patch ?? {}));
	ipcMain.handle('net:addresses', () => localAddresses());
}

app.on('second-instance', () => showWindow());
app.on('activate', () => showWindow());
app.on('window-all-closed', () => {
	if (!isMac && !sharing()) app.quit();
});
app.on('before-quit', event => {
	if (quitting) return;
	quitting = true;
	event.preventDefault();
	// Release the keyboard and mouse before leaving: closing the core's input does the same if this is cut short.
	void supervisor.stop().finally(() => leave(0));
});

void app.whenReady().then(async () => {
	settings = new SettingsStore(join(app.getPath('userData'), 'settings.json'), vault);
	registerIpc();
	tray = createTray({
		onShow: showWindow,
		onStop: () => void stopSharing(),
		onQuit: () => app.quit(),
	});
	updateTray();
	supervisor.on('status', status => send('core:status', status));
	supervisor.on('event', (event: CoreEvent) => {
		track(event);
		send('core:event', event);
	});
	supervisor.on('stderr', (line: string) => {
		console.error(`[core] ${line}`);
		send('core:event', { event: 'log', data: { source: 'core', line } } satisfies CoreEvent);
	});
	createWindow();
	if (process.env.EDGEHOP_SMOKE) {
		const started = supervisor.start();
		try {
			await started;
			console.log(await runSmoke(window!));
			quitting = true;
			await supervisor.stop();
			leave(0);
		} catch (error) {
			console.error(String(error));
			quitting = true;
			await supervisor.stop();
			leave(1);
		}
		return;
	}
	await supervisor.start();
});
