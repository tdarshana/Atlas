// The counting and icon rules behind the Board tab's side panel. They are here rather than
// inline in the component so a regression in a count or a glyph is caught by a test.

import type { IconName } from '$lib/ds';
import type { Stage, Task } from '$lib/types';

export interface AssigneeGroups {
	/** Every assignee holding a task, with its count, in name order. */
	named: [string, number][];
	/** How many tasks nobody has claimed. */
	unassigned: number;
}

export function groupAssignees(tasks: Task[]): AssigneeGroups {
	const counts = new Map<string, number>();
	let unassigned = 0;
	for (const task of tasks) {
		const name = task.assignee?.trim();
		if (name) counts.set(name, (counts.get(name) ?? 0) + 1);
		else unassigned++;
	}
	return {
		named: [...counts.entries()].sort((a, b) => a[0].localeCompare(b[0])),
		unassigned
	};
}

export interface PersonaGroup {
	slug: string;
	/** The role when the roster or library knows it, else the persona's name. */
	label: string;
	count: number;
}

/** Every persona holding a task, with its count, in label order; `roleOf` names the chip. */
export function groupPersonas(tasks: Task[], roleOf: (slug: string, name: string) => string): PersonaGroup[] {
	const counts = new Map<string, PersonaGroup>();
	for (const task of tasks) {
		const slug = task.persona_slug;
		if (!slug) continue;
		const hit = counts.get(slug);
		if (hit) hit.count += 1;
		else counts.set(slug, { slug, label: roleOf(slug, task.persona_name ?? slug), count: 1 });
	}
	return [...counts.values()].sort((a, b) => a.label.localeCompare(b.label));
}

/**
 * The frame's glyphs by stage. A board's stages are the user's own, so the one the design
 * names by word is matched by name and the rest fall back on their place in the list.
 */
export function stageIcon(stage: Stage, i: number): IconName {
	if (stage.name.toLowerCase() === 'testing') return 'flask-conical';
	if (stage.done) return 'circle-check';
	return i === 0 ? 'circle' : 'circle-dot';
}

/** How many tasks stand in a stage. */
export function countInStage(tasks: Task[], stage: string): number {
	return tasks.filter((t) => t.stage === stage).length;
}
