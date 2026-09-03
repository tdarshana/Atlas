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

const warnedCommands = new Set<string>();

/**
 * Every desktop-only capability in the app goes through this: it calls a Rust command
 * when running inside Tauri, and otherwise runs `fallback` (`localStorage`, a no-op,
 * whatever the caller has), printing one console warning per command name so a
 * repeatedly-called command does not spam the console. This keeps the browser build
 * (`bun run dev`) working without a Tauri bridge.
 */
export async function desktop<T>(
	command: string,
	args: Record<string, unknown> | undefined,
	fallback: () => T | Promise<T>
): Promise<T> {
	if (inTauri()) {
		const { invoke } = await import('@tauri-apps/api/core');
		return invoke<T>(command, args);
	}
	if (!warnedCommands.has(command)) {
		warnedCommands.add(command);
		console.warn(`Not running in Tauri; "${command}" falls back.`);
	}
	return fallback();
}

/**
 * Every copy action in the app goes through this: `clipboard_write` inside Tauri,
 * `navigator.clipboard.writeText` in the browser build.
 */
export async function copyText(text: string): Promise<void> {
	await desktop('clipboard_write', { text }, () => navigator.clipboard.writeText(text));
}
