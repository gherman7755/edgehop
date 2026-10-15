// End-to-end check of the core IPC (docs/IPC.md) from a real client, the way the Electron UI will use it.
// Works against any core (Swift on macOS, Rust on Windows).
//   node scripts/ipc-smoke.mjs [path/to/edgehop-core] [--platform macos|windows] [--stub]
// --stub: the core has no session backend yet (the Rust core until its network layer exists), so starting a
// session must fail with `internal` instead of producing connection events.
// Starts no listener and needs no permissions: it uses an unreachable address to exercise events.
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';

const argv = process.argv.slice(2);
const option = name => {
	const i = argv.indexOf(name);
	return i >= 0 ? argv[i + 1] : undefined;
};
const platform = option('--platform') ?? 'macos';
const stub = argv.includes('--stub');
const bin = argv.find((a, i) => !a.startsWith('--') && argv[i - 1] !== '--platform') ?? '.build/debug/edgehop-core';
let failures = 0;
const check = (ok, label, extra = '') => {
	console.log(`${ok ? 'ok  ' : 'FAIL'} ${label}${ok || !extra ? '' : ` -> ${extra}`}`);
	if (!ok) failures += 1;
};

const child = spawn(bin, [], { stdio: ['pipe', 'pipe', 'inherit'] });
const events = [];
const pending = new Map();
let nextId = 1;
let exitCode = null;

createInterface({ input: child.stdout }).on('line', line => {
	let message;
	try {
		message = JSON.parse(line);
	} catch {
		check(false, 'every stdout line is JSON', line);
		return;
	}
	if (message.event) events.push(message);
	else pending.get(message.id)?.(message);
});
child.on('exit', code => { exitCode = code; });

const raw = text => child.stdin.write(text + '\n');
const request = (method, params) =>
	new Promise((resolve, reject) => {
		const id = nextId++;
		const timer = setTimeout(() => reject(new Error(`timeout waiting for ${method}`)), 5000);
		pending.set(id, message => { clearTimeout(timer); resolve(message); });
		raw(JSON.stringify({ id, method, params }));
	});
const until = async (predicate, ms) => {
	const end = Date.now() + ms;
	while (Date.now() < end) {
		if (predicate()) return true;
		await new Promise(r => setTimeout(r, 50));
	}
	return false;
};

try {
	const hello = await request('core.hello');
	check(hello.result?.ipcVersion === 1 && hello.result?.platform === platform, `core.hello (platform ${platform})`, JSON.stringify(hello));

	const token = await request('token.generate');
	check(token.result?.token?.length === 20, 'token.generate');

	const permissions = await request('permissions.get');
	check(typeof permissions.result?.accessibility === 'boolean' && typeof permissions.result?.inputMonitoring === 'boolean', 'permissions.get');

	const idle = await request('controller.state');
	check(idle.result?.state === 'idle', 'controller.state is idle');

	const unknown = await request('nope.nothing');
	check(unknown.error?.code === 'unknown_method', 'unknown method is reported');

	raw('this is not json');
	const afterGarbage = await request('core.hello');
	check(afterGarbage.result?.ipcVersion === 1, 'still serving after a malformed line');

	const invalid = await request('controller.start', { token: 'short', host: '192.0.2.1' });
	check(invalid.error?.code === 'invalid_params', 'controller.start validates params', JSON.stringify(invalid));

	if (stub) {
		// No session backend: starting must be refused cleanly, never hang or crash.
		const controller = await request('controller.start', { token: token.result.token, host: '192.0.2.1' });
		check(controller.error?.code === 'internal', 'controller.start is refused (no session backend)', JSON.stringify(controller));
		const receiver = await request('receiver.start', { token: token.result.token });
		check(receiver.error?.code === 'internal', 'receiver.start is refused (no session backend)', JSON.stringify(receiver));
		const state = await request('controller.state');
		check(state.result?.state === 'idle', 'controller stays idle after a refused start');
	} else {
		// Real session lifecycle without permissions: 192.0.2.1 (TEST-NET-1) never answers.
		const start = await request('controller.start', { token: token.result.token, host: '192.0.2.1', switching: false });
		check(start.result !== undefined && !start.error, 'controller.start accepted', JSON.stringify(start));
		check(await until(() => events.some(e => e.event === 'controller.state' && e.data.state === 'connecting'), 2000), 'event: controller.state connecting');
		const busy = await request('controller.start', { token: token.result.token, host: '192.0.2.1' });
		check(busy.error?.code === 'busy', 'second controller.start is busy', JSON.stringify(busy));
		check(await until(() => events.some(e => e.event === 'controller.state' && e.data.state === 'failed'), 15000), 'event: controller.state failed (after ~8 s)');
		const failed = events.find(e => e.event === 'controller.state' && e.data.state === 'failed');
		check(typeof failed?.data.message === 'string' && failed.data.message.length > 0, 'failure carries a readable message', failed?.data.message);
		check(events.some(e => e.event === 'log' && e.data.source === 'controller'), 'event: log lines arrive');
	}

	const shutdown = await request('core.shutdown');
	check(shutdown.result !== undefined, 'core.shutdown answered');
	check(await until(() => exitCode !== null, 3000) && exitCode === 0, 'process exits 0 after core.shutdown', `exit=${exitCode}`);
} catch (error) {
	check(false, 'no exception', String(error));
	child.kill();
}

// Second run: closing stdin (the UI died) must stop the core.
const orphan = spawn(bin, [], { stdio: ['pipe', 'ignore', 'ignore'] });
let orphanExit = null;
orphan.on('exit', code => { orphanExit = code; });
await new Promise(r => setTimeout(r, 300));
orphan.stdin.end();
check(await until(() => orphanExit !== null, 3000), 'process exits when stdin closes', `exit=${orphanExit}`);

console.log(failures === 0 ? '\nALL OK' : `\n${failures} FAILED`);
process.exit(failures === 0 ? 0 : 1);
