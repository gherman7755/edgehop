import type { AppState } from './state';
import type { Peer, Settings } from '@shared/types';

export type Tone = 'idle' | 'busy' | 'ok' | 'active' | 'error';

export interface StatusView {
	tone: Tone;
	title: string;
	detail: string;
}

const SIDE_WORD = { left: 'left', right: 'right', top: 'top', bottom: 'bottom' } as const;

/** What the header says, for the mode the user is looking at. */
export function statusView(state: AppState): StatusView {
	if (state.core.state === 'starting') return { tone: 'busy', title: 'Starting…', detail: 'Waiting for the EdgeHop core.' };
	if (state.core.state === 'exited') return { tone: 'error', title: 'Core not running', detail: 'EdgeHop cannot share anything until the core is running.' };
	if (state.settings.mode === 'controller') {
		const s = state.controller;
		switch (s.state) {
			case 'idle':
				return { tone: 'idle', title: 'Not connected', detail: state.startError ?? 'Connect to another computer to share this keyboard and mouse.' };
			case 'connecting':
				return { tone: 'busy', title: 'Connecting…', detail: 'Reaching the other computer.' };
			case 'connected':
				return s.remoteActive
					? { tone: 'active', title: `Controlling ${s.peer}`, detail: 'Your keyboard and mouse are on the other computer. Move back through the same edge to return.' }
					: { tone: 'ok', title: `Connected to ${s.peer}`, detail: `Move the cursor through the ${SIDE_WORD[state.settings.side]} edge of the screen.` };
			case 'failed':
				return { tone: 'error', title: 'Could not connect', detail: s.message };
		}
	}
	const s = state.receiver;
	switch (s.state) {
		case 'stopped':
			return { tone: 'idle', title: 'Not listening', detail: state.startError ?? 'Start listening so another computer can control this one.' };
		case 'listening':
			return { tone: 'ok', title: 'Waiting for a computer', detail: `Listening on port ${s.port}.` };
		case 'connected':
			return { tone: 'active', title: `Controlled by ${s.peer}`, detail: 'The other computer is sharing its keyboard and mouse with this one.' };
	}
}

/** A session is running or starting in the current mode (the start button becomes a stop button). */
export function isRunning(state: AppState): boolean {
	if (state.settings.mode === 'controller') return state.controller.state === 'connecting' || state.controller.state === 'connected';
	return state.receiver.state !== 'stopped';
}

export const MIN_TOKEN_LENGTH = 12;

/** Why Connect is not available yet, or null if it is. */
export function connectBlocker(settings: Settings, selectedPeer: string | null, peers: Peer[]): string | null {
	const hasPeer = selectedPeer !== null && peers.some(p => p.name === selectedPeer);
	if (!hasPeer && settings.host.trim() === '') return 'Enter the other computer’s address.';
	if (!hasPeer && settings.host.includes('@')) return 'Enter the address only, without “user@”.';
	if (settings.peerToken.trim().length < MIN_TOKEN_LENGTH) return `Enter the token shown on the other computer (at least ${MIN_TOKEN_LENGTH} characters).`;
	return null;
}

/** Keyboard sharing needs the neutral key map, which only some cores have so far. */
export function keyboardSupported(platform: string | undefined): boolean {
	return platform === 'macos';
}

export function formatLatency(ms: number | null): string | null {
	return ms === null ? null : `${Math.round(ms)} ms`;
}

export function formatTime(time: number): string {
	return new Date(time).toLocaleTimeString([], { hour12: false });
}
