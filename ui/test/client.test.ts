import { PassThrough } from 'node:stream';
import { describe, expect, it } from 'vitest';
import { CoreClient, CoreError } from '../src/main/core/client';

function setup(timeoutMs?: number) {
	const toCore = new PassThrough();
	const fromCore = new PassThrough();
	const client = new CoreClient(toCore, fromCore, timeoutMs);
	const written: string[] = [];
	toCore.on('data', chunk => written.push(...String(chunk).split('\n').filter(Boolean)));
	const reply = (value: unknown) => fromCore.write(`${JSON.stringify(value)}\n`);
	return { client, toCore, fromCore, written, reply };
}

describe('CoreClient', () => {
	it('sends a request line and resolves with the result', async () => {
		const { client, written, reply } = setup();
		const answer = client.request('token.generate');
		await new Promise(r => setImmediate(r));
		expect(JSON.parse(written[0]!)).toEqual({ id: 1, method: 'token.generate' });
		reply({ id: 1, result: { token: 'x' } });
		await expect(answer).resolves.toEqual({ token: 'x' });
	});

	it('includes params and matches answers that arrive out of order', async () => {
		const { client, written, reply } = setup();
		const first = client.request('controller.state');
		const second = client.request('controller.start', { token: 't' });
		await new Promise(r => setImmediate(r));
		expect(JSON.parse(written[1]!)).toEqual({ id: 2, method: 'controller.start', params: { token: 't' } });
		reply({ id: 2, result: 'second' });
		reply({ id: 1, result: 'first' });
		await expect(Promise.all([first, second])).resolves.toEqual(['first', 'second']);
	});

	it('turns an error answer into a CoreError with the code', async () => {
		const { client, reply } = setup();
		const answer = client.request('controller.start', {});
		reply({ id: 1, error: { code: 'invalid_params', message: 'token must be at least 12 characters' } });
		await expect(answer).rejects.toMatchObject({ code: 'invalid_params', message: 'token must be at least 12 characters' });
		await expect(answer).rejects.toBeInstanceOf(CoreError);
	});

	it('delivers events, also between a request and its answer, and ignores unknown ones', async () => {
		const { client, reply } = setup();
		const events: unknown[] = [];
		client.on('event', event => events.push(event));
		const answer = client.request('controller.start', {});
		reply({ event: 'controller.state', data: { state: 'connecting' } });
		reply({ event: 'something.new', data: {} });
		reply({ id: 1, result: {} });
		await answer;
		expect(events).toEqual([{ event: 'controller.state', data: { state: 'connecting' } }]);
	});

	it('reassembles lines split across chunks, including multi-byte characters', async () => {
		const { client, fromCore } = setup();
		const events: unknown[] = [];
		client.on('event', event => events.push(event));
		const bytes = Buffer.from('{"event":"log","data":{"source":"core","line":"Central’s Mac"}}\n{"event":"latency","data":{"ms":4}}\n');
		const cut = bytes.indexOf(0xe2) + 1; // inside the three bytes of ’
		fromCore.write(bytes.subarray(0, cut));
		fromCore.write(bytes.subarray(cut, cut + 20));
		fromCore.write(bytes.subarray(cut + 20));
		await new Promise(r => setImmediate(r));
		expect(events).toEqual([
			{ event: 'log', data: { source: 'core', line: 'Central’s Mac' } },
			{ event: 'latency', data: { ms: 4 } },
		]);
	});

	it('reports lines that are not protocol messages and keeps working', async () => {
		const { client, fromCore, reply } = setup();
		const bad: string[] = [];
		client.on('badline', line => bad.push(line));
		fromCore.write('not json\n[1,2]\n{"id":null,"error":{"code":"parse_error","message":"x"}}\n');
		const answer = client.request('core.hello');
		reply({ id: 1, result: 'ok' });
		await expect(answer).resolves.toBe('ok');
		expect(bad).toHaveLength(3);
	});

	it('fails pending requests when the core goes away, and refuses new ones', async () => {
		const { client, fromCore } = setup();
		let closed = 0;
		client.on('close', () => closed++);
		const answer = client.request('core.hello');
		fromCore.end();
		await expect(answer).rejects.toMatchObject({ code: 'closed' });
		await expect(client.request('core.hello')).rejects.toMatchObject({ code: 'closed' });
		expect(client.isClosed).toBe(true);
		expect(closed).toBe(1);
	});

	it('times out a request the core never answers', async () => {
		const { client } = setup(30);
		await expect(client.request('core.hello')).rejects.toMatchObject({ code: 'timeout' });
	});
});
