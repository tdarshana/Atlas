// Pure helpers behind the Project settings tab: the shapes the three editable cards read
// from a `Project` and write back to the daemon. No Svelte here, so these are plain unit
// tests.

import { plural } from '$lib/format';
import type { AgentAccess, ProjectExtraction } from '$lib/types';

/** `^[A-Z][A-Z0-9]{1,5}$`, the same rule the daemon applies to a board key. */
export const KEY_PREFIX_RE = /^[A-Z][A-Z0-9]{1,5}$/;

/** What the daemon sends in place of a stored key, and what means "leave it alone". */
export const MASKED_KEY = '***';

/** The three actors that exist before a project has a log to learn from. */
export const DEFAULT_ACTORS = ['cli/codex', 'cli/claude-code', 'desktop'];

/**
 * The labels the Agent access card offers a checkbox for: the defaults, whatever has
 * written to this project's log, and anything already on the allowlist, so a rule the
 * user set from the CLI is still visible here.
 */
export function knownActors(sources: string[], access: AgentAccess | null): string[] {
	const all = [
		...DEFAULT_ACTORS,
		...sources,
		...(access?.memory_writers ?? []),
		...(access?.task_movers ?? [])
	];
	return [...new Set(all.map((a) => a.trim()).filter(Boolean))].sort((a, b) => a.localeCompare(b));
}

/**
 * The ticked state per actor. A null list means any actor may write, so every box is
 * ticked; a list ticks the labels it holds.
 */
export function accessChecked(
	actors: string[],
	access: AgentAccess | null
): Record<string, boolean> {
	const allowed = access?.memory_writers ?? null;
	const out: Record<string, boolean> = {};
	for (const actor of actors) out[actor] = allowed === null || allowed.includes(actor);
	return out;
}

/**
 * The rules the ticks describe. Everything ticked is "no restriction", which is a pair of
 * nulls rather than a list of every actor, so an agent the user has not met yet is still
 * allowed. The frame draws one list, so both lists carry the same set.
 */
export function toAgentAccess(
	actors: string[],
	checked: Record<string, boolean>,
	requireReview: boolean
): AgentAccess {
	const on = actors.filter((a) => checked[a]);
	if (on.length === actors.length) {
		return { memory_writers: null, task_movers: null, require_review: requireReview };
	}
	return { memory_writers: on, task_movers: on, require_review: requireReview };
}

/** The Extraction card's fields, all strings because they come from inputs. */
export interface ExtractionForm {
	/** Ticked means the project has no override at all. */
	useGlobal: boolean;
	baseUrl: string;
	model: string;
	/** Only ever holds a key the user just typed; a stored one never comes back. */
	apiKey: string;
	threshold: string;
	/** Whether the daemon is holding a key for this project. */
	storedKey: boolean;
}

/** The form a stored override reads as. A null override is "use global". */
export function extractionForm(extraction: ProjectExtraction | null): ExtractionForm {
	return {
		useGlobal: extraction === null,
		baseUrl: extraction?.base_url ?? '',
		model: extraction?.model ?? '',
		apiKey: '',
		threshold:
			extraction?.auto_accept_min_confidence == null
				? ''
				: String(extraction.auto_accept_min_confidence),
		storedKey: extraction?.api_key === MASKED_KEY
	};
}

/**
 * The override the form describes, or null to drop it. A field left blank is a field the
 * project does not override, which the daemon fills in from the global settings. An
 * untouched key is sent back masked, which means "leave the stored one alone".
 */
export function toProjectExtraction(form: ExtractionForm): ProjectExtraction | null {
	if (form.useGlobal) return null;
	const threshold = Number(form.threshold);
	return {
		enabled: true,
		base_url: form.baseUrl.trim() || null,
		model: form.model.trim() || null,
		api_key: form.apiKey ? form.apiKey : form.storedKey ? MASKED_KEY : null,
		auto_accept_min_confidence:
			form.threshold.trim() === '' || !Number.isFinite(threshold) ? null : threshold
	};
}

/**
 * Pointing the override at a different endpoint drops the key that was entered against
 * the old one, so a key is never sent to a host it was not meant for. The daemon applies
 * the same rule; doing it here as well is what lets the card say so.
 */
export function changeBaseUrl(form: ExtractionForm, next: string): ExtractionForm {
	const moved = next.trim() !== form.baseUrl.trim();
	return moved ? { ...form, baseUrl: next, apiKey: '', storedKey: false } : { ...form, baseUrl: next };
}

/** What the key prefix confirm asks before a rename touches every task in the project. */
export function keyPrefixConfirm(tasks: number, from: string, to: string): string {
	return `Rename ${plural(tasks, 'task')} from ${from}- to ${to}-?`;
}
