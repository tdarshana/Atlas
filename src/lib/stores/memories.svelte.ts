// Memories screen state. An empty query lists the active memories; a non-empty one
// runs a hybrid search. Both paths end in `hits`, so the table renders one shape;
// `scored` says whether the score column means anything.

import { ApiError } from '$lib/api';
import { api, daemon } from '$lib/daemon.svelte';
import type { Memory, MemoryKind, RecallHit, Timestamp, Uuid } from '$lib/types';

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
	hits: [] as RecallHit[],
	scored: false,
	loading: false,
	error: null as string | null,
	/** Set only for a connection failure, so the error state can point at the log. */
	errorLogPath: null as string | null,
	selected: null as Memory | null
});

/** The project filter, or null when it does not apply. */
function activeProjectId(): Uuid | null {
	return memories.scope === 'project' && memories.projectId ? memories.projectId : null;
}

/** Client-side backstop: the list route takes neither a scope nor kinds. */
function visible(hit: RecallHit): boolean {
	const m = hit.memory;
	if (memories.scope !== 'all' && m.scope !== memories.scope) return false;
	const projectId = activeProjectId();
	if (projectId && m.project_id !== projectId) return false;
	if (memories.kinds.length > 0 && !memories.kinds.includes(m.kind)) return false;
	return true;
}

export async function loadMemories(): Promise<void> {
	const query = memories.query.trim();
	const projectId = activeProjectId();
	memories.loading = true;
	try {
		let hits: RecallHit[];
		if (query) {
			hits = await api().search({
				query,
				limit: SEARCH_LIMIT,
				// Sending `project` scope without a project would ask the daemon for
				// something we cannot name, so fall back to no scope and filter locally.
				scope: memories.scope === 'all' || (memories.scope === 'project' && !projectId)
					? null
					: memories.scope,
				project_id: projectId,
				kinds: memories.kinds.length > 0 ? memories.kinds : undefined
			});
			memories.scored = true;
		} else {
			const rows = await api().listMemories('active', projectId);
			hits = rows.map((memory) => ({ memory, score: 0 }));
			memories.scored = false;
		}
		memories.hits = hits.filter(visible);
		memories.error = null;
		memories.errorLogPath = null;
	} catch (e) {
		memories.hits = [];
		memories.error = e instanceof Error ? e.message : String(e);
		memories.errorLogPath = e instanceof ApiError && e.status === 0 ? logPath() : null;
	} finally {
		memories.loading = false;
	}
}

let timer: ReturnType<typeof setTimeout> | null = null;

/** Coalesces keystrokes into one request. */
export function scheduleLoad(delayMs = SEARCH_DEBOUNCE_MS): void {
	if (timer !== null) clearTimeout(timer);
	timer = setTimeout(() => {
		timer = null;
		void loadMemories();
	}, delayMs);
}

export function toggleKind(kind: MemoryKind): void {
	const i = memories.kinds.indexOf(kind);
	if (i >= 0) memories.kinds.splice(i, 1);
	else memories.kinds.push(kind);
	scheduleLoad(0);
}

/**
 * Forgets a memory and drops its row. Nothing is hard-deleted server side; the
 * row goes because the list only ever shows active memories. Throws on failure so
 * the caller can toast the daemon's message verbatim.
 */
export async function forgetMemory(id: Uuid, reason?: string): Promise<void> {
	await api().forget(id, reason?.trim() || undefined);
	const i = memories.hits.findIndex((h) => h.memory.id === id);
	if (i >= 0) memories.hits.splice(i, 1);
	if (memories.selected?.id === id) memories.selected = null;
}

/** The daemon log, which explains a connection failure. */
export function logPath(): string {
	return daemon.logPath || '~/.atlas/atlasd.log';
}

/** Compact "how long ago", e.g. `3h`. Shared by the memories and projects tables. */
export function relativeAge(ts: Timestamp): string {
	const ms = Date.now() - new Date(ts).getTime();
	if (!Number.isFinite(ms)) return '—';
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
