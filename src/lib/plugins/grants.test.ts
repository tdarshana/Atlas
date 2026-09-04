import { describe, expect, it } from 'vitest';
import {
	asked,
	asksForPermissions,
	holds,
	nextGrants,
	permissionChips
} from './grants';
import type { Manifest, Permission, PluginInfo } from './types';

function plugin(permissions: Permission[], granted: Permission[]): PluginInfo {
	const manifest: Manifest = {
		id: 'hello-world',
		name: 'Hello World',
		version: '1.0.0',
		description: 'd',
		author: 'a',
		api: '>=1.0 <2',
		main: 'main.js',
		permissions,
		contributes: { sections: [], themes: [], components: [], commands: [], tools: [] }
	};
	return {
		id: 'hello-world',
		manifest,
		enabled: true,
		compatible: true,
		reason: null,
		dir: '/p/hello-world',
		granted
	};
}

/** A plugin whose `atlas-plugin.json` could not be read at all. */
function manifestless(): PluginInfo {
	return {
		id: 'broken',
		manifest: null,
		enabled: false,
		compatible: false,
		reason: 'Could not read atlas-plugin.json',
		dir: '/p/broken',
		granted: []
	};
}

describe('grants', () => {
	it('reads the request off the manifest and the answer off the grants', () => {
		const p = plugin(['memories.read', 'tasks.write'], ['memories.read']);
		expect(asked(p)).toEqual(['memories.read', 'tasks.write']);
		expect(holds(p, 'memories.read')).toBe(true);
		expect(holds(p, 'tasks.write')).toBe(false);
		expect(asksForPermissions(p)).toBe(true);
	});

	it('keeps a revoked permission on the row, unticked', () => {
		const p = plugin(['memories.read', 'tasks.write'], ['memories.read']);
		expect(permissionChips(p)).toEqual([
			{ permission: 'memories.read', granted: true },
			{ permission: 'tasks.write', granted: false }
		]);
	});

	it('has nothing to show for a plugin that asks for nothing or has no manifest', () => {
		expect(permissionChips(plugin([], []))).toEqual([]);
		expect(asksForPermissions(plugin([], []))).toBe(false);
		expect(permissionChips(manifestless())).toEqual([]);
		expect(asksForPermissions(manifestless())).toBe(false);
	});

	it('builds the next grant list in the manifest order, never widening past it', () => {
		const p = plugin(['memories.read', 'tasks.write'], ['tasks.write']);
		expect(nextGrants(p, 'memories.read', true)).toEqual(['memories.read', 'tasks.write']);
		expect(nextGrants(p, 'tasks.write', false)).toEqual([]);

		// A grant the manifest no longer asks for cannot be written back.
		const shrunk = plugin(['memories.read'], ['memories.read', 'tasks.write']);
		expect(nextGrants(shrunk, 'memories.read', true)).toEqual(['memories.read']);
	});
});
