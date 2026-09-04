// Pure helpers behind the Project settings tab: the shapes the three editable cards read
// from a `Project` and write back to the daemon. No Svelte here, so these are plain unit
// tests.

import { plural } from '$lib/format';
import type { AgentAccess, ProjectExtraction } from '$lib/types';

/** `^[A-Z][A-Z0-9]{1,5}$`, the same rule the daemon applies to a board key. */
export const KEY_PREFIX_RE = /^[A-Z][A-Z0-9]{1,5}$/;

/** What the daemon sends in place of a stored key, and what means "leave it alone". */
export const MASKED_KEY = '***';

/**
 * The two agent labels the card offers before a project has a log to learn from. They
 * are `cli/*`, so the daemon exempts them either way, but the frame lists them and a
 * card that offered nothing on a fresh project would read as broken.
 */
export const DEFAULT_ACTORS = ['cli/claude-code', 'cli/codex'];

/**
 * Labels that are Atlas itself rather than an agent: the daemon's own HTTP default, the
 * bare CLI, the sync writer, the desktop and the workflow runner. They all write to the
 * log, and none of them is something a rule can usefully name, so the card hides them.
 */
export const SYSTEM_ACTORS = ['api', 'cli', 'sync', 'desktop', 'workflow'];

/**
 * Whether the daemon exempts this label from both allowlists and from `require_review`.
 * Mirrors `atlas_core::projects::actor_is_user`: the user's own hands, matched exactly
 * so `cline` is still an agent.
 */
export function alwaysAllowed(actor: string): boolean {
	const a = actor.trim();
	return a === 'desktop' || a === 'api' || a === 'cli' || a.startsWith('cli/');
}

/**
 * The labels the Agent access card offers a checkbox for: the two defaults, whatever has
 * written to this project's log, and anything already on either allowlist, so a rule the
 * user set from the CLI is still visible here. Atlas's own system labels are dropped:
 * ticking `api` would say nothing, since the daemon never checks it.
 */
export function knownActors(sources: string[], access: AgentAccess | null): string[] {
	const all = [
		...DEFAULT_ACTORS,
		...sources,
		...(access?.memory_writers ?? []),
		...(access?.task_movers ?? [])
	];
	return [...new Set(all.map((a) => a.trim()).filter(Boolean))]
		.filter((a) => !SYSTEM_ACTORS.includes(a))
		.sort((a, b) => a.localeCompare(b));
}

/**
 * The key prefix a project's tasks carry today. Mirrors `board_key_base`: the first three
 * alphanumerics of the name, uppercased and padded, which is what the daemon renames
 * *from* when the stored `board_key` is null (a project connected before migration 3).
 * A name with nothing to take falls back to the global board's key.
 */
export function boardKeyBase(name: string): string {
	const letters = (name.match(/[a-zA-Z0-9]/g) ?? []).slice(0, 3).join('').toUpperCase();
	if (!letters) return 'ATLAS';
	return letters.padEnd(3, 'X');
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
