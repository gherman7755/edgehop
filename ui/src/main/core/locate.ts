import { existsSync } from 'node:fs';
import { join, resolve } from 'node:path';

export interface LocateOptions {
	platform: NodeJS.Platform;
	/** `EDGEHOP_CORE`, if set. */
	override?: string | undefined;
	/** `process.resourcesPath` in a packaged app. */
	resourcesPath?: string | undefined;
	/** The repository root, for running from a checkout. */
	repoRoot: string;
	exists?: (path: string) => boolean;
}

/** File name of the core binary on this platform. */
export function coreFileName(platform: NodeJS.Platform): string {
	return platform === 'win32' ? 'edgehop-core.exe' : 'edgehop-core';
}

/**
 * Where the core binary is, in order: an explicit override, the packaged app's resources, then the build output
 * of a checkout (Swift on macOS, Rust on Windows; release before debug). Returns every place that was looked at
 * so a missing core can be explained.
 */
export function locateCore(options: LocateOptions): { path: string | null; searched: string[] } {
	const exists = options.exists ?? existsSync;
	const file = coreFileName(options.platform);
	const candidates: string[] = [];
	if (options.override) candidates.push(resolve(options.override));
	if (options.resourcesPath) candidates.push(join(options.resourcesPath, 'core', file));
	if (options.platform === 'win32') {
		candidates.push(join(options.repoRoot, 'rust', 'target', 'release', file), join(options.repoRoot, 'rust', 'target', 'debug', file));
	} else {
		candidates.push(join(options.repoRoot, '.build', 'release', file), join(options.repoRoot, '.build', 'debug', file));
	}
	return { path: candidates.find(exists) ?? null, searched: candidates };
}
