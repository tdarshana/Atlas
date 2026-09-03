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
 * What a reading with no value prints. The design's null sentinel is an em dash; this
 * codebase does not write one, so the middle dot stands in for it. Colour it with
 * `--text-null`, which is the token the design system gives an empty reading.
 */
export const NOTHING = '·';

/** Absolute date and time in the user's locale, e.g. `03/09/2026, 3:15:46 PM`. */
export function dateTime(ts: Timestamp | null | undefined): string {
	if (!ts) return NOTHING;
	const d = new Date(ts);
	return Number.isNaN(d.getTime()) ? NOTHING : d.toLocaleString();
}

/** Two digits, so `9:05` prints as `09:05` and the mono column stays aligned. */
const pad = (n: number) => String(n).padStart(2, '0');

/** Midnight local time on the day `ms` falls in. */
function startOfDay(ms: number): number {
	const d = new Date(ms);
	d.setHours(0, 0, 0, 0);
	return d.getTime();
}

/**
 * The log's Time column: `15:15` for today, `Yesterday` for the day before, `03/09` for
 * anything older. A time and a date never share a row's meaning, so the column says which
 * day it is talking about only when that day is not today.
 */
export function logTime(ts: Timestamp, now: number = Date.now()): string {
	const at = new Date(ts).getTime();
	if (!Number.isFinite(at)) return NOTHING;
	const today = startOfDay(now);
	if (at >= today) {
		const d = new Date(at);
		return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
	}
	if (at >= startOfDay(today - 1)) return 'Yesterday';
	const d = new Date(at);
	return `${pad(d.getDate())}/${pad(d.getMonth() + 1)}`;
}

/** True when `ts` falls on the same local day as `now`. */
export function isToday(ts: Timestamp, now: number = Date.now()): boolean {
	const at = new Date(ts).getTime();
	return Number.isFinite(at) && at >= startOfDay(now) && at < startOfDay(now) + 86_400_000;
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
