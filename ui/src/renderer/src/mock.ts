import type {
	AllowedMethod,
	ControllerState,
	CoreEvent,
	CoreInfo,
	CoreStatus,
	EdgeHopApi,
	ReceiverState,
	Settings,
} from '@shared/types';
import { DEFAULT_SETTINGS } from '@shared/types';

/**
 * A simulated core and main process for working on the screens in a browser. `?platform=windows` pretends to be
 * the Windows core (no permissions, no discovery); `window.__mock` lets a test drive events by hand.
 */
export function createMockApi(): EdgeHopApi {
	const query = new URLSearchParams(location.search);
	const platform = query.get('platform') ?? 'macos';
	const info: CoreInfo = { name: 'EdgeHop core (simulated)', version: '0.1.0', ipcVersion: 1, platform, deviceName: platform === 'windows' ? 'DESKTOP-7K2P' : 'Central’s MacBook Pro' };
	const status: CoreStatus = query.get('core') === 'exited' ? { state: 'exited', message: 'The EdgeHop core was not found. Looked in:\n/repo/.build/release/edgehop-core\n/repo/.build/debug/edgehop-core\nBuild it first (see the README) or set EDGEHOP_CORE.' } : { state: 'running', info };

	const eventListeners = new Set<(event: CoreEvent) => void>();
	const statusListeners = new Set<(status: CoreStatus) => void>();
	const emit = (event: CoreEvent): void => eventListeners.forEach(listener => listener(event));

	let controller: ControllerState = { state: 'idle' };
	let receiver: ReceiverState = { state: 'stopped' };
	let permissions = { accessibility: platform !== 'macos' || query.has('granted'), inputMonitoring: platform !== 'macos' || query.has('granted') };
	let latencyTimer: ReturnType<typeof setInterval> | undefined;
	const setController = (next: ControllerState): void => {
		controller = next;
		emit({ event: 'controller.state', data: next });
	};
	const setReceiver = (next: ReceiverState): void => {
		receiver = next;
		emit({ event: 'receiver.state', data: next });
	};
	const log = (source: 'controller' | 'receiver', line: string): void => emit({ event: 'log', data: { source, line } });

	const stored = ((): Settings => {
		try {
			return { ...DEFAULT_SETTINGS, ...(JSON.parse(localStorage.getItem('edgehop-mock-settings') ?? '{}') as Partial<Settings>) };
		} catch {
			return DEFAULT_SETTINGS;
		}
	})();

	const token = (): string => Array.from({ length: 20 }, () => 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789'[Math.floor(Math.random() * 32)]).join('');

	const handlers: Record<AllowedMethod, (params: Record<string, unknown>) => unknown> = {
		'core.hello': () => info,
		'permissions.get': () => permissions,
		'permissions.request': () => {
			permissions = { accessibility: true, inputMonitoring: true };
			return {};
		},
		'token.generate': () => ({ token: token() }),
		'peers.watch': params => {
			if (params.enabled && platform === 'macos' && query.has('peers')) {
				setTimeout(() => emit({ event: 'peers.changed', data: { peers: [{ name: 'DESKTOP-7K2P' }] } }), 600);
			}
			return {};
		},
		'peers.list': () => ({ peers: [] }),
		'controller.start': params => {
			if (controller.state === 'connecting' || controller.state === 'connected') throw Object.assign(new Error('a controller session is already running'), { code: 'busy' });
			const host = String(params.host ?? params.peer ?? '');
			setController({ state: 'connecting' });
			log('controller', `connecting to ${host}:47474…`);
			setTimeout(() => {
				if (controller.state !== 'connecting') return;
				if (host.includes('fail')) {
					setController({ state: 'failed', message: `could not reach ${host}:47474 within 8 s (timed out)` });
					return;
				}
				const peer = platform === 'macos' ? 'DESKTOP-7K2P' : 'Central’s MacBook Pro';
				setController({ state: 'connected', peer, remoteActive: false });
				log('controller', `connected to "${peer}"`);
				latencyTimer = setInterval(() => emit({ event: 'latency', data: { ms: 3 + Math.round(Math.random() * 6) } }), 2000);
				emit({ event: 'latency', data: { ms: 5 } });
			}, 900);
			return {};
		},
		'controller.stop': () => {
			clearInterval(latencyTimer);
			if (controller.state !== 'idle') setController({ state: 'idle' });
			return {};
		},
		'controller.state': () => controller,
		'receiver.start': () => {
			setReceiver({ state: 'listening', port: 47474 });
			log('receiver', 'listening on port 47474');
			return { port: 47474 };
		},
		'receiver.stop': () => {
			if (receiver.state !== 'stopped') setReceiver({ state: 'stopped' });
			return {};
		},
		'receiver.state': () => receiver,
	};

	(window as unknown as { __mock: unknown }).__mock = {
		emit,
		setController,
		setReceiver,
		setCore: (next: CoreStatus) => statusListeners.forEach(listener => listener(next)),
	};

	return {
		platform: query.get('os') ?? 'darwin',
		async request(method, params) {
			const handler = handlers[method];
			if (!handler) throw Object.assign(new Error(`unknown method ${method}`), { code: 'unknown_method' });
			await new Promise(done => setTimeout(done, 20));
			return handler(params ?? {}) as never;
		},
		onEvent(listener) {
			eventListeners.add(listener);
			return () => eventListeners.delete(listener);
		},
		onCoreStatus(listener) {
			statusListeners.add(listener);
			return () => statusListeners.delete(listener);
		},
		getCoreStatus: async () => status,
		restartCore: async () => undefined,
		loadSettings: async () => ({ settings: stored, tokensPersist: !query.has('nopersist') }),
		saveSettings: async patch => {
			Object.assign(stored, patch);
			localStorage.setItem('edgehop-mock-settings', JSON.stringify({ ...stored, peerToken: '', ownToken: '' }));
		},
		localAddresses: async () => ['192.168.0.128', '10.0.0.7'],
	};
}
