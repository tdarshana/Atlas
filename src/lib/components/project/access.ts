// The agent access form behind the project Permissions tab, as pure shapes.
//
// Since the daemon grew global defaults, a project's `null` list no longer means "any
// actor": it means "take the global default", which may itself be a list. So each of the
// two list rules is either inherited or an explicit list, and the form says which rather
// than leaving the user to infer it from a row of ticks.

import type { AgentAccess } from '$lib/types';

export interface AccessForm {
	/** The labels offered a tick, already sorted. */
	actors: string[];
	/** True while the project says nothing and takes `access.memory_writers`. */
	inheritWriters: boolean;
	memoryWriters: Record<string, boolean>;
	inheritMovers: boolean;
	taskMovers: Record<string, boolean>;
	requireReview: boolean;
}

/** The two list rules, named the way the daemon's JSON names them. */
export type ListRule = 'memory_writers' | 'task_movers';

function checked(actors: string[], allowed: string[] | null): Record<string, boolean> {
	const out: Record<string, boolean> = {};
	// A null list at this point is the global default saying "any actor", so every box is
	// ticked: that is what turning inheritance off would start from.
	for (const actor of actors) out[actor] = allowed === null || allowed.includes(actor);
	return out;
}

/**
 * The form for one project. A rule the project does not set starts from the effective
 * value, so unticking `Inherit` begins at what is in force rather than at nothing.
 */
export function accessForm(
	access: AgentAccess,
	effective: AgentAccess,
	actors: string[]
): AccessForm {
	return {
		actors,
		inheritWriters: access.memory_writers === null,
		memoryWriters: checked(actors, access.memory_writers ?? effective.memory_writers),
		inheritMovers: access.task_movers === null,
		taskMovers: checked(actors, access.task_movers ?? effective.task_movers),
		requireReview: access.require_review
	};
}

/**
 * The rules the form describes. An inherited rule is `null`, which is what makes the
 * daemon fall back to the global default; a set rule is written out in full, so a label
 * the user added by hand survives the save.
 */
export function toAccess(form: AccessForm): AgentAccess {
	const list = (ticks: Record<string, boolean>) => form.actors.filter((a) => ticks[a]);
	return {
		memory_writers: form.inheritWriters ? null : list(form.memoryWriters),
		task_movers: form.inheritMovers ? null : list(form.taskMovers),
		require_review: form.requireReview
	};
}

/** What one rule's value reads as on screen. A null list is the daemon's "any actor". */
export function accessText(list: string[] | null): string {
	if (list === null) return 'Any actor';
	if (list.length === 0) return 'No actor';
	return list.join(', ');
}

/** The line under a list rule: whether the project sets it, and what is in force. */
export function ruleNote(
	access: AgentAccess,
	effective: AgentAccess,
	rule: ListRule
): { inherited: boolean; text: string } {
	const own = rule === 'memory_writers' ? access.memory_writers : access.task_movers;
	const live = rule === 'memory_writers' ? effective.memory_writers : effective.task_movers;
	return own === null
		? { inherited: true, text: `Inherited from the global default: ${accessText(live)}` }
		: { inherited: false, text: `Set on this project: ${accessText(own)}` };
}

/**
 * The review rule's line. The global flag is a floor a project can only raise, so a
 * project that leaves it off still requires review when the default is on, and the note
 * has to say so rather than let the unticked box imply otherwise.
 */
export function reviewNote(
	access: AgentAccess,
	defaults: AgentAccess
): { inherited: boolean; text: string } {
	if (!access.require_review && defaults.require_review) {
		return {
			inherited: true,
			text: 'Inherited from the global default: review is required.'
		};
	}
	return {
		inherited: false,
		text: access.require_review
			? 'Set on this project: review is required.'
			: 'Set on this project: review is not required.'
	};
}
