// The palette's plugin commands: one per contributed command, dispatched to the plugin's
// mounted frames, and what happens when the plugin has no frame on screen.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Bridge } from '$lib/plugins/bridge';
import { collectContributions } from '$lib/plugins/contributions';
import { dispatchCommand, registerFrame, unregisterFrame } from '$lib/plugins/host.svelte';
import type { Contributes, Manifest, PluginInfo } from '$lib/plugins/types';
import { clear, toasts } from '$lib/platform/toasts.svelte';
import { pluginCommands } from './commands';

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

const withSection = collectContributions([
	plugin('hello-world', 'Hello World', {
		sections: [{ id: 'hello', title: 'Hello', icon: 'sparkles', view: 'hello-view' }],
		commands: [
			{ id: 'say-hello', title: 'Say hello' },
			{ id: 'shout', title: 'Shout', combo: 'Mod+H' }
		]
	})
]);

const withoutSection = collectContributions([
	plugin('quiet', 'Quiet Plugin', { commands: [{ id: 'hush', title: 'Hush' }] })
]);

/** A bridge that records what was sent to it, which is all a dispatch touches. */
function fakeBridge(): Bridge & { sent: string[] } {
	const sent: string[] = [];
	let disposed = false;
	return {
		sent,
		get disposed() {
			return disposed;
		},
		handle: () => {},
		sendInit: () => {},
		sendTheme: () => {},
		sendContext: () => {},
		sendCommand: (id: string) => sent.push(id),
		callTool: async () => null,
		dispose: () => {
			disposed = true;
		}
	};
}

const ctx = () => ({ projectId: null, goto: vi.fn() });

beforeEach(clear);
afterEach(clear);

describe('pluginCommands', () => {
	it('makes one palette command per contributed command', () => {
		const commands = pluginCommands(withSection);

		expect(commands.map((c) => c.id)).toEqual([
			'plugin:hello-world:say-hello',
			'plugin:hello-world:shout'
		]);
		expect(commands.map((c) => c.label)).toEqual([
			'Hello World: Say hello',
			'Hello World: Shout'
		]);
		// A manifest combo is printed, never bound: nothing sets `combo`, which the palette
		// would render as a key hint.
		expect(commands[0].hint).toBe('plugin command');
		expect(commands[1].hint).toContain('inside the plugin');
		expect(commands.every((c) => c.combo === undefined)).toBe(true);
	});

	it('answers with nothing when no plugin contributes a command', () => {
		expect(pluginCommands(collectContributions([]))).toEqual([]);
	});

	it('sends the command to every mounted frame of that plugin', () => {
		const one = fakeBridge();
		const two = fakeBridge();
		const other = fakeBridge();
		registerFrame('hello-world', one);
		registerFrame('hello-world', two);
		registerFrame('quiet', other);

		const command = pluginCommands(withSection)[0];
		const context = ctx();
		command.run(context);

		expect(one.sent).toEqual(['say-hello']);
		expect(two.sent).toEqual(['say-hello']);
		expect(other.sent).toEqual([]);
		// The plugin heard it, so there is nothing to open.
		expect(context.goto).not.toHaveBeenCalled();

		unregisterFrame('hello-world', one);
		unregisterFrame('hello-world', two);
		unregisterFrame('quiet', other);
	});

	// A registration that outlived its frame must not count as a listener. The frame
	// component unregisters under the id it registered with, so this should never happen;
	// `dispatchCommand` refuses to count it anyway, because the cost of getting it wrong is
	// a command that silently does nothing instead of opening the plugin.
	it('falls back to the section when the only registered frame is disposed', () => {
		const stale = fakeBridge();
		registerFrame('hello-world', stale);
		stale.dispose();

		const command = pluginCommands(withSection)[0];
		const context = ctx();
		command.run(context);

		expect(stale.sent).toEqual([]);
		expect(context.goto).toHaveBeenCalledWith('/plugins/hello-world/hello-view');
		// And the stale entry is gone, so the next run does not have to rediscover it.
		expect(dispatchCommand('hello-world', 'say-hello')).toBe(0);

		// Belt and braces: a stale entry must never outlive its test, whatever prunes it.
		unregisterFrame('hello-world', stale);
	});

	it('still delivers to the live frames when one of several is disposed', () => {
		const live = fakeBridge();
		const stale = fakeBridge();
		registerFrame('hello-world', live);
		registerFrame('hello-world', stale);
		stale.dispose();

		expect(dispatchCommand('hello-world', 'say-hello')).toBe(1);
		expect(live.sent).toEqual(['say-hello']);
		expect(stale.sent).toEqual([]);

		unregisterFrame('hello-world', live);
	});

	it('opens the first section of the plugin when no frame is mounted', () => {
		const command = pluginCommands(withSection)[0];
		const context = ctx();

		command.run(context);

		expect(context.goto).toHaveBeenCalledWith('/plugins/hello-world/hello-view');
		expect(toasts).toEqual([]);
	});

	it('says to open the plugin when it has no frame and no section', () => {
		const command = pluginCommands(withoutSection)[0];
		const context = ctx();

		command.run(context);

		expect(context.goto).not.toHaveBeenCalled();
		expect(toasts.map((t) => t.text)).toEqual(['Open Quiet Plugin first']);
	});
});
