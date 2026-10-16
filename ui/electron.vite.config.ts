import react from '@vitejs/plugin-react';
import { defineConfig } from 'electron-vite';
import { resolve } from 'node:path';

export default defineConfig({
	main: {},
	preload: {},
	renderer: {
		resolve: {
			alias: { '@shared': resolve('src/shared') },
		},
		plugins: [react()],
	},
});
