// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Component } from 'svelte';

import { resolvePlatform } from './platform';
import {
	RAIL_KEY,
	SIDEPANEL_DEFAULT,
	SIDEPANEL_KEY,
	SIDEPANEL_MAX,
	SIDEPANEL_MIN,
	SIDEPANEL_WIDTH_KEY,
	THEME_KEY,
	clearSidePanelOverride,
	setSidePanelOverride,
	setSidePanelWidth,
	setTheme,
	setView,
	shell,
	toggleRail,
	toggleSidePanel
} from './shell.svelte';

// setTheme mirrors the choice to the daemon; the store must not depend on one answering.
vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({ setSettings: vi.fn().mockResolvedValue({}) })
}));

describe('shell state', () => {
	beforeEach(() => {
		localStorage.clear();
		shell.railExpanded = false;
		shell.sidePanel = true;
		shell.theme = 'dark';
	});

	it('toggles the rail and remembers it', () => {
		toggleRail();
		expect(shell.railExpanded).toBe(true);
		expect(localStorage.getItem(RAIL_KEY)).toBe('expanded');

		toggleRail();
		expect(shell.railExpanded).toBe(false);
		expect(localStorage.getItem(RAIL_KEY)).toBe('collapsed');
	});

	it('toggles the side panel and remembers it', () => {
		toggleSidePanel();
		expect(shell.sidePanel).toBe(false);
		expect(localStorage.getItem(SIDEPANEL_KEY)).toBe('hidden');

		toggleSidePanel();
		expect(shell.sidePanel).toBe(true);
		expect(localStorage.getItem(SIDEPANEL_KEY)).toBe('shown');
	});

	it('setTheme paints the root element and remembers the choice', () => {
		setTheme('light');
		expect(document.documentElement.dataset.theme).toBe('light');
		expect(localStorage.getItem(THEME_KEY)).toBe('light');
		expect(shell.theme).toBe('light');
	});

	it('survives a localStorage that throws', () => {
		const setItem = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
			throw new Error('site data blocked');
		});
		expect(() => toggleRail()).not.toThrow();
		expect(shell.railExpanded).toBe(true);
		setItem.mockRestore();
	});
});

describe('side panel width', () => {
	it('clamps and persists a dragged or nudged width', () => {
		setSidePanelWidth(9000);
		expect(shell.sidePanelWidth).toBe(SIDEPANEL_MAX);
		expect(localStorage.getItem(SIDEPANEL_WIDTH_KEY)).toBe(String(SIDEPANEL_MAX));

		setSidePanelWidth(10);
		expect(shell.sidePanelWidth).toBe(SIDEPANEL_MIN);
		expect(localStorage.getItem(SIDEPANEL_WIDTH_KEY)).toBe(String(SIDEPANEL_MIN));

		setSidePanelWidth(260);
		expect(shell.sidePanelWidth).toBe(260);
		expect(localStorage.getItem(SIDEPANEL_WIDTH_KEY)).toBe('260');
	});

	it('clamps an out-of-range width already in storage when the module loads', async () => {
		localStorage.setItem(SIDEPANEL_WIDTH_KEY, '9000');
		vi.resetModules();
		const fresh = await import('./shell.svelte');
		expect(fresh.shell.sidePanelWidth).toBe(SIDEPANEL_MAX);
	});

	it('defaults on load when nothing is stored', async () => {
		localStorage.removeItem(SIDEPANEL_WIDTH_KEY);
		vi.resetModules();
		const fresh = await import('./shell.svelte');
		expect(fresh.shell.sidePanelWidth).toBe(SIDEPANEL_DEFAULT);
	});
});

describe('side panel override', () => {
	// The panel only needs something to render; the store never calls it.
	const stub = (() => {}) as unknown as Component;

	it('is set and cleared by the page that lends the panel a body', () => {
		expect(shell.sidePanelOverride).toBeNull();

		setSidePanelOverride({ title: 'Board filters', component: stub });
		expect(shell.sidePanelOverride?.title).toBe('Board filters');
		expect(shell.sidePanelOverride?.component).toBe(stub);

		clearSidePanelOverride();
		expect(shell.sidePanelOverride).toBeNull();
	});

	it('outlives a view change, so a move between two boards keeps the panel', () => {
		setSidePanelOverride({ title: 'Board filters', component: stub });
		setView('projects');
		expect(shell.sidePanelOverride?.title).toBe('Board filters');
		clearSidePanelOverride();
	});
});

describe('resolvePlatform', () => {
	it('is mac outside Tauri', async () => {
		await expect(resolvePlatform()).resolves.toBe('mac');
	});
});
