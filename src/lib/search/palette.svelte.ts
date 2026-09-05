// The command palette's state. The input holds only the text: a finished `@name `
// token is lifted out into `scopes` and drawn as a chip, which is what the design
// shows. Every keystroke schedules one debounced request; a generation counter drops
// a response that lands after a newer one went out.

import { goto } from '$app/navigation';
import { api } from '$lib/daemon.svelte';
import { errorMessage } from '$lib/errors';
import { persistSet } from '$lib/platform/persist';
import { projects } from '$lib/stores/projects.svelte';
import type { SearchGroup, SearchHit, SearchKind, SearchResult, Uuid } from '$lib/types';
import { parseQuery, resolveScopes, stripScopes, type ParsedQuery, type ScopeChip } from './parse';

/** Where the recent list survives a restart. */
export const RECENT_KEY = 'atlas.recent';
export const RECENT_MAX = 8;
export const DEBOUNCE_MS = 120;

/** The order the daemon returns groups in, and the order the palette draws them. */
export const KIND_ORDER: SearchKind[] = [
	'task',
	'memory',
	'project',
	'file',
	'commit',
	'event',
	'workflow'
];

/** How Enter opens the selected row. */
export type OpenMode = 'open' | 'inspector' | 'board';

export const palette = $state({
	open: false,
	/** The text part of the query; scopes are held separately. */
	input: '',
	/** Project names as typed. Unknown ones stay here and are ignored in the request. */
	scopes: [] as string[],
	results: null as SearchResult | null,
	/** Index into the flattened row list the component builds. */
	selected: 0,
	recent: [] as SearchHit[],
	loading: false,
	error: null as string | null,
	total: 0,
	took_ms: 0
});

/** The query as the parser sees it, scopes included. */
export function parsed(): ParsedQuery {
	const p = parseQuery(palette.input);
	return { ...p, scopes: [...palette.scopes, ...p.scopes] };
}

/** The scope chips to draw, each resolved against the connected projects or not. */
export function scopeChips(): ScopeChip[] {
	return resolveScopes(palette.scopes, projects.items);
}

// ---- opening and closing ----

export function openPalette(): void {
	palette.open = true;
	palette.selected = 0;
	loadRecent();
}

export function closePalette(): void {
	cancelSearch();
	palette.open = false;
	palette.input = '';
	palette.scopes = [];
	palette.results = null;
	palette.selected = 0;
	palette.loading = false;
	palette.error = null;
	palette.total = 0;
	palette.took_ms = 0;
}

// ---- input and scopes ----

/**
 * Takes the raw box contents. A `@name ` token that has been finished with a space
 * becomes a chip and leaves the text; a `@partial` still being typed stays put so the
 * project picker can filter on it.
 */
export function setInput(raw: string): void {
	const p = parseQuery(raw);
	for (const name of p.scopes) addScope(name);
	palette.input = p.scopes.length ? stripScopes(raw) : raw;
	palette.selected = 0;
	schedule();
}

export function addScope(name: string): void {
	if (!name) return;
	if (palette.scopes.some((s) => s.toLowerCase() === name.toLowerCase())) return;
	palette.scopes.push(name);
}

export function removeScope(name: string): void {
	palette.scopes = palette.scopes.filter((s) => s.toLowerCase() !== name.toLowerCase());
	schedule();
}

/** Backspace on an empty box drops the last chip, as the footer hint promises. */
export function popScope(): void {
	if (!palette.scopes.length) return;
	palette.scopes.pop();
	schedule();
}

export function clearScopes(): void {
	palette.scopes = [];
	schedule();
}

/** Picking a project from the picker: the partial `@…` leaves the box as a chip. */
export function chooseScope(name: string): void {
	addScope(name);
	palette.input = '';
	palette.selected = 0;
	schedule();
}

/** Picking `#`, `~` or `>` from the picker replaces the partial with that prefix. */
export function choosePrefix(prefix: string): void {
	palette.input = prefix;
	palette.selected = 0;
	schedule();
}

// ---- searching ----

let timer: ReturnType<typeof setTimeout> | null = null;
let generation = 0;

export function cancelSearch(): void {
	if (timer) clearTimeout(timer);
	timer = null;
	generation++;
}

/** Coalesces a burst of keystrokes into one request. */
export function schedule(): void {
	if (timer) clearTimeout(timer);
	timer = setTimeout(() => {
		timer = null;
		void runSearch();
	}, DEBOUNCE_MS);
}

function clearResults(): void {
	palette.results = null;
	palette.total = 0;
	palette.took_ms = 0;
	palette.loading = false;
	palette.error = null;
}

/**
 * One round trip per resolved scope, merged. The daemon's `project_id` takes a single
 * project, so stacked scopes are the union of one search each; an unresolved scope
 * name is ignored rather than narrowing the search to nothing.
 */
export async function runSearch(): Promise<void> {
	const g = ++generation;
	const p = parsed();
	const text = p.text.trim();

	if (!text || p.type === 'command' || p.type === 'project-picker') {
		clearResults();
		return;
	}

	const kinds: SearchKind[] | undefined =
		p.type === 'task' ? ['task'] : p.type === 'memory' ? ['memory'] : undefined;
	const ids = resolveScopes(p.scopes, projects.items)
		.map((c) => c.id)
		.filter((id): id is Uuid => id !== null);

	palette.loading = true;
	try {
		const parts = ids.length
			? await Promise.all(ids.map((id) => api().globalSearch({ q: text, project_id: id, kinds })))
			: [await api().globalSearch({ q: text, kinds })];
		if (g !== generation) return;
		const merged = mergeResults(parts);
		palette.results = merged;
		palette.total = merged.total;
		palette.took_ms = merged.took_ms;
		palette.error = null;
	} catch (e) {
		if (g !== generation) return;
		palette.results = null;
		palette.total = 0;
		palette.took_ms = 0;
		palette.error = errorMessage(e);
	} finally {
		if (g === generation) palette.loading = false;
	}
}

