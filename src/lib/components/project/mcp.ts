// Pure helpers for the project MCP tab: the read-only agent access summary line, kept
// separate from the page so it is unit-testable without mounting Svelte. The tools
// table's three-way state and the toggle payload live in `$lib/mcp`, shared with the
// global MCP view.

import type { AgentAccess } from '$lib/types';

/** `null` means every actor; an empty list would too, but the API never sends one. */
function actorList(names: string[] | null): string {
	return names && names.length > 0 ? names.join(', ') : 'Any actor';
}

export interface AgentAccessSummary {
	memoryWriters: string;
	taskMovers: string;
	requireReview: boolean;
}

/** The AGENT ACCESS card's three readings, straight off the project's own settings. */
export function agentAccessSummary(access: AgentAccess): AgentAccessSummary {
	return {
		memoryWriters: actorList(access.memory_writers),
		taskMovers: actorList(access.task_movers),
		requireReview: access.require_review
	};
}
