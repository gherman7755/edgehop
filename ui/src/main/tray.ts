import { Menu, nativeImage, Tray } from 'electron';
import { resourcePath } from './resources';

export interface TrayState {
	status: string;
	/** A controller or receiver session is running or starting. */
	sharing: boolean;
	/** This computer's keyboard and mouse are driving the other one right now. */
	active: boolean;
}

export interface TrayHandle {
	update(state: TrayState): void;
}

/** The menu bar / notification area icon: shows what is going on and keeps EdgeHop reachable with the window closed. */
export function createTray(handlers: { onShow(): void; onStop(): void; onQuit(): void }): TrayHandle {
	const isMac = process.platform === 'darwin';
	const image = nativeImage.createFromPath(resourcePath(isMac ? 'trayTemplate.png' : 'tray.png'));
	if (isMac) image.setTemplateImage(true);
	const tray = new Tray(image);
	tray.on('click', handlers.onShow);

	return {
		update(state) {
			tray.setToolTip(`EdgeHop: ${state.status}`);
			tray.setContextMenu(
				Menu.buildFromTemplate([
					{ label: state.status, enabled: false },
					{ type: 'separator' },
					{ label: 'Show EdgeHop', click: handlers.onShow },
					{ label: 'Stop sharing', enabled: state.sharing, click: handlers.onStop },
					{ type: 'separator' },
					{ label: 'Quit EdgeHop', click: handlers.onQuit },
				]),
			);
		},
	};
}
