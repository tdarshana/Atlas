// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { resolvePlatform } from './platform';
import {
	RAIL_KEY,
	SIDEPANEL_KEY,
	THEME_KEY,
	setTheme,
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

describe('resolvePlatform', () => {
	it('is mac outside Tauri', async () => {
		await expect(resolvePlatform()).resolves.toBe('mac');
	});
});
