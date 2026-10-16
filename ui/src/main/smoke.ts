import type { BrowserWindow } from 'electron';

/**
 * `EDGEHOP_SMOKE=1`: a check of the whole app for CI. Waits for the window to show a running core, then asks the
 * page itself (through the preload bridge) for a token and reports what it saw. Resolves with a one-line report, or
 * rejects with what went wrong.
 */
export async function runSmoke(window: BrowserWindow, timeoutMs = 30_000): Promise<string> {
	const script = `(async () => {
		const api = window.edgehop;
		if (!api || typeof api.request !== 'function') return { ok: false, why: 'the preload bridge is missing' };
		const status = await api.getCoreStatus();
		if (status.state !== 'running') return { ok: false, why: 'core status: ' + JSON.stringify(status) };
		const { token } = await api.request('token.generate');
		const shown = document.querySelector('.device')?.textContent ?? '';
		const heading = document.querySelector('.status strong')?.textContent ?? '';
		const settings = await api.loadSettings();
		return { ok: token.length === 20 && shown === status.info.deviceName, token: token.length, device: shown, heading, platform: status.info.platform, mode: settings.settings.mode };
	})()`;
	const deadline = Date.now() + timeoutMs;
	let last = 'the page did not load';
	while (Date.now() < deadline) {
		if (!window.isDestroyed() && !window.webContents.isLoading()) {
			try {
				const result = (await window.webContents.executeJavaScript(script)) as { ok: boolean; why?: string };
				if (result.ok) return `SMOKE OK ${JSON.stringify(result)}`;
				last = result.why ?? JSON.stringify(result);
			} catch (error) {
				last = String(error);
			}
		}
		await new Promise(done => setTimeout(done, 300));
	}
	throw new Error(`SMOKE FAILED: ${last}`);
}
