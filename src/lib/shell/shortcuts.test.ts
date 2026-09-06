// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';

const goto = vi.fn();
vi.mock('$app/navigation', () => ({ goto: (href: string) => goto(href) }));
const saveSettings = vi.fn(async (_partial: unknown) => {});
vi.mock('$lib/stores/settings.svelte', () => ({ saveSettings: (p: unknown) => saveSettings(p) }));

import { PALETTE_EVENT, installShortcuts } from './shortcuts';
import { shell } from './shell.svelte';

function press(key: string, target?: HTMLElement): KeyboardEvent {
	const e = new KeyboardEvent('keydown', { key, metaKey: true, bubbles: true, cancelable: true });
	(target ?? document.body).dispatchEvent(e);
	return e;
}

describe('installShortcuts', () => {
	let teardown: () => void;

	beforeEach(() => {
		goto.mockClear();
		localStorage.clear();
		shell.platform = 'mac';
		shell.railExpanded = false;
		teardown?.();
		teardown = installShortcuts();
	});

	it('maps Mod+1..9 to the first nine main views', () => {
		const hrefs = [
			'/',
			'/projects',
			'/memories',
			'/agents',
			'/workflows',
			'/review',
			'/mcp',
			'/permissions',
			'/skills'
		];
		hrefs.forEach((href, i) => {
			press(String(i + 1));
			expect(goto).toHaveBeenLastCalledWith(href);
		});
		expect(goto).toHaveBeenCalledTimes(9);
	});

	it('maps Mod+0 to the tenth main view', () => {
		press('0');
		expect(goto).toHaveBeenCalledWith('/personas');
	});

	it('maps Mod+, to settings', () => {
		press(',');
		expect(goto).toHaveBeenCalledWith('/settings');
	});

	it('maps Mod+B to the rail toggle', () => {
		press('b');
		expect(shell.railExpanded).toBe(true);
		press('b');
		expect(shell.railExpanded).toBe(false);
	});

	it('maps Mod+K to the palette event', () => {
		const seen = vi.fn();
		window.addEventListener(PALETTE_EVENT, seen);
		press('k');
		expect(seen).toHaveBeenCalledTimes(1);
		window.removeEventListener(PALETTE_EVENT, seen);
	});

	it('ignores keydown inside an input, except Mod+K', () => {
		const input = document.createElement('input');
		document.body.append(input);
		const seen = vi.fn();
		window.addEventListener(PALETTE_EVENT, seen);

		press('3', input);
		press('b', input);
		expect(goto).not.toHaveBeenCalled();
		expect(shell.railExpanded).toBe(false);

		press('k', input);
		expect(seen).toHaveBeenCalledTimes(1);

		window.removeEventListener(PALETTE_EVENT, seen);
		input.remove();
	});

	it('does nothing without the platform modifier', () => {
		document.body.dispatchEvent(new KeyboardEvent('keydown', { key: '2', bubbles: true }));
		expect(goto).not.toHaveBeenCalled();
	});

	it('stops listening after teardown', () => {
		teardown();
		press('2');
		expect(goto).not.toHaveBeenCalled();
		teardown = installShortcuts();
	});
});

describe('scale shortcuts', () => {
	let teardown: () => void;

	function pressShift(key: string, code: string, target?: HTMLElement): KeyboardEvent {
		const e = new KeyboardEvent('keydown', { key, code, metaKey: true, shiftKey: true, bubbles: true, cancelable: true });
		(target ?? document.body).dispatchEvent(e);
		return e;
	}

	beforeEach(() => {
		saveSettings.mockClear();
		document.documentElement.style.zoom = '';
		document.documentElement.style.removeProperty('--ui-zoom');
		shell.platform = 'mac';
		teardown?.();
		teardown = installShortcuts();
	});

	it('steps the UI scale up one stop on Mod+Shift+= and saves it', async () => {
		const e = pressShift('+', 'Equal');
		expect(e.defaultPrevented).toBe(true);
		expect(document.documentElement.style.zoom).toBe('1.1');
		await vi.waitFor(() => expect(saveSettings).toHaveBeenCalledWith({ 'ui.scale': 110 }));
	});

	it('steps the UI scale down one stop on Mod+Shift+-', async () => {
		document.documentElement.style.zoom = '1.1';
		pressShift('_', 'Minus');
		expect(document.documentElement.style.zoom).toBe('');
		await vi.waitFor(() => expect(saveSettings).toHaveBeenCalledWith({ 'ui.scale': 100 }));
	});

	it('stops at the last stop', () => {
		document.documentElement.style.zoom = '1.5';
		pressShift('+', 'Equal');
		expect(document.documentElement.style.zoom).toBe('1.5');
		expect(saveSettings).not.toHaveBeenCalled();
	});

	it('works while typing in a field', () => {
		const input = document.createElement('input');
		document.body.appendChild(input);
		pressShift('+', 'Equal', input);
		expect(document.documentElement.style.zoom).toBe('1.1');
		input.remove();
	});

	it('leaves other shifted combos alone', () => {
		const seen = vi.fn();
		window.addEventListener(PALETTE_EVENT, seen);
		const e = pressShift('K', 'KeyK');
		window.removeEventListener(PALETTE_EVENT, seen);
		expect(seen).not.toHaveBeenCalled();
		expect(e.defaultPrevented).toBe(false);
	});
});
