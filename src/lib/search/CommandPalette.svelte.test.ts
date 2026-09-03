// @vitest-environment jsdom
// The palette component's own contract: the window event opens it, the same event puts
// it away again, and Escape closes it. The store's behaviour lives in
// `palette.svelte.test.ts`; this file only pins what the component does with events.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render } from '@testing-library/svelte';
import { tick } from 'svelte';

vi.mock('$app/navigation', () => ({ goto: vi.fn() }));
vi.mock('$app/state', () => ({ page: { url: new URL('http://localhost/') } }));

const mocks = vi.hoisted(() => ({ globalSearch: vi.fn(), listProjects: vi.fn() }));

vi.mock('$lib/daemon.svelte', () => ({
	api: () => mocks,
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import { PALETTE_EVENT } from '$lib/shell/shortcuts';
import CommandPalette from './CommandPalette.svelte';
import { closePalette, palette } from './palette.svelte';

/** The component opens on a tick, so give the render a chance to settle. */
async function settle(): Promise<void> {
	await tick();
	await tick();
}

function panel(): HTMLElement | null {
	return document.querySelector<HTMLElement>('[data-testid="command-palette"]');
}

beforeEach(() => {
	mocks.globalSearch.mockReset();
	mocks.listProjects.mockReset();
	mocks.listProjects.mockResolvedValue([]);
	localStorage.clear();
	closePalette();
});

afterEach(() => {
	closePalette();
	cleanup();
});

describe('CommandPalette', () => {
	it('stays closed until the palette event arrives', async () => {
		render(CommandPalette);
		expect(panel()).toBeNull();

		window.dispatchEvent(new CustomEvent(PALETTE_EVENT));
		await settle();

		expect(panel()).toBeTruthy();
		expect(palette.open).toBe(true);
	});

	it('closes on Escape', async () => {
		render(CommandPalette);
		window.dispatchEvent(new CustomEvent(PALETTE_EVENT));
		await settle();

		await fireEvent.keyDown(panel()!, { key: 'Escape' });
		await settle();

		expect(panel()).toBeNull();
		expect(palette.open).toBe(false);
	});

	it('treats a second palette event as a toggle', async () => {
		render(CommandPalette);
		window.dispatchEvent(new CustomEvent(PALETTE_EVENT));
		await settle();
		expect(panel()).toBeTruthy();

		window.dispatchEvent(new CustomEvent(PALETTE_EVENT));
		await settle();

		expect(panel()).toBeNull();
		expect(palette.open).toBe(false);
	});
});
