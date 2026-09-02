// Practices and workflows are the same shape behind the same routes, so one
// store is built per kind and kept as a module-level singleton.

import { api } from '$lib/daemon.svelte';
import type { Doc, DocKind, NewDoc } from '$lib/types';

export interface DocsState {
	list: Doc[];
	loading: boolean;
	error: string | null;
	loaded: boolean;
}

export interface DocsStore {
	readonly kind: DocKind;
	readonly state: DocsState;
	load(): Promise<void>;
	save(d: NewDoc): Promise<Doc>;
	remove(name: string): Promise<void>;
}

function message(e: unknown): string {
	return e instanceof Error ? e.message : String(e);
}

export function createDocsStore(kind: DocKind): DocsStore {
	const state = $state<DocsState>({ list: [], loading: false, error: null, loaded: false });

	return {
		kind,
		state,

		async load() {
			state.loading = true;
			try {
				state.list = await api().listDocs(kind);
				state.error = null;
				state.loaded = true;
			} catch (e) {
				state.error = message(e);
			} finally {
				state.loading = false;
			}
		},

		/** Creates or replaces a doc; the caller reports the error to the user. */
		async save(d: NewDoc) {
			const saved = await api().saveDoc(kind, d);
			const i = state.list.findIndex((x) => x.name === saved.name);
			if (i >= 0) state.list[i] = saved;
			else state.list.push(saved);
			return saved;
		},

		async remove(name: string) {
			await api().deleteDoc(kind, name);
			const i = state.list.findIndex((x) => x.name === name);
			if (i >= 0) state.list.splice(i, 1);
		}
	};
}

export const practices = createDocsStore('practice');
export const workflows = createDocsStore('workflow');
