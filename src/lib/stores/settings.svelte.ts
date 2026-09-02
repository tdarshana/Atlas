// Settings state. `GET /api/v1/settings` returns a flat map of the seven keys the
// spec lists, with `extraction.api_key` masked to "***" when it is set. A PUT takes
// a partial map and returns the whole masked map, so the store keeps the server's
// answer as the single source of truth and never guesses at what it stored.

import { api } from '$lib/daemon.svelte';
import { errorLogPath, errorMessage } from '$lib/errors';
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

/**
 * Used when the daemon has never stored a threshold. The spec's default is 1.0,
 * which auto-accepts nothing: a confidence is at most 1.0 and a row only qualifies
 * at or above the threshold, so an unconfigured Atlas asks about every memory.
 */
export const DEFAULT_MIN_CONFIDENCE = 1.0;

export const settings = $state({
	values: {} as Settings,
	loading: false,
	loaded: false,
	error: null as string | null,
	/** Set only for a connection failure, so the error state can point at the log. */
	errorLogPath: null as string | null
});

export async function loadSettings(): Promise<void> {
	settings.loading = true;
	try {
		settings.values = await api().getSettings();
		settings.error = null;
		settings.errorLogPath = null;
		settings.loaded = true;
	} catch (e) {
		settings.error = errorMessage(e);
		settings.errorLogPath = errorLogPath(e);
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
// The map is `Record<string, unknown>` and the daemon answers `null` for a key it
// has never stored, so every read coerces and falls back.

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

// ---- the form's diff ----

/** What the settings form holds. The API key is blank unless the user typed one. */
export interface SettingsDraft {
	enabled: boolean;
	baseUrl: string;
	apiKey: string;
	model: string;
	threshold: number;
}

/** The draft that matches what the server currently reports. */
export function draftFromSettings(): SettingsDraft {
	return {
		enabled: settingBool('extraction.enabled'),
		baseUrl: settingString('extraction.base_url'),
		// A stored key reads back as "***", which is not a key; the box starts blank.
		apiKey: '',
		model: settingString('extraction.model'),
		threshold: minConfidence()
	};
}

/**
 * Only the keys the user actually changed. The API key is sent only when the user
 * typed one: a blank box means "leave the stored key alone", so the "***" the
 * server reports never travels back. Floats compare with a tolerance so a slider
 * round trip at the stored value is not reported as a change.
 */
export function changedSettings(draft: SettingsDraft): Settings {
	const out: Settings = {};
	if (draft.enabled !== settingBool('extraction.enabled')) {
		out['extraction.enabled'] = draft.enabled;
	}
	if (draft.baseUrl !== settingString('extraction.base_url')) {
		out['extraction.base_url'] = draft.baseUrl;
	}
	if (draft.model !== settingString('extraction.model')) {
		out['extraction.model'] = draft.model;
	}
	if (Math.abs(draft.threshold - minConfidence()) > 1e-9) {
		out['extraction.auto_accept_min_confidence'] = draft.threshold;
	}
	if (draft.apiKey !== '') out['extraction.api_key'] = draft.apiKey;
	return out;
}
