// Pure helper for the project Workflows tab (frame 02.5). Phase 9 adds real triggers and
// run history; until then every workflow doc reads as one manual action that has never
// run, which is the only state the frame itself draws a value for.

import type { Doc } from '$lib/types';

export interface WorkflowRow {
	name: string;
	trigger: 'manual';
	actions: number;
	lastRun: 'never';
}

export function workflowRows(docs: Doc[]): WorkflowRow[] {
	return docs.map((d) => ({ name: d.name, trigger: 'manual', actions: 1, lastRun: 'never' }));
}
