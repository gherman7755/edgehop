import { spawn } from 'node:child_process';

/** How long a process that was told to exit gets before it is killed from outside. */
export const EXIT_GRACE_SECONDS = 3;

/** The command (and arguments) of a helper that kills `pid` after the grace period, on this platform. */
export function watchdogCommand(platform: NodeJS.Platform, pid: number): { command: string; args: string[] } {
	if (platform === 'win32') {
		// `ping -n 9` waits about eight seconds; `taskkill` ends the whole process.
		return { command: 'cmd.exe', args: ['/c', `ping -n ${EXIT_GRACE_SECONDS + 1} 127.0.0.1 >nul & taskkill /F /PID ${pid}`] };
	}
	return { command: '/bin/sh', args: ['-c', `sleep ${EXIT_GRACE_SECONDS}; kill -9 ${pid} 2>/dev/null`] };
}

/**
 * Starts a separate process that kills this one after [`EXIT_GRACE_SECONDS`] if it is still running: a safety net for
 * an exit that never finishes. Electron's main process has been seen on macOS to stay alive, idle, after `app.exit`
 * had returned (and a CI runner saw the same after `process.exit`), and nothing running inside the process can be relied
 * on to end it then, so the net lives outside. If the exit works the helper finds nothing to kill.
 */
export function armExitWatchdog(pid = process.pid, platform = process.platform): void {
	try {
		const { command, args } = watchdogCommand(platform, pid);
		spawn(command, args, { detached: true, stdio: 'ignore', windowsHide: true }).unref();
	} catch {
		// no shell to run it: the exit has to work on its own
	}
}
