// The narrow slice of the daemon's API a plugin can reach, and the real implementation
// behind it.
//
// The bridge depends on this interface rather than on `AtlasApi` so a test can hand it a
// stub, and so the six operations a plugin may perform are listed in one place. Every
// request carries `X-Atlas-Actor: plugin/<id>`, which is how the daemon labels the audit
// rows and task events a plugin causes; the routes and body shapes are the same ones
// `src/lib/api.ts` uses for the same operations.

import { baseUrl } from '$lib/daemon.svelte';
import type { Memory, MemoryKind, NewMemory, Settings, Task, TaskFilter } from '$lib/types';

/** A memory a plugin asks to be remembered. Scope and kind default on the host side. */
export interface PluginRemember {
	text: string;
	kind?: MemoryKind;
	tags?: string[];
}

/** A task a plugin asks to be created. */
export interface PluginNewTask {
	title: string;
	description?: string;
	kind?: string;
	priority?: string;
	project_id?: string | null;
}

export interface PluginBackend {
	searchMemories(query: string, limit?: number): Promise<unknown>;
	remember(input: PluginRemember): Promise<Memory>;
	listTasks(filter: TaskFilter): Promise<Task[]>;
	createTask(input: PluginNewTask): Promise<Task>;
	moveTask(key: string, stage: string): Promise<Task>;
	getSetting(key: string): Promise<unknown>;
}

/** A plugin's memory has no project of its own, so it lands in the global scope. */
const DEFAULT_SCOPE = 'global';
/** The kind a plugin gets when it names none. */
const DEFAULT_KIND: MemoryKind = 'insight';

function query(params: Record<string, string | null | undefined>): string {
	const q = new URLSearchParams();
	for (const [k, v] of Object.entries(params)) if (v != null && v !== '') q.set(k, v);
	const s = q.toString();
	return s ? `?${s}` : '';
}

/** The daemon's `error` field, falling back to the body or the status line. */
function errorText(text: string, res: Response): string {
	try {
		const parsed = JSON.parse(text);
		if (parsed && typeof parsed.error === 'string') return parsed.error;
	} catch {
		// Not JSON; the body itself is the best message available.
	}
	return text || `${res.status} ${res.statusText}`;
}

/** The real backend for `pluginId`, talking to the daemon the rest of the app talks to. */
export function pluginBackend(pluginId: string): PluginBackend {
	const actor = `plugin/${pluginId}`;

	async function req<T>(method: string, path: string, body?: unknown): Promise<T> {
		const headers: Record<string, string> = {
			Accept: 'application/json',
			'X-Atlas-Actor': actor
		};
		if (body !== undefined) headers['Content-Type'] = 'application/json';
		const res = await fetch(`${baseUrl()}${path}`, {
			method,
			headers,
			body: body === undefined ? undefined : JSON.stringify(body)
		});
		const text = await res.text();
		if (!res.ok) throw new Error(errorText(text, res));
		if (res.status === 204 || text === '') return undefined as T;
		return JSON.parse(text) as T;
	}

	return {
		searchMemories: (q, limit) => req('POST', '/api/v1/memories/search', { query: q, limit }),
		remember: (input) =>
			req<Memory>('POST', '/api/v1/memories', {
				scope: DEFAULT_SCOPE,
				kind: input.kind ?? DEFAULT_KIND,
				text: input.text,
				tags: input.tags ?? []
			} satisfies NewMemory),
		listTasks: (filter) =>
			req<Task[]>(
				'GET',
				`/api/v1/tasks${query({
					project_id: filter.project_id,
					stage: filter.stage,
					include_done: filter.include_done ? 'true' : null
				})}`
			),
		createTask: (input) => req<Task>('POST', '/api/v1/tasks', input),
		moveTask: (key, stage) =>
			req<Task>('POST', `/api/v1/tasks/${encodeURIComponent(key)}/move`, { stage }),
		getSetting: async (key) => {
			const settings = await req<Settings>('GET', '/api/v1/settings');
			return settings[key];
		}
	};
}
