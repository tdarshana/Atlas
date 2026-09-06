// The daemon's change stream. One EventSource on `GET /api/v1/events` for the life of
// the app; every write the daemon makes arrives here as an event named by its entity
// (`task`, `memory`, ...) the moment it lands, and the stores that show that entity
// subscribe and refresh themselves. Polling stays only as the fallback for the seconds
// the stream is down: the browser reconnects an EventSource on its own.

import { baseUrl, reauth, tokenQuery } from '$lib/daemon.svelte';
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
/** True once a stream has been opened before, so the next open is a reopen and the
 * subscribers are told to refresh: events may have landed while the stream was down. */
let openedBefore = false;

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
	// The token goes in the query: an `EventSource` cannot carry a header (SEC-5).
	source = new EventSource(`${baseUrl()}/api/v1/events${tokenQuery()}`);
	source.onopen = () => {
		changes.connected = true;
		// A reopened stream has a gap in front of it: whatever the daemon wrote while
		// the stream was down never arrived. Tell every subscriber to refresh wholesale,
		// the same way a `lagged` event from the daemon does.
		if (openedBefore) {
			dispatch({ entity: 'lagged', action: 'reconnected', id: null, key: null, project_id: null, at: new Date().toISOString() });
		}
		openedBefore = true;
	};
	source.onerror = () => {
		changes.connected = false;
		// The browser retries an EventSource by itself, but with the same URL: after a
		// daemon restart the token in that URL is stale and every retry is a 401. Ask the
		// host for the current token and, when it changed, reopen the stream with it.
		// Only the transport is replaced: the subscribers stay, or every mounted view
		// would go quiet after the first daemon restart while the stream reads as open.
		void reauth().then((token) => {
			if (token && source) {
				closeStream();
				connectChanges();
			}
		});
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

/** Closes the transport and nothing else; the subscribers wait for the next open. */
function closeStream(): void {
	source?.close();
	source = null;
	changes.connected = false;
}

/** Closes the stream and forgets every listener: full teardown, for tests. A daemon
 * restart goes through the reconnect path above instead, which keeps the listeners. */
export function disconnectChanges(): void {
	closeStream();
	listeners.clear();
	openedBefore = false;
}
