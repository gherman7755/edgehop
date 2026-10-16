import { contextBridge, ipcRenderer } from 'electron';
import type { CoreErrorInfo, CoreEvent, CoreStatus, EdgeHopApi } from '../shared/types';

type Reply<T> = { ok: true; result: T } | { ok: false; error: CoreErrorInfo };

const api: EdgeHopApi = {
	async request<T>(method: string, params?: Record<string, unknown>): Promise<T> {
		const reply = (await ipcRenderer.invoke('core:request', method, params)) as Reply<T>;
		if (reply.ok) return reply.result;
		throw Object.assign(new Error(reply.error.message), { code: reply.error.code });
	},
	onEvent(listener) {
		const handler = (_: unknown, event: CoreEvent): void => listener(event);
		ipcRenderer.on('core:event', handler);
		return () => ipcRenderer.removeListener('core:event', handler);
	},
	onCoreStatus(listener) {
		const handler = (_: unknown, status: CoreStatus): void => listener(status);
		ipcRenderer.on('core:status', handler);
		return () => ipcRenderer.removeListener('core:status', handler);
	},
	getCoreStatus: () => ipcRenderer.invoke('core:get-status'),
	restartCore: () => ipcRenderer.invoke('core:restart'),
	loadSettings: () => ipcRenderer.invoke('settings:load'),
	saveSettings: patch => ipcRenderer.invoke('settings:save', patch),
	localAddresses: () => ipcRenderer.invoke('net:addresses'),
	platform: process.platform,
};

contextBridge.exposeInMainWorld('edgehop', api);
