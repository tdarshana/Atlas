// Daemon bootstrap. Inside Tauri the Rust `daemon_ensure` command starts atlasd
// through `daemon_ctl::ensure_daemon`; in a plain browser (`bun run dev`) there is
// no invoke bridge, so we only probe a daemon the developer started themselves.

import { AtlasApi } from './api';

export const daemon = $state({
	port: 7433,
	ready: false,
	error: null as string | null,
	logPath: ''
});

const inTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export function baseUrl(): string {
	return `http://127.0.0.1:${daemon.port}`;
}

export function api(): AtlasApi {
	return new AtlasApi(baseUrl());
}

export async function boot(): Promise<void> {
	daemon.error = null;
	try {
		if (inTauri()) {
			const { invoke } = await import('@tauri-apps/api/core');
			daemon.port = await invoke<number>('daemon_ensure');
		} else {
			// The browser cannot start a daemon, so a failed probe is the whole answer.
			await new AtlasApi(baseUrl()).status();
		}
		daemon.ready = true;
	} catch (e) {
		daemon.ready = false;
		daemon.error = e instanceof Error ? e.message : String(e);
		daemon.logPath = await logPath();
	}
}

async function logPath(): Promise<string> {
	if (!inTauri()) return '~/.atlas/atlasd.log';
	try {
		const { invoke } = await import('@tauri-apps/api/core');
		return await invoke<string>('log_path');
	} catch {
		return '~/.atlas/atlasd.log';
	}
}
