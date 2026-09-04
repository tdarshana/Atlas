// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';

const goto = vi.fn();
vi.mock('$app/navigation', () => ({ goto: (href: string) => goto(href) }));

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
			'/practices',
			'/workflows',
			'/review',
			'/mcp',
			'/permissions'
		];
		hrefs.forEach((href, i) => {
			press(String(i + 1));
			expect(goto).toHaveBeenLastCalledWith(href);
		});
		expect(goto).toHaveBeenCalledTimes(9);
	});

	it('maps Mod+0 to the tenth main view', () => {
		press('0');
		expect(goto).toHaveBeenCalledWith('/skills');
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
