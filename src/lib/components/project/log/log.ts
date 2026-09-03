// Pure helpers behind the Log tab: what the two Selects offer, where a row's Ref points,
// what the status bar counts, and what the export file is called. No Svelte here, so
// these are plain unit tests.

import { isToday } from '$lib/format';
import type { LogEntry } from '$lib/types';

export interface Option {
	value: string;
	label: string;
}

/**
 * One filter select's options: the "all" row, the vocabulary the store captured from an
 * unfiltered page, and the value in force if it is somehow not in that vocabulary. A
 * select that does not offer its own value displays the first option instead, which would
 * have the screen claim no filter while the store still sends one.
 */
function options(allLabel: string, vocabulary: string[], selected: string): Option[] {
	const values = vocabulary.includes(selected) || !selected ? vocabulary : [...vocabulary, selected];
	return [
		{ value: '', label: allLabel },
		...[...values].sort((a, b) => a.localeCompare(b)).map((v) => ({ value: v, label: v }))
	];
}

/** The Source select's options, over the actors that have written to this project. */
export function sourceOptions(vocabulary: string[], selected = ''): Option[] {
	return options('All sources', vocabulary, selected);
}

/** The Event select's options, over the kinds this project's log holds. */
export function kindOptions(vocabulary: string[], selected = ''): Option[] {
	return options('All events', vocabulary, selected);
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
