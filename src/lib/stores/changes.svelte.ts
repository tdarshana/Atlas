// The daemon's change stream. One EventSource on `GET /api/v1/events` for the life of
// the app; every write the daemon makes arrives here as an event named by its entity
// (`task`, `memory`, ...) the moment it lands, and the stores that show that entity
// subscribe and refresh themselves. Polling stays only as the fallback for the seconds
// the stream is down: the browser reconnects an EventSource on its own.

import { baseUrl } from '$lib/daemon.svelte';
import type { Change } from '$lib/types';

export const changes = $state({
	/** True while the stream is open; false before the first open and while reconnecting. */
	connected: false,
	/** How many changes have arrived since the stream was opened, for tests and the status line. */
	received: 0
});

type Listener = (change: Change) => void;
const listeners = new Map<string, Set<Listener>>();
let source: EventSource | null = null;

/** The entities the stream names; `lagged` means the daemon dropped events and the
 * listener should refresh wholesale. Any other entity is delivered under its own name. */
export const ENTITIES = ['task', 'memory', 'project', 'agent', 'persona', 'lagged'] as const;

/** Calls `fn` for every change to `entity` (or to anything, for `*`). Returns the unsubscribe. */
export function onChange(entity: string, fn: Listener): () => void {
	let set = listeners.get(entity);
	if (!set) {
		set = new Set();
		listeners.set(entity, set);
	}
	set.add(fn);
	return () => {
		set.delete(fn);
	};
}

/** Hands a change to its entity's listeners and to the `*` listeners. Exported for tests. */
export function dispatch(change: Change): void {
	changes.received += 1;
	for (const key of [change.entity, '*']) {
		for (const fn of listeners.get(key) ?? []) fn(change);
	}
}

function onEvent(e: MessageEvent<string>): void {
	let change: Change;
	try {
		change = JSON.parse(e.data) as Change;
	} catch {
		return;
	}
	dispatch(change);
}

/** Opens the stream (once). Safe to call again; a second call is a no-op. */
export function connectChanges(): void {
	if (source || typeof EventSource === 'undefined') return;
	source = new EventSource(`${baseUrl()}/api/v1/events`);
	source.onopen = () => {
		changes.connected = true;
	};
	source.onerror = () => {
		changes.connected = false;
	};
	for (const entity of ENTITIES) {
		if (entity === 'lagged') {
			source.addEventListener('lagged', () =>
				dispatch({ entity: 'lagged', action: 'lagged', id: null, key: null, project_id: null, at: new Date().toISOString() })
			);
		} else {
			source.addEventListener(entity, onEvent as EventListener);
		}
	}
}

/** Closes the stream and forgets every listener. For tests and a daemon restart. */
export function disconnectChanges(): void {
	source?.close();
	source = null;
	changes.connected = false;
	listeners.clear();
}
