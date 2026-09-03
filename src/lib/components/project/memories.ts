// Pure helpers for the project Memories tab's filter row (frame 02.1). Kept apart from
// the global memories store: that store's `list` route always fetches every project when
// called with no project id, and this tab is scoped to one, so the tab fetches directly
// through `api()` rather than reusing the shared singleton's state.

import type { MemoryKind, MemoryStatus } from '$lib/types';

export type MemoryStateFilter = 'active' | 'pending' | 'all';

/** The state select's options, frame order: Accepted, Pending, All. */
export const MEMORY_STATE_OPTIONS: { value: MemoryStateFilter; label: string }[] = [
	{ value: 'active', label: 'Accepted' },
	{ value: 'pending', label: 'Pending' },
	{ value: 'all', label: 'All' }
];

/** Toggles a kind in the chip selection; an empty selection means "every kind". */
export function toggleMemoryKind(kinds: MemoryKind[], kind: MemoryKind): MemoryKind[] {
	return kinds.includes(kind) ? kinds.filter((k) => k !== kind) : [...kinds, kind];
}

/**
 * The statuses the state select fetches. The daemon's list route takes exactly one
 * status and has no combined filter, so "All" merges two calls (active and pending);
 * rejected and superseded are forgotten memories and stay out of this tab.
 */
export function memoryStatusesFor(state: MemoryStateFilter): MemoryStatus[] {
	return state === 'all' ? ['active', 'pending'] : [state];
}

/** Whether a row's kind passes the chip filter; no chips on means everything passes. */
export function passesKindFilter(kind: MemoryKind, kinds: MemoryKind[]): boolean {
	return kinds.length === 0 || kinds.includes(kind);
}