/**
 * Union by `kind:id`, kinds in the fixed order, each group by score then title.
 *
 * `total` is always the number of rows this returns, not the daemon's own `total`.
 * The daemon counts every candidate that scored, capped per source, so passing it
 * through for one scope and recounting for two would make stacking a scope look like
 * it narrowed the search. One meaning, in every case: how many rows are on screen.
 */
export function mergeResults(parts: SearchResult[]): SearchResult {
	const byKind = new Map<SearchKind, Map<string, SearchHit>>();
	for (const part of parts) {
		for (const group of part.groups) {
			const bucket = byKind.get(group.kind) ?? new Map<string, SearchHit>();
			for (const hit of group.items) if (!bucket.has(hit.id)) bucket.set(hit.id, hit);
			byKind.set(group.kind, bucket);
		}
	}
	const groups: SearchGroup[] = [];
	let total = 0;
	for (const kind of KIND_ORDER) {
		const bucket = byKind.get(kind);
		if (!bucket || bucket.size === 0) continue;
		const items = [...bucket.values()].sort(
			(a, b) => b.score - a.score || a.title.localeCompare(b.title)
		);
		total += items.length;
		groups.push({ kind, items });
	}
	const took = parts.length ? Math.max(...parts.map((p) => p.took_ms)) : 0;
	return { groups, total, took_ms: took };
}

// ---- recent ----

/**
 * A stored row only counts if it still has the three fields the list draws with. An
 * older or hand-edited `atlas.recent` must cost the user their history, never the
 * palette: a row without a title would throw inside the render.
 */
function isStoredHit(value: unknown): value is SearchHit {
	if (!value || typeof value !== 'object') return false;
	const row = value as Record<string, unknown>;
	return (
		typeof row.id === 'string' &&
		typeof row.title === 'string' &&
		typeof row.kind === 'string' &&
		KIND_ORDER.includes(row.kind as SearchKind)
	);
}

/** Reads `atlas.recent`. A webview with site data blocked simply has no history. */
export function loadRecent(): void {
	try {
		if (typeof localStorage === 'undefined') return;
		const raw = localStorage.getItem(RECENT_KEY);
		if (!raw) return;
		const stored: unknown = JSON.parse(raw);
		if (!Array.isArray(stored)) return;
		palette.recent = stored
			.filter(isStoredHit)
			.slice(0, RECENT_MAX)
			.map((hit) => ({ ...hit, highlights: [] }));
	} catch {
		/* unreadable or unparseable history is no history */
	}
}

function saveRecent(): void {
	try {
		if (typeof localStorage === 'undefined') return;
		localStorage.setItem(RECENT_KEY, JSON.stringify(palette.recent));
	} catch {
		/* the list is a convenience; losing it must not break opening the item */
	}
	void persistSet(RECENT_KEY, palette.recent);
}

/**
 * Newest first, deduped by `kind:id`, capped at eight. The highlights are dropped on
 * the way in: they belong to the query that found the row, and the empty state that
 * draws the recent list has no query to highlight against.
 */
export function pushRecent(hit: SearchHit): void {
	const key = `${hit.kind}:${hit.id}`;
	const row: SearchHit = { ...hit, highlights: [] };
	palette.recent = [row, ...palette.recent.filter((h) => `${h.kind}:${h.id}` !== key)].slice(
		0,
		RECENT_MAX
	);
	saveRecent();
}

// ---- opening an item ----

function withParam(href: string, param: string): string {
	return `${href}${href.includes('?') ? '&' : '?'}${param}`;
}

/** The route segment for the every-project board, where a hit with no project belongs. */
const GLOBAL_BOARD = 'global';

function boardHref(projectId: Uuid | null, key: string): string {
	return `/projects/${projectId ?? GLOBAL_BOARD}/board?task=${encodeURIComponent(key)}`;
}

/**
 * Where a hit opens. `inspector` is the same destination with `inspector=1`; `board`
 * goes to the owning project's board pre-filtered by the query text, or to the global
 * board when the hit belongs to no project.
 */
export function hrefFor(hit: SearchHit, mode: OpenMode = 'open'): string {
	const project = hit.project_id;

	if (mode === 'board') {
		const text = parsed().text.trim();
		const id = project ?? GLOBAL_BOARD;
		return `/projects/${id}/board${text ? `?q=${encodeURIComponent(text)}` : ''}`;
	}

	let href: string;
	switch (hit.kind) {
		case 'task':
			href = boardHref(project, hit.reference ?? hit.id);
			break;
		case 'memory':
			href = `/memories?id=${encodeURIComponent(hit.id)}`;
			break;
		case 'project':
			href = `/projects/${hit.id}`;
			break;
		case 'file':
		case 'commit':
			href = project ? `/projects/${project}` : '/projects';
			break;
		case 'workflow':
			href = '/workflows';
			break;
		case 'event':
			// An event points at the thing it happened to; without a ref, at its project.
			if (hit.reference) href = boardHref(project, hit.reference);
			else href = project ? `/projects/${project}` : '/projects';
			break;
	}

	return mode === 'inspector' ? withParam(href, 'inspector=1') : href;
}

/** Records the hit in `recent`, closes the palette and navigates. */
export async function openItem(hit: SearchHit, mode: OpenMode = 'open'): Promise<void> {
	const href = hrefFor(hit, mode);
	pushRecent(hit);
	closePalette();
	await goto(href);
}
