// Pure helper for the project Agents tab's Sync card (frame 02.4): maps the four target
// checkboxes to the `SyncKind`s the daemon's `/api/v1/sync` route takes, and builds the
// request a project sync sends. `SyncPanel.svelte` (the global sync screen) builds the
// same request shape inline; this is kept separate rather than extracted from it because
// that panel also drops the two managed-block targets in global mode, a rule this
// project-scoped card has no use for.

import type { SyncKind, SyncRequest } from '$lib/types';

export interface SyncTargetChoice {
	claude: boolean;
	codex: boolean;
	agentsMd: boolean;
	claudeMd: boolean;
}

export const DEFAULT_SYNC_TARGETS: SyncTargetChoice = {
	claude: true,
	codex: true,
	agentsMd: true,
	claudeMd: true
};

const TARGET_KIND: Record<keyof SyncTargetChoice, SyncKind> = {
	claude: 'claude',
	codex: 'codex',
	agentsMd: 'agents_md',
	claudeMd: 'claude_md'
};

export const SYNC_TARGET_LABELS: { key: keyof SyncTargetChoice; label: string }[] = [
	{ key: 'claude', label: 'Claude agents' },
	{ key: 'codex', label: 'Codex agents' },
	{ key: 'agentsMd', label: 'AGENTS.md block' },
	{ key: 'claudeMd', label: 'CLAUDE.md block' }
];

/** Which `SyncKind`s the checked targets choose. */
export function chosenSyncKinds(choice: SyncTargetChoice): SyncKind[] {
	return SYNC_TARGET_LABELS.filter((t) => choice[t.key]).map((t) => TARGET_KIND[t.key]);
}

/** The request a project's Check or Sync button sends: rooted at the project, never global. */
export function projectSyncRequest(
	root: string,
	choice: SyncTargetChoice,
	checkOnly: boolean
): SyncRequest {
	return { root, global: false, targets: chosenSyncKinds(choice), check_only: checkOnly };
}
