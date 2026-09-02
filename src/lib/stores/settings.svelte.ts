// Settings state. `GET /api/v1/settings` returns a flat map of the seven keys the
// spec lists, with `extraction.api_key` masked to "***" when it is set. A PUT takes
// a partial map and returns the whole masked map, so the store keeps the server's
// answer as the single source of truth and never guesses at what it stored.

import { ApiError } from '$lib/api';
import { api, daemon } from '$lib/daemon.svelte';
import type { Settings } from '$lib/types';

/** The value `extraction.api_key` reads back as once a key is stored. */
export const MASKED = '***';

export const SETTINGS_KEYS = [
	'extraction.enabled',
	'extraction.base_url',
	'extraction.api_key',
	'extraction.model',
	'extraction.auto_accept_min_confidence',
	'embedding.model',
	'daemon.port'
] as const;

/** Used when the daemon has never stored a threshold. */
export const DEFAULT_MIN_CONFIDENCE = 0.8;

export const settings = $state({
	values: {} as Settings,
	loading: false,
	loaded: false,
	error: null as string | null,
	/** Set only for a connection failure, so the error state can point at the log. */
	errorLogPath: null as string | null
});

function message(e: unknown): string {
	return e instanceof Error ? e.message : String(e);
}

export async function loadSettings(): Promise<void> {
	settings.loading = true;
	try {
		settings.values = await api().getSettings();
		settings.error = null;
		settings.errorLogPath = null;
		settings.loaded = true;
	} catch (e) {
		settings.error = message(e);
		settings.errorLogPath =
			e instanceof ApiError && e.status === 0 ? daemon.logPath || '~/.atlas/atlasd.log' : null;
	} finally {
		settings.loading = false;
	}
}

/**
 * Sends only the keys the caller changed. The reply is the full masked map, so the
 * store replaces its values wholesale. Throws so the caller can show the daemon's
 * `error` string verbatim.
 */
export async function saveSettings(partial: Settings): Promise<void> {
	settings.values = await api().setSettings(partial);
	settings.error = null;
}

// ---- readers ----
// The map is `Record<string, unknown>`, so every read coerces and falls back.

export function settingString(key: string, fallback = ''): string {
	const v = settings.values[key];
	if (v == null) return fallback;
	return typeof v === 'string' ? v : String(v);
}

export function settingBool(key: string, fallback = false): boolean {
	const v = settings.values[key];
	if (typeof v === 'boolean') return v;
	if (typeof v === 'string') return v === 'true' || v === '1';
	if (typeof v === 'number') return v !== 0;
	return fallback;
}

export function settingNumber(key: string, fallback: number): number {
	const v = settings.values[key];
	const n = typeof v === 'number' ? v : typeof v === 'string' ? Number(v) : NaN;
	return Number.isFinite(n) ? n : fallback;
}

/** The auto-accept threshold Review compares confidences against. */
export function minConfidence(): number {
	return settingNumber('extraction.auto_accept_min_confidence', DEFAULT_MIN_CONFIDENCE);
}
