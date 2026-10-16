import { type ChildProcess, spawn } from 'node:child_process';
import { EventEmitter } from 'node:events';
import type { CoreEvent, CoreInfo, CoreStatus } from '../../shared/types';
import { CoreClient } from './client';

export const SUPPORTED_IPC_VERSION = 1;

export interface SupervisorOptions {
	/** Finds the core binary; `null` path means it is not there. */
	locate(): { path: string | null; searched: string[] };
	/** Arguments for the binary (none for the real core; tests run a script through node). */
	args?: string[];
	/** How long the core gets to answer `core.hello` and to exit after `core.shutdown`. */
	startupTimeoutMs?: number;
	shutdownTimeoutMs?: number;
}

/**
 * Runs the core as a child process and keeps its client. Emits `status` (CoreStatus), `event` (CoreEvent) and
 * `stderr` (one line of the core's diagnostics).
 *
 * The core ends all sessions and gives the cursor back when its stdin closes, so however this process dies the
 * input is released; `stop` also asks politely first.
 */
export class CoreSupervisor extends EventEmitter {
	private child: ChildProcess | null = null;
	private client: CoreClient | null = null;
	private current: CoreStatus = { state: 'starting' };
	private generation = 0;

	constructor(private readonly options: SupervisorOptions) {
		super();
	}

	get status(): CoreStatus {
		return this.current;
	}

	/** The live client, or null while the core is not running. */
	get active(): CoreClient | null {
		return this.current.state === 'running' ? this.client : null;
	}

	private setStatus(status: CoreStatus): void {
		this.current = status;
		this.emit('status', status);
	}

	/** Starts the core (stopping a previous one first) and waits for the handshake. */
	async start(): Promise<void> {
		await this.stop();
		const generation = ++this.generation;
		this.setStatus({ state: 'starting' });

		const { path, searched } = this.options.locate();
		if (!path) {
			this.setStatus({
				state: 'exited',
				message: `The EdgeHop core was not found. Looked in:\n${searched.join('\n')}\nBuild it first (see the README) or set EDGEHOP_CORE.`,
			});
			return;
		}

		let child: ChildProcess;
		try {
			child = spawn(path, this.options.args ?? [], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
		} catch (error) {
			this.setStatus({ state: 'exited', message: `Could not start the core: ${String(error)}` });
			return;
		}
		this.child = child;
		let stderrBuffer = '';
		child.stderr?.setEncoding('utf8');
		child.stderr?.on('data', (text: string) => {
			stderrBuffer += text;
			const lines = stderrBuffer.split('\n');
			stderrBuffer = lines.pop() ?? '';
			for (const line of lines) if (line.trim()) this.emit('stderr', line);
		});
		child.on('error', error => {
			if (generation === this.generation) this.fail(`Could not start the core: ${error.message}`);
		});
		child.on('exit', (code, signal) => {
			if (generation !== this.generation) return;
			this.fail(signal ? `The core was stopped (${signal}).` : `The core exited unexpectedly (code ${code ?? '?'}).`);
		});

		if (!child.stdin || !child.stdout) {
			this.fail('The core has no input or output.');
			return;
		}
		const client = new CoreClient(child.stdin, child.stdout);
		this.client = client;
		client.on('event', (event: CoreEvent) => {
			if (generation === this.generation) this.emit('event', event);
		});
		client.on('badline', (line: string) => this.emit('stderr', `unexpected output from the core: ${line}`));

		try {
			const info = await client.request<CoreInfo>('core.hello', undefined, this.options.startupTimeoutMs ?? 10_000);
			if (generation !== this.generation) return;
			if (info.ipcVersion !== SUPPORTED_IPC_VERSION) {
				this.fail(`This core speaks IPC version ${info.ipcVersion}; the UI needs version ${SUPPORTED_IPC_VERSION}.`);
				return;
			}
			this.setStatus({ state: 'running', info });
		} catch (error) {
			if (generation === this.generation) this.fail(`The core did not start properly: ${(error as Error).message}`);
		}
	}

	private fail(message: string): void {
		if (this.current.state === 'exited') return;
		this.client?.dispose();
		this.client = null;
		this.child?.kill();
		this.child = null;
		this.setStatus({ state: 'exited', message });
	}

	/** Asks the core to shut down, then makes sure it is gone. Safe to call at any time. */
	async stop(): Promise<void> {
		const child = this.child;
		const client = this.client;
		this.generation++; // late events from the old process are ignored
		this.child = null;
		this.client = null;
		if (!child) return;
		const exited = new Promise<void>(done => {
			if (child.exitCode !== null || child.signalCode !== null) done();
			else child.once('exit', () => done());
		});
		try {
			if (client && !client.isClosed) await client.request('core.shutdown', undefined, 1_000).catch(() => undefined);
			child.stdin?.end();
		} catch {
			// the pipe is already gone
		}
		const timeout = new Promise<void>(done => setTimeout(done, this.options.shutdownTimeoutMs ?? 3_000).unref());
		await Promise.race([exited, timeout]);
		client?.dispose();
		if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
	}
}
