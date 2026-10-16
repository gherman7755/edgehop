import { describe, expect, it } from 'vitest';
import { DEFAULT_SETTINGS } from '../src/shared/types';
import { connectBlocker, isRunning, keyboardSupported, statusView } from '../src/renderer/src/derive';
import { initialState, MAX_LOG_LINES, reducer, type AppState } from '../src/renderer/src/state';

const running = (state: AppState = initialState): AppState => ({
	...state,
	core: { state: 'running', info: { name: 'EdgeHop core', version: '0.1.0', ipcVersion: 1, platform: 'macos', deviceName: 'Mac' } },
});

describe('reducer', () => {
	it('follows controller and receiver state events and clears the latency when idle', () => {
		let state = reducer(running(), { type: 'event', event: { event: 'controller.state', data: { state: 'connected', peer: 'PC', remoteActive: true } }, now: 1 });
		state = reducer(state, { type: 'event', event: { event: 'latency', data: { ms: 4 } }, now: 2 });
		expect(state.controller).toEqual({ state: 'connected', peer: 'PC', remoteActive: true });
		expect(state.latency).toBe(4);
		state = reducer(state, { type: 'event', event: { event: 'controller.state', data: { state: 'idle' } }, now: 3 });
		expect(state.latency).toBeNull();
		state = reducer(state, { type: 'event', event: { event: 'receiver.state', data: { state: 'listening', port: 47474 } }, now: 4 });
		expect(state.receiver).toEqual({ state: 'listening', port: 47474 });
	});

	it('shows the warning of a peer that cannot apply input only for the session it came with', () => {
		let state = reducer(running(), { type: 'event', event: { event: 'controller.state', data: { state: 'connecting' } }, now: 1 });
		state = reducer(state, { type: 'event', event: { event: 'controller.warning', data: { message: 'Mac cannot apply input' } }, now: 2 });
		state = reducer(state, { type: 'event', event: { event: 'controller.state', data: { state: 'connected', peer: 'Mac', remoteActive: false } }, now: 3 });
		expect(state.peerWarning).toBe('Mac cannot apply input');
		state = reducer(state, { type: 'event', event: { event: 'controller.state', data: { state: 'connected', peer: 'Mac', remoteActive: true } }, now: 4 });
		expect(state.peerWarning).toBe('Mac cannot apply input');
		state = reducer(state, { type: 'event', event: { event: 'controller.state', data: { state: 'idle' } }, now: 5 });
		expect(state.peerWarning).toBeNull();
	});

	it('resets everything to idle when the core exits', () => {
		let state = reducer(running(), { type: 'event', event: { event: 'controller.state', data: { state: 'connected', peer: 'PC', remoteActive: false } }, now: 1 });
		state = reducer(state, { type: 'core-status', status: { state: 'exited', message: 'gone' } });
		expect(state.controller).toEqual({ state: 'idle' });
		expect(state.receiver).toEqual({ state: 'stopped' });
	});

	it('keeps at most MAX_LOG_LINES lines, newest last, with increasing ids', () => {
		let state = initialState;
		for (let i = 0; i < MAX_LOG_LINES + 25; i++) state = reducer(state, { type: 'note', text: `line ${i}`, now: i });
		expect(state.log).toHaveLength(MAX_LOG_LINES);
		expect(state.log.at(-1)?.text).toBe(`line ${MAX_LOG_LINES + 24}`);
		expect(state.log[0]?.text).toBe('line 25');
		expect(new Set(state.log.map(l => l.id)).size).toBe(MAX_LOG_LINES);
	});

	it('clears a start error when a state event arrives', () => {
		let state = reducer(initialState, { type: 'start-error', message: 'busy' });
		expect(state.startError).toBe('busy');
		state = reducer(state, { type: 'event', event: { event: 'controller.state', data: { state: 'connecting' } }, now: 1 });
		expect(state.startError).toBeNull();
	});
});

describe('statusView', () => {
	it('describes the core states first', () => {
		expect(statusView(initialState)).toMatchObject({ tone: 'busy' });
		expect(statusView({ ...initialState, core: { state: 'exited', message: 'not found' } })).toMatchObject({ tone: 'error', title: 'Core not running' });
	});

	it('describes the controller in each state', () => {
		const at = (controller: AppState['controller']) => statusView({ ...running(), controller });
		expect(at({ state: 'idle' }).tone).toBe('idle');
		expect(at({ state: 'connecting' }).tone).toBe('busy');
		expect(at({ state: 'connected', peer: 'PC', remoteActive: false })).toMatchObject({ tone: 'ok', title: 'Connected to PC' });
		expect(at({ state: 'connected', peer: 'PC', remoteActive: true })).toMatchObject({ tone: 'active', title: 'Controlling PC' });
		expect(at({ state: 'failed', message: 'rejected' })).toMatchObject({ tone: 'error', detail: 'rejected' });
	});

	it('describes the receiver when that mode is selected', () => {
		const base = { ...running(), settings: { ...DEFAULT_SETTINGS, mode: 'receiver' as const } };
		expect(statusView({ ...base, receiver: { state: 'listening', port: 47474 } })).toMatchObject({ tone: 'ok' });
		expect(statusView({ ...base, receiver: { state: 'connected', peer: 'Mac' } })).toMatchObject({ tone: 'active', title: 'Controlled by Mac' });
	});

	it('shows a start error while idle', () => {
		expect(statusView({ ...running(), startError: 'a controller session is already running' }).detail).toContain('already running');
	});
});

describe('isRunning and connectBlocker', () => {
	it('is running while connecting or connected (controller) or not stopped (receiver)', () => {
		expect(isRunning({ ...running(), controller: { state: 'connecting' } })).toBe(true);
		expect(isRunning({ ...running(), controller: { state: 'failed', message: 'x' } })).toBe(false);
		const receiver = { ...running(), settings: { ...DEFAULT_SETTINGS, mode: 'receiver' as const } };
		expect(isRunning({ ...receiver, receiver: { state: 'listening', port: 1 } })).toBe(true);
		expect(isRunning(receiver)).toBe(false);
	});

	it('asks for an address, then a token', () => {
		const token = 'ABCDEFGHJKLMN';
		expect(connectBlocker({ ...DEFAULT_SETTINGS, peerToken: token }, null, [])).toMatch(/address/);
		expect(connectBlocker({ ...DEFAULT_SETTINGS, host: 'me@pc', peerToken: token }, null, [])).toMatch(/without/);
		expect(connectBlocker({ ...DEFAULT_SETTINGS, host: '10.0.0.2', peerToken: 'short' }, null, [])).toMatch(/token/);
		expect(connectBlocker({ ...DEFAULT_SETTINGS, host: '10.0.0.2', peerToken: token }, null, [])).toBeNull();
	});

	it('accepts a discovered computer instead of an address, but only one that is still there', () => {
		const settings = { ...DEFAULT_SETTINGS, peerToken: 'ABCDEFGHJKLMN' };
		expect(connectBlocker(settings, 'Mac', [{ name: 'Mac' }])).toBeNull();
		expect(connectBlocker(settings, 'Mac', [])).toMatch(/address/);
	});
});

describe('keyboardSupported', () => {
	it('is true only where the core has the key map', () => {
		expect(keyboardSupported('macos')).toBe(true);
		expect(keyboardSupported('windows')).toBe(true);
		expect(keyboardSupported('linux')).toBe(false);
		expect(keyboardSupported(undefined)).toBe(false);
	});
});
