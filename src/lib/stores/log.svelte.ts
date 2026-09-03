// The project Log tab's state: the three filters, the page of rows they produced, and
// whether another page is worth asking for. Paging is by time, not by offset: the daemon
// takes `after` and answers with the entries strictly older than it, so a row written
// while the user reads cannot shift the page under them.

import { api } from '$lib/daemon.svelte';
import { errorLogPath, errorMessage } from '$lib/errors';
import type { LogEntry, LogFilter, Uuid } from '$lib/types';

/** One page. The daemon clamps `limit` to 500; 100 is its own default. */
export const LOG_PAGE = 100;

/** The free-text filter waits this long after the last keystroke. */
export const LOG_DEBOUNCE_MS = 200;

export interface LogFilters {
	/** Empty is "All sources"; the Select binds a string. */
	source: string;
	/** Empty is "All events". */
	kind: string;
	q: string;
}

export const log = $state({
	projectId: '' as Uuid | '',
	source: '',
	kind: '',
	q: '',
	rows: [] as LogEntry[],
	/**
	 * The actors and kinds the two Selects offer. Captured from an unfiltered page, never
	 * from the rows a filter left behind: a vocabulary that narrowed with the rows would
	 * drop the very value the user picked, and a native select with no matching option
	 * falls back to displaying the first one while the store still sends the filter.
	 */
	sources: [] as string[],
	kinds: [] as string[],
	loading: false,
	loadingMore: false,
	/** The last page came back full, so there is at least one more to ask for. */
	more: false,
	error: null as string | null,
	/** Set only for a connection failure, so the error state can point at the log. */
	errorLogPath: null as string | null
});

/**
 * The request a filter state makes. `after` is the last row's time when paging and left
 * off for the first page; an empty filter is a filter not sent, which is what the daemon
 * reads as "no filter" rather than "match the empty string".
 */
export function logParams(filters: LogFilters, after?: string | null): LogFilter {
	return {
		source: filters.source || null,
		kind: filters.kind || null,
		q: filters.q.trim() || null,
		after: after || null,
		limit: LOG_PAGE
	};
}

/** True when at least one filter is set, which is what shows `Clear filters`. */
export function hasFilters(filters: LogFilters): boolean {
	return !!(filters.source || filters.kind || filters.q.trim());
}

/** Distinct sorted values of one field across a page of rows. */
export function distinct(rows: LogEntry[], pick: (row: LogEntry) => string): string[] {
	return [...new Set(rows.map(pick).filter(Boolean))].sort((a, b) => a.localeCompare(b));
}

/**
 * Bumped by every load. Debouncing coalesces keystrokes but says nothing about requests
 * already in flight, so a slow early page must not land on top of a later one.
 */
let generation = 0;

/** Points the store at a project and drops whatever the previous one left behind. */
export function openLog(projectId: Uuid | ''): void {
	if (log.projectId === projectId) return;
	generation++;
	log.projectId = projectId;
	log.source = '';
	log.kind = '';
	log.q = '';
	log.rows = [];
	log.sources = [];
	log.kinds = [];
	log.more = false;
	log.error = null;
	log.errorLogPath = null;
}

/** The first page for the filters as they stand. */
export async function loadLog(): Promise<void> {
	const g = ++generation;
	const id = log.projectId;
	if (!id) return;
	const unfiltered = !hasFilters(log);
	log.loading = true;
	try {
		const rows = await api().projectLog(id, logParams(log));
		if (g !== generation) return;
		log.rows = rows;
		// The Selects learn their vocabulary from the page nothing was filtered out of, so
		// picking a source cannot take the other sources off the list.
		if (unfiltered) {
			log.sources = distinct(rows, (r) => r.source);
			log.kinds = distinct(rows, (r) => r.kind);
		}
		log.more = rows.length === LOG_PAGE;
		log.error = null;
		log.errorLogPath = null;
	} catch (e) {
		if (g !== generation) return;
		log.rows = [];
		log.more = false;
		log.error = errorMessage(e);
		log.errorLogPath = errorLogPath(e);
	} finally {
		if (g === generation) log.loading = false;
	}
}

/** The next page, appended. Silent when there is nothing older to ask for. */
export async function loadMoreLog(): Promise<void> {
	const g = generation;
	const id = log.projectId;
	const last = log.rows.at(-1);
	if (!id || !last || !log.more || log.loadingMore) return;
	log.loadingMore = true;
	try {
		const rows = await api().projectLog(id, logParams(log, last.time));
		if (g !== generation) return;
		log.rows = [...log.rows, ...rows];
		log.more = rows.length === LOG_PAGE;
	} catch (e) {
		if (g !== generation) return;
		log.error = errorMessage(e);
		log.errorLogPath = errorLogPath(e);
	} finally {
		if (g === generation) log.loadingMore = false;
	}
}

let timer: ReturnType<typeof setTimeout> | null = null;

/** Coalesces keystrokes into one request. */
export function scheduleLoadLog(delayMs = LOG_DEBOUNCE_MS): void {
	cancelLoadLog();
	timer = setTimeout(() => {
		timer = null;
		void loadLog();
	}, delayMs);
}

/** Drops a pending debounce so leaving the tab does not fire one more request. */
export function cancelLoadLog(): void {
	if (timer !== null) clearTimeout(timer);
	timer = null;
}

/** Puts every filter back to "all" and reloads. */
export function clearLogFilters(): void {
	log.source = '';
	log.kind = '';
	log.q = '';
	void loadLog();
}
