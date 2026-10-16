// A tiny stand-in for the core that speaks docs/IPC.md, for tests of the client and the supervisor.
//   node test/fake-core.mjs [--hello-version N] [--no-hello] [--die-after-hello]
import { createInterface } from 'node:readline';

const args = process.argv.slice(2);
const version = args.includes('--hello-version') ? Number(args[args.indexOf('--hello-version') + 1]) : 1;
const out = value => process.stdout.write(`${JSON.stringify(value)}\n`);

process.stderr.write('fake core ready\n');
const lines = createInterface({ input: process.stdin });
lines.on('line', line => {
	let message;
	try {
		message = JSON.parse(line);
	} catch {
		out({ id: null, error: { code: 'parse_error', message: 'line is not JSON' } });
		return;
	}
	const { id, method, params } = message;
	switch (method) {
		case 'core.hello':
			if (args.includes('--no-hello')) return;
			out({ id, result: { name: 'fake core', version: '0', ipcVersion: version, platform: 'test', deviceName: 'Fake' } });
			if (args.includes('--die-after-hello')) process.exit(3);
			break;
		case 'token.generate':
			out({ id, result: { token: 'ABCDEFGHJKLMNPQRSTUV' } });
			break;
		case 'controller.start':
			if (!params?.token || params.token.length < 12) {
				out({ id, error: { code: 'invalid_params', message: 'token must be at least 12 characters' } });
				break;
			}
			out({ event: 'controller.state', data: { state: 'connecting' } });
			out({ id, result: {} });
			out({ event: 'controller.state', data: { state: 'connected', peer: 'Fake Mac', remoteActive: false } });
			out({ event: 'log', data: { source: 'controller', line: 'connected' } });
			break;
		case 'core.shutdown':
			out({ id, result: {} });
			process.exit(0);
			break;
		default:
			out({ id, error: { code: 'unknown_method', message: `unknown method ${method}` } });
	}
});
lines.on('close', () => process.exit(0)); // stdin closed: the UI is gone
