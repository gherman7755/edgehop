import { app } from 'electron';
import { join } from 'node:path';

/** A file from `ui/resources`, in a checkout and in a packaged app (where electron-builder copies the folder). */
export function resourcePath(name: string): string {
	return app.isPackaged ? join(process.resourcesPath, 'resources', name) : join(__dirname, '..', '..', 'resources', name);
}
