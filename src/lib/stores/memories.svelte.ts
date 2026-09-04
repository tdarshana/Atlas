// Memories screen state. An empty query lists the active memories; a non-empty one
// runs a hybrid search. Both paths end in `hits`, so the table renders one shape;
// `scored` says whether the score column means anything.

import { api } from '$lib/daemon.svelte';
import { errorLogPath, errorMessage } from '$lib/errors';
import type { Memory, MemoryFacets, MemoryKind, MemoryListScope, RecallHit, Uuid } from '$lib/types';

/** `memories.facets` before a load has ever landed, or after one failed. */
const EMPTY_FACETS: MemoryFacets = { kinds: {}, tags: {}, total: 0 };

export type ScopeFilter = 'all' | 'global' | 'project';

export const SEARCH_DEBOUNCE_MS = 300;
export const SEARCH_LIMIT = 50;

/** Every `MemoryKind`, in the order the filter chips show them. */
export const MEMORY_KINDS: MemoryKind[] = ['fact', 'decision', 'preference', 'insight', 'todo'];

export const memories = $state({
	query: '',
	scope: 'all' as ScopeFilter,
	/** Empty means "no project chosen"; `Select` binds a string. */
	projectId: '' as Uuid | '',
	kinds: [] as MemoryKind[],
	/**
	 * Every hit the load returned, narrowed by scope and project but not by kind. The
	 * kind filter is a client-side view over this, so toggling a kind costs no request
	 * and the side panel can count the kinds the user just filtered out.
	 */
	all: [] as RecallHit[],
	hits: [] as RecallHit[],
	scored: false,
	/**
	 * Kind and tag counts over the active set, scoped like `all` but from `GET
	 * /memories/facets` rather than derived from the loaded rows: unlike `all`, this
	 * always covers the whole active set, a search query included.
	 */
	facets: EMPTY_FACETS,
	loading: false,
	error: null as string | null,
	/** Set only for a connection failure, so the error state can point at the log. */
	errorLogPath: null as string | null,
	selected: null as Memory | null
});

/**
 * Bumped by every load. Debouncing coalesces keystrokes but says nothing about
 * requests already in flight, and a slow early search must not land on top of a
 * later one, so a load writes state only while it is still the newest.
 */
let generation = 0;

/** The project filter, or null when it does not apply. */
function activeProjectId(): Uuid | null {
	return memories.scope === 'project' && memories.projectId ? memories.projectId : null;
}

/**
 * The `list_scope` facets are fetched with: `project_only` narrows to that project's
 * own memories, the same narrowing `activeProjectId` already applies for the list
 * itself, so the two never disagree about what "this project" means.
 */
function facetsListScope(): MemoryListScope | null {
	return activeProjectId() ? 'project_only' : null;
}

/** Client-side backstop: the list route takes neither a scope nor kinds. */
function inScope(hit: RecallHit): boolean {
	const m = hit.memory;
	if (memories.scope !== 'all' && m.scope !== memories.scope) return false;
	const projectId = activeProjectId();
	if (projectId && m.project_id !== projectId) return false;
	return true;
}

/** Re-derives `hits` from `all`. The only place the kind filter is applied. */
function applyKinds(): void {
	const kinds = memories.kinds;
	memories.hits =
		kinds.length > 0 ? memories.all.filter((h) => kinds.includes(h.memory.kind)) : [...memories.all];
	reselect(memories.hits);
}

/** Re-points the detail panel at the fresh row, or closes it when it is gone. */
function reselect(hits: RecallHit[]): void {
	const selected = memories.selected;
	if (!selected) return;
	memories.selected = hits.find((h) => h.memory.id === selected.id)?.memory ?? null;
}

export async function loadMemories(): Promise<void> {
	const g = ++generation;
	const query = memories.query.trim();
	const projectId = activeProjectId();
	memories.loading = true;
	try {
		let hits: RecallHit[];
		let scored: boolean;
		// Independent of the query text and the kind filter: this is always the whole
		// active set's facets, not just what the current search matched.
		const facetsPromise = api().memoryFacets(projectId, facetsListScope());
		if (query) {
			// No `kinds` here on purpose: one fetch per load, and the side panel needs the
			// counts for the kinds the filter is currently hiding.
			hits = await api().search({
				query,
				limit: SEARCH_LIMIT,
				// Sending `project` scope without a project would ask the daemon for
				// something we cannot name, so fall back to no scope and filter locally.
				scope:
					memories.scope === 'all' || (memories.scope === 'project' && !projectId)
						? null
						: memories.scope,
				project_id: projectId
			});
			scored = true;
		} else {
			const rows = await api().listMemories('active', projectId);
			hits = rows.map((memory) => ({ memory, score: 0 }));
			scored = false;
		}
		const facets = await facetsPromise;
		if (g !== generation) return;
		memories.all = hits.filter(inScope);
		memories.scored = scored;
		memories.facets = facets;
		applyKinds();
		memories.error = null;
		memories.errorLogPath = null;
	} catch (e) {
		if (g !== generation) return;
		memories.all = [];
		memories.hits = [];
		memories.facets = EMPTY_FACETS;
		memories.selected = null;
		memories.error = errorMessage(e);
		memories.errorLogPath = errorLogPath(e);
	} finally {
		if (g === generation) memories.loading = false;
	}
}

let timer: ReturnType<typeof setTimeout> | null = null;

/** Coalesces keystrokes into one request. */
export function scheduleLoad(delayMs = SEARCH_DEBOUNCE_MS): void {
	cancelLoad();
	timer = setTimeout(() => {
		timer = null;
		void loadMemories();
	}, delayMs);
}

/** Drops a pending debounce so leaving the screen does not fire one more request. */
export function cancelLoad(): void {
	if (timer !== null) clearTimeout(timer);
	timer = null;
}

/** The kind filter is a view over `all`, so a toggle costs no request. */
export function toggleKind(kind: MemoryKind): void {
	const i = memories.kinds.indexOf(kind);
	if (i >= 0) memories.kinds.splice(i, 1);
	else memories.kinds.push(kind);
	applyKinds();
}

export function clearKinds(): void {
	memories.kinds = [];
	applyKinds();
}

/**
 * Forgets a memory and drops its row. Nothing is hard-deleted server side; the
 * row goes because the list only ever shows active memories. Throws on failure so
 * the caller can toast the daemon's message verbatim.
 */
export async function forgetMemory(id: Uuid, reason?: string): Promise<void> {
	await api().forget(id, reason?.trim() || undefined);
	const i = memories.all.findIndex((h) => h.memory.id === id);
	if (i >= 0) memories.all.splice(i, 1);
	const j = memories.hits.findIndex((h) => h.memory.id === id);
	if (j >= 0) memories.hits.splice(j, 1);
	if (memories.selected?.id === id) memories.selected = null;
}
