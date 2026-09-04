// What `collectContributions` gathers: everything an enabled, compatible plugin
// contributes, nothing at all from a disabled one, and components grouped by slot.

import { describe, expect, it } from 'vitest';
import { collectContributions, sectionHref } from './contributions';
import type { Contributes, Manifest, PluginInfo } from './types';

function plugin(id: string, enabled: boolean, contributes: Partial<Contributes>): PluginInfo {
	const manifest: Manifest = {
		id,
		name: id,
		version: '1.0.0',
		description: 'd',
		author: 'a',
		api: '>=1.0 <2',
		main: 'main.js',
		permissions: [],
		contributes: {
			sections: [],
			themes: [],
			components: [],
			commands: [],
			tools: [],
			...contributes
		}
	};
	return { id, manifest, enabled, compatible: true, reason: null, dir: `/plugins/${id}` };
}

const everything = plugin('everything', true, {
	sections: [{ id: 'hello', title: 'Hello', icon: 'sparkles', view: 'hello-view' }],
	themes: [{ id: 'midnight', name: 'Midnight', file: 'midnight.css' }],
	components: [
		{ slot: 'dashboard.card', id: 'card', view: 'card-view' },
		{ slot: 'board.card.badge', id: 'badge', view: 'badge-view' },
		{ slot: 'dashboard.card', id: 'second-card', view: 'second-view' }
	],
	commands: [{ id: 'say', title: 'Say hello' }],
	tools: [{ name: 'hello', description: 'says hello', scope: 'read' }]
});

const disabled = plugin('quiet', false, {
	sections: [{ id: 'quiet', title: 'Quiet', icon: 'moon', view: 'quiet-view' }],
	components: [{ slot: 'dashboard.card', id: 'quiet-card', view: 'quiet-card' }],
	commands: [{ id: 'hush', title: 'Hush' }]
});

describe('collectContributions', () => {
	it('takes everything an enabled plugin contributes', () => {
		const out = collectContributions([everything, disabled]);

		expect(out.sections).toEqual([
			{
				id: 'hello',
				title: 'Hello',
				icon: 'sparkles',
				view: 'hello-view',
				pluginId: 'everything',
				pluginName: 'everything'
			}
		]);
		expect(out.themes.map((t) => t.id)).toEqual(['midnight']);
		expect(out.commands.map((c) => c.id)).toEqual(['say']);
		expect(out.tools.map((t) => t.name)).toEqual(['hello']);
	});

	it('contributes nothing from a disabled plugin', () => {
		const out = collectContributions([disabled]);

		expect(out.sections).toEqual([]);
		expect(out.commands).toEqual([]);
		expect(out.components).toEqual({});
	});

	it('contributes nothing from an incompatible plugin or one with no manifest', () => {
		const broken: PluginInfo = {
			id: 'broken',
			manifest: null,
			enabled: false,
			compatible: false,
			reason: 'Could not read atlas-plugin.json',
			dir: '/plugins/broken'
		};
		const tooNew = { ...everything, id: 'too-new', compatible: false };

		expect(collectContributions([broken, tooNew]).sections).toEqual([]);
	});

	it('groups components by slot and leaves an unfilled slot out', () => {
		const out = collectContributions([everything, disabled]);

		expect(out.components['dashboard.card']?.map((c) => c.id)).toEqual(['card', 'second-card']);
		expect(out.components['board.card.badge']?.map((c) => c.id)).toEqual(['badge']);
		expect(out.components['task.detail.panel']).toBeUndefined();
		expect(out.components['dashboard.card']?.every((c) => c.pluginId === 'everything')).toBe(true);
	});

	it('labels every ref with the plugin manifest name', () => {
		const named: PluginInfo = {
			...everything,
			manifest: { ...everything.manifest!, name: 'Everything Plugin' }
		};
		const out = collectContributions([named]);

		expect(out.commands[0].pluginName).toBe('Everything Plugin');
		expect(out.themes[0].pluginName).toBe('Everything Plugin');
		expect(out.components['dashboard.card']?.[0].pluginName).toBe('Everything Plugin');
	});

	it('routes a section to its own plugin view', () => {
		const [section] = collectContributions([everything]).sections;
		expect(sectionHref(section)).toBe('/plugins/everything/hello-view');
	});
});
