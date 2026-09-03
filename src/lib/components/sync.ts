// Pure helpers for the global Agents screen's Sync card (frame 06). The scope select
// offers the home directory or a connected project, and the two managed-block targets
// only mean something inside a repository, so a global scope disables them. The
// project-scoped card has its own helpers in `project/sync.ts`, which never has to make
// that choice.

import type { SyncKind, SyncRequest } from '$lib/types';

/** The scope select's value for the home directory. Anything else is a project id. */
export const GLOBAL_SCOPE = 'global';

export interface SyncTarget {
	kind: SyncKind;
	label: string;
	/** A managed block is spliced into a file in a repository, so global has nowhere to put it. */
	projectOnly: boolean;
}

export const SYNC_TARGETS: SyncTarget[] = [
	{ kind: 'claude', label: 'Claude agents', projectOnly: false },
	{ kind: 'codex', label: 'Codex agents', projectOnly: false },
	{ kind: 'agents_md', label: 'AGENTS.md block', projectOnly: true },
	{ kind: 'claude_md', label: 'CLAUDE.md block', projectOnly: true }
];

/** The hint a disabled target carries, so the reason is on the control itself. */
export const PROJECT_ONLY_HINT = 'project scope only';

/** Whether `target` can be written at this scope. */
export function targetEnabled(target: SyncTarget, scope: string): boolean {
	return !(scope === GLOBAL_SCOPE && target.projectOnly);
}

/**
 * The kinds a Check or Sync at this scope sends. A target the user ticked but the scope
 * disables is dropped, so returning to a project scope finds it ticked again.
 */
export function chosenKinds(chosen: SyncKind[], scope: string): SyncKind[] {
	return SYNC_TARGETS.filter((t) => chosen.includes(t.kind) && targetEnabled(t, scope)).map(
		(t) => t.kind
	);
}

/**
 * The request the buttons send. A global sync has no root; a project sync is rooted at
 * the path the caller resolved from the scope.
 */
export function syncRequest(
	scope: string,
	root: string | null,
	chosen: SyncKind[],
	checkOnly: boolean
): SyncRequest {
	const global = scope === GLOBAL_SCOPE;
	return {
		root: global ? null : root,
		global,
		targets: chosenKinds(chosen, scope),
		check_only: checkOnly
	};
}
