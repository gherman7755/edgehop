import { describe, expect, it } from 'vitest';
import { EXIT_GRACE_SECONDS, watchdogCommand } from '../src/main/exit';

describe('watchdogCommand', () => {
	it('kills the process with sh after the grace period on macOS and Linux', () => {
		const { command, args } = watchdogCommand('darwin', 4242);
		expect(command).toBe('/bin/sh');
		expect(args).toEqual(['-c', `sleep ${EXIT_GRACE_SECONDS}; kill -9 4242 2>/dev/null`]);
	});

	it('kills it with taskkill after the grace period on Windows', () => {
		const { command, args } = watchdogCommand('win32', 4242);
		expect(command).toBe('cmd.exe');
		expect(args[1]).toContain(`ping -n ${EXIT_GRACE_SECONDS + 1} 127.0.0.1 >nul`);
		expect(args[1]).toContain('taskkill /F /PID 4242');
	});
});
