// Differential check: send the same requests to two cores (for example the Swift and the Rust one) and require
// identical responses, so a UI cannot tell them apart.
//   node scripts/ipc-conformance.mjs <core A> <core B>
// Only requests that start no session, open no socket and show no prompt. Fields that legitimately differ
// per machine or per call (device name, random token, real permission values) are normalised first.
import { spawn } from 'node:child_process';

const [binA, binB] = process.argv.slice(2);
if (!binA || !binB) {
	console.error('usage: node scripts/ipc-conformance.mjs <core A> <core B>');
	process.exit(2);
}

const token = 'ABCDEFGHJKLMNPQRSTUV';
const requests = [
	// Framing and request validation
	'not json at all',
	'[1,2,3]',
	'{"id":1}',
	'{"id":true,"method":"core.hello"}',
	'{"id":null,"method":"core.hello"}',
	'{"method":"core.hello"}',
	'{"id":2,"method":""}',
	'{"id":3,"method":7}',
	'{"id":4,"method":"core.hello","params":[1]}',
	'{"id":5,"method":"core.hello","params":null}',
	'{"id":"six","method":"nope.nothing"}',
	// Harmless methods
	'{"id":10,"method":"core.hello"}',
	'{"id":11,"method":"token.generate"}',
	'{"id":12,"method":"permissions.get"}',
	'{"id":13,"method":"controller.state"}',
	'{"id":14,"method":"receiver.state"}',
	'{"id":15,"method":"controller.stop"}',
	'{"id":16,"method":"receiver.stop"}',
	'{"id":17,"method":"peers.list"}',
	'{"id":18,"method":"permissions.request","params":{"kind":"camera"}}',
	'{"id":19,"method":"permissions.request","params":{}}',
	'{"id":20,"method":"peers.watch","params":{"enabled":1}}',
	// controller.start validation
	'{"id":30,"method":"controller.start","params":{}}',
	`{"id":31,"method":"controller.start","params":{"token":"short","host":"10.0.0.2"}}`,
	'{"id":32,"method":"controller.start","params":{"token":12345}}',
	`{"id":33,"method":"controller.start","params":{"token":"${token}"}}`,
	`{"id":34,"method":"controller.start","params":{"token":"${token}","host":"10.0.0.2","peer":"Mac"}}`,
	`{"id":35,"method":"controller.start","params":{"token":"${token}","host":"me@10.0.0.2"}}`,
	`{"id":36,"method":"controller.start","params":{"token":"${token}","host":"   "}}`,
	`{"id":37,"method":"controller.start","params":{"token":"${token}","host":"10.0.0.2","side":"up"}}`,
	`{"id":38,"method":"controller.start","params":{"token":"${token}","host":"10.0.0.2","port":70000}}`,
	`{"id":39,"method":"controller.start","params":{"token":"${token}","host":"10.0.0.2","port":true}}`,
	`{"id":40,"method":"controller.start","params":{"token":"${token}","host":"10.0.0.2","port":1.5}}`,
	`{"id":41,"method":"controller.start","params":{"token":"${token}","host":"10.0.0.2","switching":1}}`,
	`{"id":42,"method":"controller.start","params":{"token":"${token}","host":"10.0.0.2","forwardKeys":"yes"}}`,
	`{"id":43,"method":"controller.start","params":{"token":"${token}","host":"10.0.0.2","debug":null}}`,
	`{"id":44,"method":"controller.start","params":{"token":"${token}","peer":"Nobody"}}`,
	// receiver.start validation
	'{"id":50,"method":"receiver.start","params":{}}',
	'{"id":51,"method":"receiver.start","params":{"token":"short"}}',
	`{"id":52,"method":"receiver.start","params":{"token":"${token}","port":-1}}`,
	`{"id":53,"method":"receiver.start","params":{"token":"${token}","port":65536}}`,
	`{"id":54,"method":"receiver.start","params":{"token":"${token}","allowKeys":"no"}}`,
	`{"id":55,"method":"receiver.start","params":{"token":"${token}","debug":0}}`,
];

function run(bin) {
	return new Promise((resolve, reject) => {
		const child = spawn(bin, [], { stdio: ['pipe', 'pipe', 'ignore'] });
		let out = '';
		child.stdout.on('data', chunk => { out += chunk; });
		child.on('error', reject);
		child.on('close', () => resolve(out.split('\n').filter(Boolean)));
		child.stdin.end(requests.join('\n') + '\n');
	});
}

// Per-machine / per-call values are replaced by their shape; everything else must match exactly.
function normalise(line) {
	let message;
	try {
		message = JSON.parse(line);
	} catch {
		return { notJson: line }; // a core must only ever print JSON on stdout
	}
	const result = message.result;
	if (result?.deviceName !== undefined) result.deviceName = '<device name>';
	if (result?.token !== undefined) result.token = `<token of ${result.token.length} chars>`;
	if (result?.accessibility !== undefined) {
		result.accessibility = typeof result.accessibility;
		result.inputMonitoring = typeof result.inputMonitoring;
	}
	return message;
}

const [a, b] = await Promise.all([run(binA), run(binB)]);
let failures = 0;
if (a.length !== b.length) {
	console.log(`FAIL different number of responses: ${a.length} vs ${b.length}`);
	failures += 1;
}
for (let i = 0; i < Math.min(a.length, b.length); i += 1) {
	const left = JSON.stringify(normalise(a[i]));
	const right = JSON.stringify(normalise(b[i]));
	if (left !== right) {
		failures += 1;
		console.log(`FAIL response ${i + 1}\n  A: ${left}\n  B: ${right}`);
	}
}
console.log(failures === 0 ? `ALL IDENTICAL (${a.length} responses compared)` : `${failures} DIFFERENCE(S)`);
process.exit(failures === 0 ? 0 : 1);
