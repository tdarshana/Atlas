// The global Memories screen's scope select (frame 05). The design offers `All` and then
// one row per connected project, so a single control carries what the store keeps as a
// scope filter plus a project id. These helpers are the whole mapping in both directions.

import type { Project, Uuid } from '$lib/types';
import type { ScopeFilter } from '$lib/stores/memories.svelte';

/** The select's value for "every memory, whatever its scope". */
export const ALL_SCOPE = 'all';

export interface ScopeOption {
	value: string;
	label: string;
}

/** `All` first, then the projects in the order the daemon listed them. */
export function scopeOptions(projects: Project[]): ScopeOption[] {
	return [{ value: ALL_SCOPE, label: 'All' }, ...projects.map((p) => ({ value: p.id, label: p.name }))];
}

/** What the store's filter fields become for a select value. */
export interface ScopeSelection {
	scope: ScopeFilter;
	/** Empty means "no project chosen", which is what the store's `projectId` holds. */
	projectId: Uuid | '';
}

/**
 * A select value read as a filter. `All` clears the project, so leaving a project and
 * coming back does not silently keep filtering by the one chosen before; anything else
 * is a project id and narrows to that project.
 */
export function scopeSelection(value: string): ScopeSelection {
	if (value === ALL_SCOPE) return { scope: 'all', projectId: '' };
	return { scope: 'project', projectId: value };
}

/** The select value that matches the store's current filter. */
export function scopeValue(scope: ScopeFilter, projectId: Uuid | ''): string {
	return scope === 'project' && projectId ? projectId : ALL_SCOPE;
}
