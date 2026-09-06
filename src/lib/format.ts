// Display formatters shared by the screens.

import { daemon } from './daemon.svelte';
import type { Timestamp } from './types';

/** Compact "how long ago", e.g. `3h`. `now` defaults to the wall clock; a caller
 * passes one in to test a fixed instant, the same way `logTime` does. */
export function relativeAge(ts: Timestamp, now: number = Date.now()): string {
	const ms = now - new Date(ts).getTime();
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
 * `relativeAge` as a phrase: `2d ago`, or `just now` on its own, since "just now ago"
 * is what a caller that appended the word blindly used to print.
 */
export function ageText(ts: Timestamp, now: number = Date.now()): string {
	const age = relativeAge(ts, now);
	return age === 'just now' || age === '-' ? age : `${age} ago`;
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

/** `DD/MM HH:MM` in local time, the compact form the run history's Started column
    uses so the row never wraps. */
export function shortDateTime(ts: Timestamp | null | undefined): string {
	if (!ts) return NOTHING;
	const d = new Date(ts);
	if (Number.isNaN(d.getTime())) return NOTHING;
	return `${pad(d.getDate())}/${pad(d.getMonth() + 1)} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

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

/**
 * True when `ts` falls on the same local day as `now`. Compared by date components
 * rather than by a 24-hour window: a day is 23 or 25 hours long twice a year, and either
 * one would put the window an hour out.
 */
export function isToday(ts: Timestamp, now: number = Date.now()): boolean {
	const at = new Date(ts);
	if (!Number.isFinite(at.getTime())) return false;
	const today = new Date(now);
	return (
		at.getFullYear() === today.getFullYear() &&
		at.getMonth() === today.getMonth() &&
		at.getDate() === today.getDate()
	);
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

/**
 * A run or step's wall time as `1m 42s`, whole seconds only: `finishedAt` to
 * `startedAt` once it is set, else `now` to `startedAt`, so a still-running run or step
 * reads as elapsed time instead of `n/a` (and keeps climbing as `RunDetail`'s 2s poll
 * feeds a fresh `now` in). `n/a` only when `startedAt` itself does not parse, or the
 * span it would produce runs backwards. `now` defaults to the wall clock; a caller
 * passes one in to test a fixed instant, the same way `relativeAge`/`logTime` do.
 */
export function duration(
	startedAt: Timestamp,
	finishedAt: Timestamp | null,
	now: number = Date.now()
): string {
	const startMs = new Date(startedAt).getTime();
	if (!Number.isFinite(startMs)) return 'n/a';
	const endMs = finishedAt ? new Date(finishedAt).getTime() : now;
	if (!Number.isFinite(endMs) || endMs < startMs) return 'n/a';
	const totalSeconds = Math.floor((endMs - startMs) / 1000);
	const minutes = Math.floor(totalSeconds / 60);
	const seconds = totalSeconds % 60;
	return `${minutes}m ${seconds}s`;
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/**
 * The fixed time a history row shows on hover and the Created field shows outright:
 * `2026 Sep 06, 08:39PM`. `withYear` false drops the year for a date in the current year.
 */
export function fixedTime(ts: Timestamp | null | undefined, withYear = true): string {
	if (!ts) return NOTHING;
	const d = new Date(ts);
	if (Number.isNaN(d.getTime())) return NOTHING;
	const h = d.getHours();
	const hour12 = h % 12 === 0 ? 12 : h % 12;
	const clock = `${pad(hour12)}:${pad(d.getMinutes())}${h < 12 ? 'AM' : 'PM'}`;
	const day = `${MONTHS[d.getMonth()]} ${pad(d.getDate())}`;
	return withYear ? `${d.getFullYear()} ${day}, ${clock}` : `${day}, ${clock}`;
}

/**
 * A history row's time: `4h ago` for today, the date and time for any other day (with
 * the year only when it is not this year). The fixed form sits in the row's tooltip.
 */
export function eventTime(ts: Timestamp, now: number = Date.now()): string {
	if (isToday(ts, now)) {
		const age = relativeAge(ts, now);
		return age === 'just now' || age === '-' ? age : `${age} ago`;
	}
	const sameYear = new Date(ts).getFullYear() === new Date(now).getFullYear();
	return fixedTime(ts, !sameYear);
}
