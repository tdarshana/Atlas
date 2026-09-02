// Agent list state plus the name rule the backend enforces. The list is a
// module-level rune so a save on the editor page is visible when the list page
// mounts again without a refetch round trip.

import { api } from '$lib/daemon.svelte';
import { errorMessage } from '$lib/errors';
import type { Agent, NewAgent } from '$lib/types';

/** Mirrors the backend rule for agent, practice and workflow names. */
export const NAME_PATTERN = /^[a-z0-9][a-z0-9-_]*$/;
export const NAME_MAX = 64;

/** The reason `name` is not a legal name, or null when it is one. */
export function nameError(name: string): string | null {
	if (name === '') return 'A name is required';
	if (name.length > NAME_MAX) return `A name is at most ${NAME_MAX} characters`;
	if (!NAME_PATTERN.test(name)) {
		return 'Use lowercase letters, digits, - and _, starting with a letter or digit';
	}
	return null;
}

/** Splits a comma-separated field into trimmed, non-empty entries. */
export function parseList(text: string): string[] {
	return text
		.split(',')
		.map((s) => s.trim())
		.filter((s) => s !== '');
}

export const agents = $state({
	list: [] as Agent[],
	loading: false,
	error: null as string | null,
	loaded: false
});

export async function loadAgents(): Promise<void> {
	agents.loading = true;
	try {
		agents.list = await api().listAgents();
		agents.error = null;
		agents.loaded = true;
	} catch (e) {
		agents.error = errorMessage(e);
	} finally {
		agents.loading = false;
	}
}

/** Creates or replaces an agent; the caller reports the error to the user. */
export async function saveAgent(a: NewAgent): Promise<Agent> {
	const saved = await api().saveAgent(a);
	const i = agents.list.findIndex((x) => x.name === saved.name);
	if (i >= 0) agents.list[i] = saved;
	else agents.list.push(saved);
	return saved;
}

export async function deleteAgent(name: string): Promise<void> {
	await api().deleteAgent(name);
	const i = agents.list.findIndex((x) => x.name === name);
	if (i >= 0) agents.list.splice(i, 1);
}
