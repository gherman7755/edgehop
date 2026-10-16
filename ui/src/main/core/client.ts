import { EventEmitter } from 'node:events';
import type { Readable, Writable } from 'node:stream';
import { StringDecoder } from 'node:string_decoder';
import type { CoreErrorInfo, CoreEvent } from '../../shared/types';

/** An error answer from the core, or `closed` / `timeout` from this client. */
export class CoreError extends Error implements CoreErrorInfo {
	constructor(
		readonly code: string,
		message: string,
	) {
		super(message);
		this.name = 'CoreError';
	}
}

interface Pending {
	resolve(value: unknown): void;
	reject(error: CoreError): void;
	timer: NodeJS.Timeout;
}

const EVENT_NAMES = new Set(['controller.state', 'controller.warning', 'receiver.state', 'latency', 'log', 'peers.changed']);

/**
 * The UI side of docs/IPC.md over a pair of streams (the core's stdout and stdin): newline-delimited JSON,
 * requests matched to responses by `id`, events delivered as they come. Knows nothing about processes.
 *
 * Emits `event` (a CoreEvent), `badline` (text that was not valid protocol) and `close` (once, when the core's
 * output ends).
 */
export class CoreClient extends EventEmitter {
	private nextId = 1;
	private readonly pending = new Map<number, Pending>();
	private readonly decoder = new StringDecoder('utf8');
	private buffer = '';
	private closed = false;

	constructor(
		private readonly input: Writable,
		output: Readable,
		private readonly defaultTimeoutMs = 15_000,
	) {
		super();
		output.on('data', (chunk: Buffer | string) => this.onData(typeof chunk === 'string' ? chunk : this.decoder.write(chunk)));
		output.on('end', () => this.onClose());
		output.on('close', () => this.onClose());
		input.on('error', () => this.onClose()); // a dead pipe is reported through `close`
	}

	get isClosed(): boolean {
		return this.closed;
	}

	request<T = unknown>(method: string, params?: Record<string, unknown>, timeoutMs = this.defaultTimeoutMs): Promise<T> {
		if (this.closed) return Promise.reject(new CoreError('closed', 'The core is not running.'));
		const id = this.nextId++;
		return new Promise<T>((resolve, reject) => {
			const timer = setTimeout(() => {
				this.pending.delete(id);
				reject(new CoreError('timeout', `The core did not answer ${method} in time.`));
			}, timeoutMs);
			this.pending.set(id, { resolve: resolve as (value: unknown) => void, reject, timer });
			const message = params === undefined ? { id, method } : { id, method, params };
			this.input.write(`${JSON.stringify(message)}\n`, error => {
				if (error) this.settle(id, new CoreError('closed', 'The core is not running.'));
			});
		});
	}

	/** Stops listening and fails everything still waiting. */
	dispose(): void {
		this.onClose();
	}

	private settle(id: number, outcome: CoreError | { result: unknown }): void {
		const pending = this.pending.get(id);
		if (!pending) return;
		this.pending.delete(id);
		clearTimeout(pending.timer);
		if (outcome instanceof CoreError) pending.reject(outcome);
		else pending.resolve(outcome.result);
	}

	private onData(text: string): void {
		this.buffer += text;
		let newline = this.buffer.indexOf('\n');
		while (newline >= 0) {
			const line = this.buffer.slice(0, newline).trim();
			this.buffer = this.buffer.slice(newline + 1);
			if (line) this.onLine(line);
			newline = this.buffer.indexOf('\n');
		}
	}

	private onLine(line: string): void {
		let message: unknown;
		try {
			message = JSON.parse(line);
		} catch {
			this.emit('badline', line);
			return;
		}
		if (typeof message !== 'object' || message === null) {
			this.emit('badline', line);
			return;
		}
		const object = message as Record<string, unknown>;
		if (typeof object.event === 'string') {
			if (EVENT_NAMES.has(object.event)) this.emit('event', { event: object.event, data: object.data } as CoreEvent);
			return;
		}
		if (typeof object.id !== 'number') {
			this.emit('badline', line); // an answer to a line the core could not parse carries `id: null`
			return;
		}
		const error = object.error as { code?: unknown; message?: unknown } | undefined;
		if (error) {
			this.settle(object.id, new CoreError(String(error.code ?? 'internal'), String(error.message ?? 'The core reported an error.')));
		} else {
			this.settle(object.id, { result: object.result });
		}
	}

	private onClose(): void {
		if (this.closed) return;
		this.closed = true;
		for (const [id] of [...this.pending]) this.settle(id, new CoreError('closed', 'The core stopped.'));
		this.emit('close');
	}
}
