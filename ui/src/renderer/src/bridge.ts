import type { EdgeHopApi } from '@shared/types';
import { createMockApi } from './mock';

let api: EdgeHopApi | null = null;

/** The preload bridge inside Electron; a simulated core when the page is opened in a plain browser (npm run preview:web). */
export function getApi(): EdgeHopApi {
	api ??= window.edgehop ?? createMockApi();
	return api;
}

export const isSimulated = (): boolean => window.edgehop === undefined;
