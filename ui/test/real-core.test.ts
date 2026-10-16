// Runs the UI's client and supervisor against the real core binary when there is one (Swift on macOS, Rust on
// Windows; built by `swift build` / `cargo build -p edgehop-core`, or named by EDGEHOP_CORE). Skipped otherwise.
import { resolve } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import type { CoreEvent } from '../src/shared/types';
import { locateCore } from '../src/main/core/locate';
import { CoreSupervisor } from '../src/main/core/supervisor';

const located = locateCore({ platform: process.platform, override: process.env.EDGEHOP_CORE, repoRoot: resolve(__dirname, '..', '..') });
const supervisor = new CoreSupervisor({ locate: () => located });

afterAll(() => supervisor.stop());

describe.skipIf(!located.path)('the real core', () => {
	it('starts and answers core.hello', async () => {
		await supervisor.start();
		expect(supervisor.status).toMatchObject({ state: 'running', info: { ipcVersion: 1 } });
	});

	it('generates tokens and validates parameters like the documented API', async () => {
		const client = supervisor.active!;
		const { token } = await client.request<{ token: string }>('token.generate');
		expect(token).toHaveLength(20);
		await expect(client.request('controller.start', { token: 'short', host: '192.0.2.1' })).rejects.toMatchObject({ code: 'invalid_params' });
		await expect(client.request('nope.nothing')).rejects.toMatchObject({ code: 'unknown_method' });
		expect(await client.request('controller.state')).toEqual({ state: 'idle' });
		expect(await client.request('receiver.state')).toEqual({ state: 'stopped' });
	});

	it('reports a session starting as an event, and stops it again', async () => {
		const client = supervisor.active!;
		const states: string[] = [];
		const listener = (event: CoreEvent) => event.event === 'controller.state' && states.push(event.data.state);
		client.on('event', listener);
		const { token } = await client.request<{ token: string }>('token.generate');
		// 192.0.2.1 (TEST-NET-1) never answers; this only checks the lifecycle, and needs no permissions to start.
		await client.request('controller.start', { token, host: '192.0.2.1', switching: false });
		await client.request('controller.stop');
		client.off('event', listener);
		expect(states[0]).toBe('connecting');
		expect(await client.request('controller.state')).toMatchObject({ state: 'idle' });
	});

	it('exits cleanly when the UI asks it to', async () => {
		await supervisor.stop();
		expect(supervisor.active).toBeNull();
	});
});
