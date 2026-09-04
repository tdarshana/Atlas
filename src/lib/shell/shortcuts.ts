// Window-level shortcuts. Mod+1..9 and Mod+0 walk the rail. Mod is ⌘ on mac and Ctrl
// elsewhere, matching what the rail
// prints. Typing in a field must not navigate, so every combo but Mod+K is ignored while
// the focus is in an editable control; Mod+K reaches the palette from anywhere.

import { goto } from '$app/navigation';
import { inTauri } from './platform';
import { shell, toggleRail, toggleSidePanel } from './shell.svelte';
import { MAIN_VIEWS, SETTINGS_VIEW } from './views';

/** The palette listens for this on `window` and toggles: Mod+K again puts it away. */
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
	// Shift is not part of any combo here, so a shifted press belongs to whatever else
	// claims it. Add the exception alongside the combo when one declares Shift.
	if (!hasMod(e) || e.altKey || e.shiftKey) return;

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

	// `Mod+0` is the tenth rail item: one key cannot spell 10, and 0 is the key next to 9.
	const n = e.key === '0' ? 10 : Number(e.key);
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

/**
 * The Rust global shortcut handler runs outside the webview and cannot dispatch a DOM
 * event directly, so it emits `atlas:palette` as a Tauri event instead; this bridges
 * that onto the same window `CustomEvent` Mod+K raises, so the palette needs only one
 * listener. A no-op outside Tauri. Returns the teardown, for `onMount`.
 */
export async function installGlobalShortcutBridge(): Promise<() => void> {
	if (!inTauri()) return () => {};
	const { listen } = await import('@tauri-apps/api/event');
	const unlisten = await listen(PALETTE_EVENT, () => {
		window.dispatchEvent(new CustomEvent(PALETTE_EVENT));
	});
	return unlisten;
}
