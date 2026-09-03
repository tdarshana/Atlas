// Pure helper for the project Workflows tab (frame 02.5). Phase 9 gives workflows real
// triggers and run history, so a row now comes straight off the daemon's `Workflow`
// rather than being synthesised from a doc.

import { relativeAge } from '$lib/format';
import type { RunStatus, Timestamp, TriggerKind, Workflow } from '$lib/types';

export interface WorkflowRow {
	id: string;
	name: string;
	triggerKind: TriggerKind;
	/** Only set when `triggerKind` is `schedule`. */
	cron: string | null;
	/** Count of `action`-kind nodes in the graph; the trigger and the output are not
	 * actions. */
	actions: number;
	lastRunStatus: RunStatus | null;
	lastRunAt: Timestamp | null;
}

export function workflowRows(workflows: Workflow[]): WorkflowRow[] {
	return workflows.map((w) => ({
		id: w.id,
		name: w.name,
		triggerKind: w.trigger.kind,
		cron: w.trigger.kind === 'schedule' ? w.trigger.cron : null,
		actions: w.graph.nodes.filter((n) => n.kind === 'action').length,
		lastRunStatus: w.last_status,
		lastRunAt: w.last_run_at
	}));
}

/** The Last run column's text: `never` with no run yet, else how long ago it started. */
export function lastRunLabel(row: WorkflowRow, now?: number): string {
	return row.lastRunAt ? relativeAge(row.lastRunAt, now) : 'never';
}
