// @vitest-environment jsdom
// The Import theme button flips to "Importing…" while the native open dialog is up.
// Pins that a cancelled dialog (`open()` resolving null) puts the button back to
// "Import theme…" rather than leaving it stuck, per the Task 1 review fix round.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, waitFor } from '@testing-library/svelte';
import type { Settings } from '$lib/types';

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({
		getSettings: async () => ({}) as Settings,
		setSettings: async (partial: Settings) => partial,
		boardStages: async () => ({ stages: [] }),
		mcpStatus: async () => null
	}),
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

let openDialog: (...args: unknown[]) => Promise<string | null>;
vi.mock('@tauri-apps/plugin-dialog', () => ({
	open: (...args: unknown[]) => openDialog(...args)
}));
vi.mock('@tauri-apps/api/path', () => ({
	downloadDir: async () => '/tmp'
}));
// Every other `desktop()` call the page's onMount reload makes (autostart, about,
// notifications, ...) goes through here once `__TAURI_INTERNALS__` is set; none of
// them matter to this test, so answer them all the same harmless way.
vi.mock('@tauri-apps/api/core', () => ({
	invoke: async () => undefined
}));

import Page from './+page.svelte';

afterEach(() => {
	cleanup();
	Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
	vi.restoreAllMocks();
});

describe('Import theme button', () => {
	it('returns to "Import theme…" when the open dialog is cancelled', async () => {
		// Simulate the Tauri desktop shell so `readThemeFile` takes the native-dialog path.
		(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
		let resolveOpen: (path: string | null) => void;
		openDialog = vi.fn(
			() =>
				new Promise<string | null>((resolve) => {
					resolveOpen = resolve;
				})
		);

		const { getByTestId } = render(Page);
		const button = getByTestId('appearance-import') as HTMLButtonElement;
		expect(button.textContent?.trim()).toBe('Import theme…');

		await fireEvent.click(button);
		await waitFor(() => expect(button.textContent?.trim()).toBe('Importing…'));
		expect(button.disabled).toBe(true);
		// The dynamic imports `readThemeFile` awaits before calling `open()` are still
		// pending right after the click; wait for the call itself before resolving it.
		await waitFor(() => expect(openDialog).toHaveBeenCalled());

		// The user cancelled the native picker: `open()` resolves null.
		resolveOpen!(null);

		await waitFor(() => expect(button.textContent?.trim()).toBe('Import theme…'));
		expect(button.disabled).toBe(false);
		expect(getByTestId('appearance-import')).not.toBeNull();
	});
});
