// What each task kind looks like on the board: a label, a Lucide glyph and the colour of
// the square it sits in, in the style of an issue tracker's type icons. Subtasks are not
// a kind (they are tasks with a parent) but carry a mark of their own beside it.

import type { IconName } from '$lib/ds';
import type { TaskKind, TaskPriority } from '$lib/types';

export interface KindMeta {
	label: string;
	icon: IconName;
	color: string;
}

export const KIND_META: Record<TaskKind, KindMeta> = {
	task: { label: 'Task', icon: 'bookmark', color: '#4BADE8' },
	bug: { label: 'Bug', icon: 'bug', color: '#E5493A' },
	feature: { label: 'Feature', icon: 'square-arrow-up', color: '#65BA43' },
	epic: { label: 'Epic', icon: 'zap', color: '#904EE2' },
	feature_request: { label: 'Feature request', icon: 'lightbulb', color: '#F79232' },
	chore: { label: 'Chore', icon: 'wrench', color: '#8993A4' }
};

export const SUBTASK_META: KindMeta = { label: 'Subtask', icon: 'corner-left-up', color: '#4BADE8' };

/** Every kind, in the order pickers list them. */
export const KINDS = Object.keys(KIND_META) as TaskKind[];

/** Select options with the display label rather than the wire value. */
export const KIND_OPTIONS = KINDS.map((k) => ({ value: k, label: KIND_META[k].label }));

/** The meta for a kind, falling back to `task` for a value this build does not know. */
export function kindMeta(kind: string): KindMeta {
	return KIND_META[kind as TaskKind] ?? KIND_META.task;
}

/** The colour of a stage's lozenge in the task detail, by the default stage names; a
 * renamed or added stage falls back to grey. */
export const STAGE_COLORS: Record<string, string> = {
	Backlog: '#8993A4',
	'In Progress': '#4BADE8',
	Testing: '#904EE2',
	Done: '#65BA43'
};

export function stageColor(stage: string): string {
	return STAGE_COLORS[stage] ?? STAGE_COLORS.Backlog;
}

/** Priority glyphs and colours, in the tracker style: a down arrow for low, a dash for
 * medium, an up arrow for high and a double up arrow for urgent. */
export const PRIORITY_META: Record<TaskPriority, { label: string; icon: IconName; color: string }> = {
	low: { label: 'Low', icon: 'arrow-down', color: '#4BADE8' },
	medium: { label: 'Medium', icon: 'minus', color: '#F79232' },
	high: { label: 'High', icon: 'arrow-up', color: '#E5493A' },
	urgent: { label: 'Urgent', icon: 'chevrons-up', color: '#E5493A' }
};

export const PRIORITIES = Object.keys(PRIORITY_META) as TaskPriority[];

/** Menu options for the Kind picker, each with its mark. */
export const KIND_MENU = KINDS.map((k) => ({ value: k, label: KIND_META[k].label, kind: k }));

/** Menu options for the Priority picker, each with its glyph. */
export const PRIORITY_MENU = PRIORITIES.map((p) => ({ value: p, label: PRIORITY_META[p].label, icon: PRIORITY_META[p].icon, color: PRIORITY_META[p].color }));
