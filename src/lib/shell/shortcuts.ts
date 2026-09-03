// Window-level shortcuts. Mod is ⌘ on mac and Ctrl elsewhere, matching what the rail
// prints. Typing in a field must not navigate, so every combo but Mod+K is ignored while
// the focus is in an editable control; Mod+K opens the palette from anywhere.

import { goto } from '$app/navigation';
import { shell, toggleRail, toggleSidePanel } from './shell.svelte';
import { MAIN_VIEWS, SETTINGS_VIEW } from './views';

/** Task 5 listens for this on `window` and opens the command palette. */
export const PALETTE_EVENT = 'atlas:palette';

function isEditable(target: EventTarget | null): boolean {
	if (!(target instanceof HTMLElement)) return false;
	const tag = target.tagName;
	if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true;
	return target.isContentEditable;
}

function hasMod(e: KeyboardEvent): boolean {
	return shell.platform === 'mac' ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
}

export function handleKeydown(e: KeyboardEvent): void {
	if (!hasMod(e) || e.altKey) return;

	if (e.key.toLowerCase() === 'k') {
		e.preventDefault();
		window.dispatchEvent(new CustomEvent(PALETTE_EVENT));
		return;
	}

	if (isEditable(e.target)) return;

	if (e.key.toLowerCase() === 'j') {
		e.preventDefault();
		toggleSidePanel();
		return;
	}

	if (e.key.toLowerCase() === 'b') {
		e.preventDefault();
		toggleRail();
		return;
	}

	if (e.key === ',') {
		e.preventDefault();
		void goto(SETTINGS_VIEW.href);
		return;
	}

	const n = Number(e.key);
	if (Number.isInteger(n) && n >= 1 && n <= MAIN_VIEWS.length) {
		e.preventDefault();
		void goto(MAIN_VIEWS[n - 1].href);
	}
}

/** Installs the listener and returns its teardown, for `onMount`. */
export function installShortcuts(): () => void {
	window.addEventListener('keydown', handleKeydown);
	return () => window.removeEventListener('keydown', handleKeydown);
}
