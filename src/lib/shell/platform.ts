// Which window chrome to draw. The DS `resolvePlatform` sniffs the user agent, which is
// wrong inside a webview: Tauri reports the host OS through its own plugin, and outside
// Tauri (`bun run dev`, tests) the design is authored for the mac chrome, so that is the
// fallback.

import type { Platform } from '$lib/ds';

export function inTauri(): boolean {
	return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/** `mac` unless Tauri answers otherwise. Never throws: chrome is not worth a failure. */
export async function resolvePlatform(): Promise<Platform> {
	if (!inTauri()) return 'mac';
	try {
		const { type } = await import('@tauri-apps/plugin-os');
		const os = type();
		if (os === 'windows') return 'windows';
		if (os === 'linux') return 'linux';
		return 'mac';
	} catch {
		return 'mac';
	}
}
