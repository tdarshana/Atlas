// Daemon bootstrap. Inside Tauri the Rust `daemon_ensure` command starts atlasd
// through `daemon_ctl::ensure_daemon`; in a plain browser (`bun run dev`) there is
// no invoke bridge, so we only probe a daemon the developer started themselves.

import { AtlasApi } from './api';

export const daemon = $state({
	port: 7433,
	/** The secret every `/api/v1` request carries as `X-Atlas-Token` (SEC-5). The Rust
	 * `daemon_ensure` command reads it from `daemon.json`, which the webview cannot; in a
	 * plain browser there is no way to learn it and the daemon answers 401. */
	token: '',
	ready: false,
	error: null as string | null,
	logPath: ''
});

/** What `daemon_ensure` and `daemon_restart` answer with. */
export type DaemonHandle = { port: number; token: string };

const inTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export function baseUrl(): string {
	return `http://127.0.0.1:${daemon.port}`;
}

/** `?token=<token>` for the two URLs a browser cannot put a header on (`EventSource`
 * and `WebSocket`), or nothing when no token is known. */
export function tokenQuery(token: string = daemon.token): string {
	return token ? `?token=${encodeURIComponent(token)}` : '';
}

/** Takes the port and token of a daemon the host just started or restarted. */
export function adopt(handle: DaemonHandle): void {
	daemon.port = handle.port;
	daemon.token = handle.token;
}

/**
 * The daemon restarted (from the menu bar, `atlas daemon stop`, or a crash) and minted a
 * new token, so the one in hand answers 401. Ask the host for the current handle once
 * for however many requests failed at the same moment, adopt it, and hand the token
 * back so the caller retries. Outside Tauri there is no host to ask.
 */
let reauthInFlight: Promise<string | null> | null = null;
export function reauth(): Promise<string | null> {
	if (!inTauri()) return Promise.resolve(null);
	if (!reauthInFlight) {
		reauthInFlight = (async () => {
			try {
				const { invoke } = await import('@tauri-apps/api/core');
				const before = daemon.token;
				const handle = await invoke<DaemonHandle>('daemon_ensure');
				adopt(handle);
				return handle.token === before ? null : handle.token;
			} catch {
				return null;
			} finally {
				reauthInFlight = null;
			}
		})();
	}
	return reauthInFlight;
}

export function api(): AtlasApi {
	return new AtlasApi(baseUrl(), daemon.token, reauth);
}

export async function boot(): Promise<void> {
	daemon.error = null;
	try {
		if (inTauri()) {
			const { invoke } = await import('@tauri-apps/api/core');
			adopt(await invoke<DaemonHandle>('daemon_ensure'));
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
