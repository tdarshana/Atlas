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
 * written to this project's log, and anything already on either allowlist, so a rule the
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
 * True when the project's two rules are not the same list. The frame draws one column of
 * ticks, which can only speak for one rule; when the daemon holds two different rules the
 * card has to draw both rather than quietly flatten the one it is not showing.
 */
export function accessIsSplit(access: AgentAccess | null): boolean {
	const writers = access?.memory_writers ?? null;
	const movers = access?.task_movers ?? null;
	if (writers === null || movers === null) return writers !== movers;
	return writers.length !== movers.length || writers.some((a) => !movers.includes(a));
}

/**
 * The ticked state per actor for one rule. A null list means any actor may act, so every
 * box is ticked; a list ticks the labels it holds.
 */
export function accessChecked(
	actors: string[],
	allowed: string[] | null
): Record<string, boolean> {
	const out: Record<string, boolean> = {};
	for (const actor of actors) out[actor] = allowed === null || allowed.includes(actor);
	return out;
}

/**
 * The rules the ticks describe. Everything ticked is "no restriction", which is a pair of
 * nulls rather than a list of every actor, so an agent the user has not met yet is still
 * allowed. `explicit` overrides that and writes the lists out in full, which is how a
 * label the user added by hand survives a save.
 */
export function toAgentAccess(
	actors: string[],
	memoryWriters: Record<string, boolean>,
	taskMovers: Record<string, boolean>,
	requireReview: boolean,
	explicit = false
): AgentAccess {
	const list = (checked: Record<string, boolean>) => {
		const on = actors.filter((a) => checked[a]);
		return !explicit && on.length === actors.length ? null : on;
	};
	return {
		memory_writers: list(memoryWriters),
		task_movers: list(taskMovers),
		require_review: requireReview
	};
}

/** The Extraction card's fields, all strings because they come from inputs. */
export interface ExtractionForm {
	/** Ticked means the project has no override at all. */
	useGlobal: boolean;
	/**
	 * Whether extraction runs for this project. Null is the loaded state of an override
	 * that says nothing about it, and inherits the global switch.
	 */
	enabled: boolean | null;
	baseUrl: string;
	/** The base URL the project was loaded with, which is the one the stored key belongs to. */
	loadedBaseUrl: string;
	model: string;
	/** Only ever holds a key the user just typed; a stored one never comes back. */
	apiKey: string;
	threshold: string;
	/** Whether the daemon is holding a key for this project. */
	storedKey: boolean;
}

/** The form a stored override reads as. A null override is "use global". */
export function extractionForm(extraction: ProjectExtraction | null): ExtractionForm {
	const baseUrl = extraction?.base_url ?? '';
	return {
		useGlobal: extraction === null,
		enabled: extraction?.enabled ?? null,
		baseUrl,
		loadedBaseUrl: baseUrl,
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
 * Whether the form still points at the endpoint the stored key was entered against. An
 * edit that is typed and then undone lands back here, so the key survives it.
 */
export function sameEndpoint(form: ExtractionForm): boolean {
	return form.baseUrl.trim() === form.loadedBaseUrl.trim();
}

/**
 * True when saving as things stand would drop the stored key: the base URL has moved off
 * the one the key belongs to and no new key has been typed. A key is never sent to a host
 * it was not meant for, and the card says so before Save rather than after.
 */
export function keyWillDrop(form: ExtractionForm): boolean {
	return !form.useGlobal && form.storedKey && !form.apiKey && !sameEndpoint(form);
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
		enabled: form.enabled,
		base_url: form.baseUrl.trim() || null,
		model: form.model.trim() || null,
		api_key: form.apiKey ? form.apiKey : form.storedKey && sameEndpoint(form) ? MASKED_KEY : null,
		auto_accept_min_confidence:
			form.threshold.trim() === '' || !Number.isFinite(threshold) ? null : threshold
	};
}

/** What the key prefix confirm asks before a rename touches every task in the project. */
export function keyPrefixConfirm(tasks: number, from: string, to: string): string {
	return `Rename ${plural(tasks, 'task')} from ${from}- to ${to}-?`;
}
