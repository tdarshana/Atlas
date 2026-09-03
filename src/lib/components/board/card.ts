// The card's one piece of pure display logic, kept out of the component so it can be
// read on its own. Colour is never the only carrier: the dot also carries a title.

import type { TaskPriority } from '$lib/types';

const TONES: Record<TaskPriority, string> = {
	urgent: 'var(--danger)',
	high: 'var(--warning)',
	medium: 'var(--text-tertiary)',
	low: 'var(--border-default)'
};

/** The priority dot's colour. An unknown priority reads as the quietest one. */
export function priorityTone(priority: TaskPriority | string): string {
	return TONES[priority as TaskPriority] ?? TONES.low;
}
