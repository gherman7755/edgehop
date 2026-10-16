// Types shared by the main process, the preload bridge and the renderer. They mirror docs/IPC.md.

export type Side = 'left' | 'right' | 'top' | 'bottom';

export type ControllerState =
	| { state: 'idle' }
	| { state: 'connecting' }
	| { state: 'connected'; peer: string; remoteActive: boolean }
	| { state: 'failed'; message: string };

export type ReceiverState =
	| { state: 'stopped' }
	| { state: 'listening'; port: number }
	| { state: 'connected'; peer: string };

export interface Peer {
	name: string;
}

export interface Permissions {
	accessibility: boolean;
	inputMonitoring: boolean;
}

export interface CoreInfo {
	name: string;
	version: string;
	ipcVersion: number;
	/** "macos", "windows", ... */
	platform: string;
	deviceName: string;
}

/** An unsolicited message from the core. */
export type CoreEvent =
	| { event: 'controller.state'; data: ControllerState }
	| { event: 'receiver.state'; data: ReceiverState }
	| { event: 'controller.warning'; data: { message: string } }
	| { event: 'latency'; data: { ms: number } }
	| { event: 'log'; data: { source: 'controller' | 'receiver' | 'core'; line: string } }
	| { event: 'peers.changed'; data: { peers: Peer[] } };

/** Whether the core process is up. `exited` carries a message fit for the user. */
export type CoreStatus =
	| { state: 'starting' }
	| { state: 'running'; info: CoreInfo }
	| { state: 'exited'; message: string };

export type AppMode = 'controller' | 'receiver';

/** What is remembered between runs. Tokens are encrypted on disk by the main process. */
export interface Settings {
	mode: AppMode;
	host: string;
	side: Side;
	switching: boolean;
	forwardKeys: boolean;
	allowKeys: boolean;
	debug: boolean;
	/** The token typed in on the controlling side. */
	peerToken: string;
	/** This computer's own token, shown on the receiving side. Empty until first generated. */
	ownToken: string;
}

export const DEFAULT_SETTINGS: Settings = {
	mode: 'controller',
	host: '',
	side: 'right',
	switching: true,
	forwardKeys: false,
	allowKeys: false,
	debug: false,
	peerToken: '',
	ownToken: '',
};

/** Error code and message from the core (or `closed` / `timeout` from the client). */
export interface CoreErrorInfo {
	code: string;
	message: string;
}

/** The methods the renderer may call; `core.shutdown` is the main process's business. */
export const ALLOWED_METHODS = [
	'core.hello',
	'permissions.get',
	'permissions.request',
	'token.generate',
	'peers.watch',
	'peers.list',
	'controller.start',
	'controller.stop',
	'controller.state',
	'receiver.start',
	'receiver.stop',
	'receiver.state',
] as const;

export type AllowedMethod = (typeof ALLOWED_METHODS)[number];

export interface EdgeHopApi {
	/** Resolves with the core's `result`; rejects with an Error whose `code` is the core's error code. */
	request<T = unknown>(method: AllowedMethod, params?: Record<string, unknown>): Promise<T>;
	onEvent(listener: (event: CoreEvent) => void): () => void;
	onCoreStatus(listener: (status: CoreStatus) => void): () => void;
	getCoreStatus(): Promise<CoreStatus>;
	restartCore(): Promise<void>;
	loadSettings(): Promise<{ settings: Settings; tokensPersist: boolean }>;
	saveSettings(patch: Partial<Settings>): Promise<void>;
	/** This computer's IPv4 addresses, to type into the other computer when discovery does not find it. */
	localAddresses(): Promise<string[]>;
	/** Which window controls the platform needs (`darwin`, `win32`, ...). */
	platform: string;
}

declare global {
	interface Window {
		edgehop?: EdgeHopApi;
	}
}
