// Pure helpers behind the Log tab: what the two Selects offer, where a row's Ref points,
// what the status bar counts, and what the export file is called. No Svelte here, so
// these are plain unit tests.

import { isToday } from '$lib/format';
import type { LogEntry } from '$lib/types';

/** Distinct sorted values of one field across the rows that are loaded. */
function distinct(rows: LogEntry[], pick: (row: LogEntry) => string): string[] {
	return [...new Set(rows.map(pick).filter(Boolean))].sort((a, b) => a.localeCompare(b));
}

/**
 * The Source select's options. The list comes from the rows on screen rather than from a
 * fixed vocabulary, because the actors that touched a project are the only ones its log
 * can be filtered by.
 */
export function sourceOptions(rows: LogEntry[]): { value: string; label: string }[] {
	return [
		{ value: '', label: 'All sources' },
		...distinct(rows, (r) => r.source).map((s) => ({ value: s, label: s }))
	];
}

/** The Event select's options, over the kinds the loaded rows carry. */
export function kindOptions(rows: LogEntry[]): { value: string; label: string }[] {
	return [
		{ value: '', label: 'All events' },
		...distinct(rows, (r) => r.kind).map((k) => ({ value: k, label: k }))
	];
}

/** The status bar's `<name> · <n> events today`. */
export function eventsToday(rows: LogEntry[], now: number = Date.now()): number {
	return rows.filter((r) => isToday(r.time, now)).length;
}

/** Enough of a uuid to recognise a row by, for a ref that carries no key. */
function shortId(id: string): string {
	return id.length > 8 ? id.slice(0, 8) : id;
}

/** What the Ref column prints: a task key where there is one, else a short id. */
export function refLabel(entry: LogEntry): string {
	const ref = entry.ref;
	if (!ref) return '';
	return ref.key || shortId(ref.id);
}

/**
 * Where the Ref column links. A run has no screen of its own yet, so it lands on the
 * workflows list; a job lands on the settings tab that configures extraction, which is
 * what queued it.
 */
export function refHref(entry: LogEntry, projectId: string): string | null {
	const ref = entry.ref;
	if (!ref || !projectId) return null;
	switch (ref.type) {
		case 'task':
			return `/projects/${projectId}/board?task=${encodeURIComponent(ref.key || ref.id)}`;
		case 'memory':
			return `/memories?id=${encodeURIComponent(ref.id)}`;
		case 'run':
			return '/workflows';
		case 'project':
			return `/projects/${projectId}`;
		case 'sync':
			return `/projects/${projectId}/agents`;
		case 'job':
			return `/projects/${projectId}/settings`;
	}
}

/**
 * The name the save dialog offers. The project name is a folder name and can hold
 * anything, so it is reduced to a slug before it reaches a file path.
 */
export function exportFilename(name: string): string {
	const slug = name
		.trim()
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '');
	return `${slug || 'project'}-log.jsonl`;
}
