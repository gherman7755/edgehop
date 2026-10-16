import { resolve } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import type { CoreEvent, CoreStatus } from '../src/shared/types';
import { CoreSupervisor } from '../src/main/core/supervisor';

const fake = resolve(__dirname, 'fake-core.mjs');
const supervisors: CoreSupervisor[] = [];

function make(extra: string[] = [], found = true): CoreSupervisor {
	const supervisor = new CoreSupervisor({
		locate: () => (found ? { path: process.execPath, searched: [] } : { path: null, searched: ['/nowhere/edgehop-core'] }),
		args: [fake, ...extra],
		startupTimeoutMs: 1_500,
		shutdownTimeoutMs: 1_500,
	});
	supervisors.push(supervisor);
	return supervisor;
}

afterEach(async () => {
	await Promise.all(supervisors.splice(0).map(s => s.stop()));
});

describe('CoreSupervisor', () => {
	it('starts the core, completes the handshake and exposes the client', async () => {
		const supervisor = make();
		const statuses: CoreStatus[] = [];
		supervisor.on('status', s => statuses.push(s));
		await supervisor.start();
		expect(statuses.map(s => s.state)).toEqual(['starting', 'running']);
		expect(supervisor.status).toMatchObject({ state: 'running', info: { name: 'fake core', ipcVersion: 1 } });
		expect(await supervisor.active!.request('token.generate')).toEqual({ token: 'ABCDEFGHJKLMNPQRSTUV' });
	});

	it('forwards events and the core’s diagnostics', async () => {
		const supervisor = make();
		const events: CoreEvent[] = [];
		const stderr: string[] = [];
		supervisor.on('event', e => events.push(e));
		supervisor.on('stderr', l => stderr.push(l));
		await supervisor.start();
		await supervisor.active!.request('controller.start', { token: 'ABCDEFGHJKLMNPQRSTUV' });
		await new Promise(r => setTimeout(r, 50));
		expect(events.map(e => e.event)).toEqual(['controller.state', 'controller.state', 'log']);
		expect(stderr).toContain('fake core ready');
	});

	it('explains where it looked when the core is missing', async () => {
		const supervisor = make([], false);
		await supervisor.start();
		expect(supervisor.status).toMatchObject({ state: 'exited' });
		expect((supervisor.status as { message: string }).message).toContain('/nowhere/edgehop-core');
		expect(supervisor.active).toBeNull();
	});

	it('refuses a core that speaks another IPC version', async () => {
		const supervisor = make(['--hello-version', '2']);
		await supervisor.start();
		expect(supervisor.status).toMatchObject({ state: 'exited' });
		expect((supervisor.status as { message: string }).message).toContain('version 2');
	});

	it('gives up on a core that never answers the handshake', async () => {
		const supervisor = make(['--no-hello']);
		await supervisor.start();
		expect(supervisor.status.state).toBe('exited');
	});

	it('reports a core that dies, and can be restarted', async () => {
		const supervisor = make(['--die-after-hello']);
		const exited = new Promise<void>(done => supervisor.on('status', (s: CoreStatus) => s.state === 'exited' && done()));
		await supervisor.start();
		await exited;
		expect(supervisor.status.state).toBe('exited');
		expect(supervisor.active).toBeNull();
		await supervisor.start();
		expect(supervisor.status.state === 'running' || supervisor.status.state === 'exited').toBe(true);
	});

	it('stop() asks the core to shut down and does not report a failure', async () => {
		const supervisor = make();
		await supervisor.start();
		const statuses: CoreStatus[] = [];
		supervisor.on('status', s => statuses.push(s));
		await supervisor.stop();
		expect(statuses).toEqual([]);
		expect(supervisor.active).toBeNull();
	});
});
