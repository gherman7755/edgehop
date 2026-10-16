import { join, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { coreFileName, locateCore } from '../src/main/core/locate';

describe('locateCore', () => {
	const root = resolve('/repo');

	it('names the binary per platform', () => {
		expect(coreFileName('win32')).toBe('edgehop-core.exe');
		expect(coreFileName('darwin')).toBe('edgehop-core');
	});

	it('prefers an explicit override, then the packaged resources, then a checkout build', () => {
		const everything = () => true;
		expect(locateCore({ platform: 'darwin', override: '/x/core', resourcesPath: '/res', repoRoot: root, exists: everything }).path).toBe(resolve('/x/core'));
		expect(locateCore({ platform: 'darwin', resourcesPath: '/res', repoRoot: root, exists: everything }).path).toBe(join('/res', 'core', 'edgehop-core'));
		expect(locateCore({ platform: 'darwin', repoRoot: root, exists: everything }).path).toBe(join(root, '.build', 'release', 'edgehop-core'));
	});

	it('uses the Rust build on Windows, release before debug', () => {
		const onlyDebug = (p: string) => p.includes(join('target', 'debug'));
		expect(locateCore({ platform: 'win32', repoRoot: root, exists: onlyDebug }).path).toBe(join(root, 'rust', 'target', 'debug', 'edgehop-core.exe'));
	});

	it('lists what it looked at when nothing is there', () => {
		const result = locateCore({ platform: 'darwin', repoRoot: root, exists: () => false });
		expect(result.path).toBeNull();
		expect(result.searched).toEqual([join(root, '.build', 'release', 'edgehop-core'), join(root, '.build', 'debug', 'edgehop-core')]);
	});
});
