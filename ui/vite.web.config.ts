// Runs the renderer alone in a normal browser (with a simulated core, see src/renderer/src/mock.ts), to work on
// the screens without Electron: `npm run preview:web`.
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';
import { resolve } from 'node:path';

export default defineConfig({
	root: resolve('src/renderer'),
	resolve: { alias: { '@shared': resolve('src/shared') } },
	plugins: [react()],
	server: { port: 5199 },
});
