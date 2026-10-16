import type { Settings } from '@shared/types';
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react';
import { getApi } from './bridge';
import { connectBlocker } from './derive';
import { initialState, reducer } from './state';

const PERMISSION_POLL_MS = 1500;

/** Everything the screens need: the live state, the actions, and the wiring to the main process. */
export function useEdgeHop() {
	const api = useMemo(getApi, []);
	const [state, dispatch] = useReducer(reducer, initialState);
	const [selectedPeer, setSelectedPeer] = useState<string | null>(null);
	const latest = useRef(state);
	latest.current = state;

	const running = state.core.state === 'running';
	const platform = state.core.state === 'running' ? state.core.info.platform : undefined;
	const note = useCallback((text: string) => dispatch({ type: 'note', text, now: Date.now() }), []);

	// Subscriptions and the first load.
	useEffect(() => {
		const offEvent = api.onEvent(event => dispatch({ type: 'event', event, now: Date.now() }));
		const offStatus = api.onCoreStatus(status => dispatch({ type: 'core-status', status }));
		void api.getCoreStatus().then(status => dispatch({ type: 'core-status', status }));
		void api.loadSettings().then(({ settings, tokensPersist }) => dispatch({ type: 'settings-loaded', settings, tokensPersist }));
		void api.localAddresses().then(addresses => dispatch({ type: 'addresses', addresses }));
		return () => {
			offEvent();
			offStatus();
		};
	}, [api]);

	// Whenever the core is (re)started, ask it what is going on: events sent before this window listened are lost.
	useEffect(() => {
		if (!running) return;
		void api.request<never>('controller.state').then(data => dispatch({ type: 'event', event: { event: 'controller.state', data }, now: Date.now() }));
		void api.request<never>('receiver.state').then(data => dispatch({ type: 'event', event: { event: 'receiver.state', data }, now: Date.now() }));
	}, [api, running]);

	// This computer's own token, made once.
	useEffect(() => {
		if (!running || !state.settingsLoaded || state.settings.ownToken !== '') return;
		void api.request<{ token: string }>('token.generate').then(({ token }) => {
			dispatch({ type: 'settings-patch', patch: { ownToken: token } });
			void api.saveSettings({ ownToken: token });
		});
	}, [api, running, state.settingsLoaded, state.settings.ownToken]);

	// macOS asks for permissions; poll until they are granted (granting happens outside this app).
	const permissionsGranted = state.permissions?.accessibility === true && state.permissions.inputMonitoring === true;
	useEffect(() => {
		if (!running || permissionsGranted) return;
		const poll = (): void => {
			if (document.visibilityState === 'hidden') return;
			void api.request<{ accessibility: boolean; inputMonitoring: boolean }>('permissions.get').then(permissions => dispatch({ type: 'permissions', permissions }));
		};
		poll();
		const timer = setInterval(poll, PERMISSION_POLL_MS);
		return () => clearInterval(timer);
	}, [api, running, permissionsGranted]);

	// Look for computers nearby while choosing one to control (where the core can; others answer and do nothing).
	const looking = running && state.settings.mode === 'controller';
	useEffect(() => {
		if (!looking) return;
		void api.request('peers.watch', { enabled: true }).catch(() => undefined);
		return () => {
			void api.request('peers.watch', { enabled: false }).catch(() => undefined);
		};
	}, [api, looking]);

	// Keep the chosen nearby computer valid, and pick the first one if nothing else was typed.
	useEffect(() => {
		const names = state.peers.map(p => p.name);
		setSelectedPeer(current => {
			if (current !== null && names.includes(current)) return current;
			return state.settings.host.trim() === '' ? (names[0] ?? null) : null;
		});
	}, [state.peers, state.settings.host]);

	const updateSettings = useCallback(
		(patch: Partial<Settings>) => {
			dispatch({ type: 'settings-patch', patch });
			void api.saveSettings(patch);
		},
		[api],
	);

	// `connect` reads the selection through a ref so the actions object stays stable.
	const selectedPeerRef = useRef<string | null>(null);
	selectedPeerRef.current = selectedPeer;

	const fail = useCallback((error: unknown) => dispatch({ type: 'start-error', message: error instanceof Error ? error.message : String(error) }), []);

	const actions = useMemo(
		() => ({
			note,
			updateSettings,
			selectPeer: setSelectedPeer,
			clearLog: () => dispatch({ type: 'clear-log' }),
			async connect(): Promise<void> {
				const { settings, peers } = latest.current;
				const blocker = connectBlocker(settings, selectedPeerRef.current, peers);
				if (blocker) return fail(new Error(blocker));
				dispatch({ type: 'start-error', message: null });
				const target = selectedPeerRef.current !== null && peers.some(p => p.name === selectedPeerRef.current) ? { peer: selectedPeerRef.current } : { host: settings.host.trim() };
				try {
					await api.request('controller.start', {
						...target,
						token: settings.peerToken.trim(),
						side: settings.side,
						switching: settings.switching,
						forwardKeys: settings.forwardKeys,
						debug: settings.debug,
					});
				} catch (error) {
					fail(error);
				}
			},
			async disconnect(): Promise<void> {
				try {
					await api.request('controller.stop');
				} catch (error) {
					fail(error);
				}
			},
			async startReceiver(): Promise<void> {
				const { settings } = latest.current;
				dispatch({ type: 'start-error', message: null });
				try {
					await api.request('receiver.start', { token: settings.ownToken, allowKeys: settings.allowKeys, debug: settings.debug });
				} catch (error) {
					fail(error);
				}
			},
			async stopReceiver(): Promise<void> {
				try {
					await api.request('receiver.stop');
				} catch (error) {
					fail(error);
				}
			},
			async newToken(): Promise<void> {
				try {
					const { token } = await api.request<{ token: string }>('token.generate');
					updateSettings({ ownToken: token });
				} catch (error) {
					fail(error);
				}
			},
			async grant(kind: 'accessibility' | 'inputMonitoring'): Promise<void> {
				await api.request('permissions.request', { kind }).catch(fail);
			},
			restartCore: () => api.restartCore(),
		}),
		[api, fail, note, updateSettings],
	);

	return { state, selectedPeer, actions, platform, os: api.platform };
}

export type EdgeHop = ReturnType<typeof useEdgeHop>;
