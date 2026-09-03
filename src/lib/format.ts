// Display formatters shared by the screens.

import { daemon } from './daemon.svelte';
import type { Timestamp } from './types';

/** Compact "how long ago", e.g. `3h`. */
export function relativeAge(ts: Timestamp): string {
	const ms = Date.now() - new Date(ts).getTime();
	if (!Number.isFinite(ms)) return '-';
	const minutes = Math.floor(ms / 60_000);
	if (minutes < 1) return 'just now';
	if (minutes < 60) return `${minutes}m`;
	const hours = Math.floor(minutes / 60);
	if (hours < 24) return `${hours}h`;
	const days = Math.floor(hours / 24);
	if (days < 30) return `${days}d`;
	const months = Math.floor(days / 30);
	if (months < 12) return `${months}mo`;
	return `${Math.floor(days / 365)}y`;
}

/**
 * `1 result`, `2 results`. Counts printed in the palette and the status bar go through
 * here so a single row never reads as "1 results". `plural(1, 'memory', 'memories')`
 * covers the words an `s` does not.
 */
export function plural(n: number, word: string, many = `${word}s`): string {
	return `${n} ${n === 1 ? word : many}`;
}

/** The daemon log, which is what explains a connection failure. */
export function logPath(): string {
	return daemon.logPath || '~/.atlas/atlasd.log';
}
