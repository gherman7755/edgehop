import {
	DEFAULT_SETTINGS,
	type ControllerState,
	type CoreEvent,
	type CoreStatus,
	type Peer,
	type Permissions,
	type ReceiverState,
	type Settings,
} from '@shared/types';

export interface LogLine {
	id: number;
	time: number;
	source: 'controller' | 'receiver' | 'core' | 'ui';
	text: string;
}

export interface AppState {
	core: CoreStatus;
	controller: ControllerState;
	receiver: ReceiverState;
	latency: number | null;
	peers: Peer[];
	permissions: Permissions | null;
	settings: Settings;
	/** False until the saved settings arrived, so defaults are not shown (or saved) over them. */
	settingsLoaded: boolean;
	/** Whether tokens survive a restart on this computer. */
	tokensPersist: boolean;
	addresses: string[];
	log: LogLine[];
	nextLogId: number;
	/** The last error from a start request (the core answered with an error, so no state event follows). */
	startError: string | null;
}

export const MAX_LOG_LINES = 500;

export const initialState: AppState = {
	core: { state: 'starting' },
	controller: { state: 'idle' },
	receiver: { state: 'stopped' },
	latency: null,
	peers: [],
	permissions: null,
	settings: DEFAULT_SETTINGS,
	settingsLoaded: false,
	tokensPersist: true,
	addresses: [],
	log: [],
	nextLogId: 1,
	startError: null,
};

export type Action =
	| { type: 'core-status'; status: CoreStatus }
	| { type: 'event'; event: CoreEvent; now: number }
	| { type: 'settings-loaded'; settings: Settings; tokensPersist: boolean }
	| { type: 'settings-patch'; patch: Partial<Settings> }
	| { type: 'permissions'; permissions: Permissions }
	| { type: 'addresses'; addresses: string[] }
	| { type: 'peers'; peers: Peer[] }
	| { type: 'note'; text: string; now: number }
	| { type: 'start-error'; message: string | null }
	| { type: 'clear-log' };

function appendLog(state: AppState, source: LogLine['source'], text: string, now: number): Pick<AppState, 'log' | 'nextLogId'> {
	const line: LogLine = { id: state.nextLogId, time: now, source, text };
	const log = state.log.length >= MAX_LOG_LINES ? [...state.log.slice(state.log.length - MAX_LOG_LINES + 1), line] : [...state.log, line];
	return { log, nextLogId: state.nextLogId + 1 };
}

export function reducer(state: AppState, action: Action): AppState {
	switch (action.type) {
		case 'core-status': {
			if (action.status.state === 'exited') {
				// Nothing is running any more, whatever the last events said.
				return { ...state, core: action.status, controller: { state: 'idle' }, receiver: { state: 'stopped' }, latency: null };
			}
			return { ...state, core: action.status };
		}
		case 'event': {
			const { event } = action;
			switch (event.event) {
				case 'controller.state':
					return { ...state, controller: event.data, latency: event.data.state === 'idle' ? null : state.latency, startError: null };
				case 'receiver.state':
					return { ...state, receiver: event.data, startError: null };
				case 'latency':
					return { ...state, latency: event.data.ms };
				case 'peers.changed':
					return { ...state, peers: event.data.peers };
				case 'log':
					return { ...state, ...appendLog(state, event.data.source, event.data.line, action.now) };
			}
			return state;
		}
		case 'settings-loaded':
			return { ...state, settings: action.settings, settingsLoaded: true, tokensPersist: action.tokensPersist };
		case 'settings-patch':
			return { ...state, settings: { ...state.settings, ...action.patch } };
		case 'permissions':
			return { ...state, permissions: action.permissions };
		case 'addresses':
			return { ...state, addresses: action.addresses };
		case 'peers':
			return { ...state, peers: action.peers };
		case 'note':
			return { ...state, ...appendLog(state, 'ui', action.text, action.now) };
		case 'start-error':
			return { ...state, startError: action.message };
		case 'clear-log':
			return { ...state, log: [] };
	}
}
