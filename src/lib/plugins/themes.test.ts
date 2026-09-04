// Contributed theme packs: read through the plugin's own folder, validated exactly as an
// imported pack is, and named after the manifest rather than the file.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { collectContributions } from './contributions';
import { loadPluginThemes } from './themes';
import type { Contributes, Manifest, PluginInfo } from './types';

function plugin(id: string, name: string, contributes: Partial<Contributes>): PluginInfo {
	const manifest: Manifest = {
		id,
		name,
		version: '1.0.0',
		description: 'd',
		author: 'a',
		api: '>=1.0 <2',
		main: 'main.js',
		// The manifest validator refuses a section, a component or a tool without its
		// permission, so a fixture that contributes any of them has to ask for all three.
		permissions: ['ui.sections', 'ui.components', 'mcp.tools'],
		contributes: { sections: [], themes: [], components: [], commands: [], tools: [], ...contributes }
	};
	return {
		id,
		manifest,
		enabled: true,
		compatible: true,
		reason: null,
		dir: `/plugins/${id}`,
		granted: [...manifest.permissions]
	};
}

const VALID = JSON.stringify({
	name: 'ignored',
	base: 'dark',
	tokens: { '--bg-base': '#12131A', '--accent': '#8B7BF6' }
});

const contribs = collectContributions([
	plugin('hello-world', 'Hello World', {
		themes: [
			{ id: 'hello-dusk', name: 'Hello dusk', file: 'theme.json' },
			{ id: 'broken', name: 'Broken', file: 'broken.json' }
		]
	})
]);

afterEach(() => vi.restoreAllMocks());

describe('loadPluginThemes', () => {
	it('labels a valid pack with the theme name and the plugin name', async () => {
		const read = vi.fn(async () => VALID);

		const packs = await loadPluginThemes(contribs, read);

		expect(packs.map((p) => p.name)).toEqual([
			'Hello dusk (Hello World)',
			'Broken (Hello World)'
		]);
		expect(packs[0].base).toBe('dark');
		expect(packs[0].tokens['--accent']).toBe('#8B7BF6');
		// Read out of the plugin's own folder, by the file the manifest names.
		expect(read.mock.calls).toEqual([
			['hello-world', 'theme.json'],
			['hello-world', 'broken.json']
		]);
	});

	it('skips a pack that fails validation, with a warning and nothing in the list', async () => {
		const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
		const read = async (_id: string, path: string) =>
			path === 'theme.json'
				? VALID
				: JSON.stringify({ base: 'dark', tokens: { '--not-a-token': '#fff' } });

		const packs = await loadPluginThemes(contribs, read);

		expect(packs.map((p) => p.name)).toEqual(['Hello dusk (Hello World)']);
		expect(warn).toHaveBeenCalledTimes(1);
		expect(String(warn.mock.calls[0][0])).toContain("'broken'");
	});

	it('skips a file that cannot be read or parsed', async () => {
		vi.spyOn(console, 'warn').mockImplementation(() => {});
		const read = async (_id: string, path: string) => {
			if (path === 'theme.json') throw new Error("'theme.json' was not found.");
			return 'not json at all';
		};

		expect(await loadPluginThemes(contribs, read)).toEqual([]);
	});

	it('answers with nothing when no plugin contributes a theme', async () => {
		const none = collectContributions([plugin('quiet', 'Quiet', {})]);
		const read = vi.fn(async () => VALID);

		expect(await loadPluginThemes(none, read)).toEqual([]);
		expect(read).not.toHaveBeenCalled();
	});
});
