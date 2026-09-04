// @vitest-environment jsdom
// One document, one bridge, and who is allowed to move between them.
//
// Two properties pull against each other here. A page the plugin navigates its own frame
// to must never rebind, because a same-frame navigation keeps the same `Window` and the
// bridge accepts a message on window identity alone. But the host does change which
// document a frame shows: the section route reuses one `PluginFrame` across its params, so
// `/plugins/a/main` -> `/plugins/b/main` and `/plugins/a/main` -> `/plugins/a/other` both
// land on a mounted component. The frame is keyed on `src` and `view` so the host's change
// throws the element away, while a self-navigation, which moves neither, still cannot.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render } from '@testing-library/svelte';

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({}),
	daemon: { port: 7433, ready: true, error: null, logPath: '' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

/** Every bridge built during a test, in order, with the plugin and view it was built for. */
const built: { pluginId: string; view: string; disposed: boolean }[] = [];

vi.mock('./bridge', () => ({
	createBridge: (options: { plugin: { id: string }; view: string }) => {
		const record = { pluginId: options.plugin.id, view: options.view, disposed: false };
		built.push(record);
		return {
			handle: () => {},
			sendInit: () => {},
			sendTheme: () => {},
			sendContext: () => {},
			dispose: () => {
				record.disposed = true;
			}
		};
	}
}));

import PluginFrame from './PluginFrame.svelte';
import type { Manifest, PluginInfo } from './types';

function plugin(id: string): PluginInfo {
	const manifest: Manifest = {
		id,
		name: id,
		version: '1.0.0',
		description: 'd',
		author: 'a',
		api: '>=1.0 <2',
		main: 'main.js',
		permissions: ['ui.sections'],
		contributes: { sections: [], themes: [], components: [], commands: [], tools: [] }
	};
	return { id, manifest, enabled: true, compatible: true, reason: null, dir: `/p/${id}`, granted: ['ui.sections'] };
}

/** The frame element currently on screen. `src` resolves after `resolvePlatform`, so a
 * caller waits for it rather than reading straight after a render. */
async function frameFor(id: string): Promise<HTMLIFrameElement> {
	for (let i = 0; i < 50; i++) {
		const el = document.querySelector<HTMLIFrameElement>(`[data-testid="plugin-frame-${id}"]`);
		if (el) return el;
		await Promise.resolve();
		await new Promise((r) => setTimeout(r, 0));
	}
	throw new Error(`no frame for ${id}`);
}

/** What the bridge client posts the moment it runs, from `frame`'s own window. */
function hello(frame: HTMLIFrameElement): void {
	window.dispatchEvent(
		new MessageEvent('message', { data: { type: 'atlas:hello' }, source: frame.contentWindow })
	);
}

beforeEach(() => {
	built.length = 0;
});

afterEach(() => {
	cleanup();
	vi.restoreAllMocks();
});

describe('PluginFrame binding', () => {
	it('binds one bridge per document and refuses a second hello from the same window', () => {
		const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

		return (async () => {
			render(PluginFrame, { props: { plugin: plugin('alpha'), view: 'main' } });
			const frame = await frameFor('alpha');

			hello(frame);
			expect(built).toHaveLength(1);
			expect(built[0]).toMatchObject({ pluginId: 'alpha', view: 'main' });

			// The self-navigation case: the same `Window` says hello again.
			hello(frame);
			expect(built).toHaveLength(1);
			expect(warn).toHaveBeenCalledWith(
				'plugin alpha: refused a second atlas:hello from an already-bound frame'
			);
		})();
	});

	it('gives a new plugin a new element and a new bridge, and drops the old one', async () => {
		const { rerender } = render(PluginFrame, { props: { plugin: plugin('alpha'), view: 'main' } });
		const first = await frameFor('alpha');
		hello(first);
		expect(built).toHaveLength(1);

		await rerender({ plugin: plugin('beta'), view: 'main' });
		const second = await frameFor('beta');
		expect(second).not.toBe(first);
		expect(built[0].disposed).toBe(true);

		// The new document's hello is not refused: it is a different element and window.
		hello(second);
		expect(built).toHaveLength(2);
		expect(built[1]).toMatchObject({ pluginId: 'beta', view: 'main' });
		expect(built[1].disposed).toBe(false);
	});

	it('does the same for a view change, which never touches the URL', async () => {
		const { rerender } = render(PluginFrame, { props: { plugin: plugin('alpha'), view: 'main' } });
		const first = await frameFor('alpha');
		hello(first);
		expect(built).toHaveLength(1);

		// One document serves every view, so `src` is unchanged here; only the key moves.
		await rerender({ plugin: plugin('alpha'), view: 'other' });
		const second = await frameFor('alpha');
		expect(second).not.toBe(first);
		expect(built[0].disposed).toBe(true);

		hello(second);
		expect(built).toHaveLength(2);
		expect(built[1]).toMatchObject({ pluginId: 'alpha', view: 'other' });
	});
});
