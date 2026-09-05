// One adapter over the store plugin's persisted UI state (`atlas-ui.json`), which every
// future write from the shell store, the table, the board's lane and detail widths and
// the palette's recent items goes through via `persistSet`. Reads for those stay
// synchronous against `localStorage`, unchanged: they seed `$state` at module or
// component load, before an async store read could land, and the browser build
// (`bun run dev`) has no store to read from anyway. `persistSet` mirrors every write into
// the store when running in Tauri and is a no-op otherwise, once a console warning per
// command name (`desktop`'s job).

import { desktop } from '$lib/shell/platform';

/** Marks the one-time copy of pre-existing `localStorage` values into the store done. */
const MIGRATED_KEY = 'atlas.persist.migrated';

/** Writes one value into the persisted UI state store. No-op in the browser. */
export async function persistSet(key: string, value: unknown): Promise<void> {
	await desktop('ui_state_set', { key, value }, () => undefined);
}

let migration: Promise<void> | null = null;

/**
 * Copies every `atlas.*` key already in `localStorage` into the store, once, so a
 * preference set before the store existed is not lost the first time Tauri persists it
 * instead. Tracked by `MIGRATED_KEY` in the store itself, so a later session's call is a
 * cheap no-op; a no-op in the browser too, since there is nowhere to migrate to.
 */
export function migrateLocalStorage(): Promise<void> {
	if (!migration) migration = runMigration();
	return migration;
}

async function runMigration(): Promise<void> {
	const migrated = await desktop<boolean | null>('ui_state_get', { key: MIGRATED_KEY }, () => true);
	if (migrated) return;
	if (typeof localStorage !== 'undefined') {
		for (let i = 0; i < localStorage.length; i++) {
			const key = localStorage.key(i);
			if (!key || !key.startsWith('atlas.') || key === MIGRATED_KEY) continue;
			const raw = localStorage.getItem(key);
			if (raw === null) continue;
			await persistSet(key, parseOrRaw(raw));
		}
	}
	await persistSet(MIGRATED_KEY, true);
}

/** Most `atlas.*` values are JSON; a few (the theme, the rail state) are plain strings
 * that were never encoded, so a parse failure keeps the raw string rather than dropping
 * the value. */
function parseOrRaw(raw: string): unknown {
	try {
		return JSON.parse(raw);
	} catch {
		return raw;
	}
}
