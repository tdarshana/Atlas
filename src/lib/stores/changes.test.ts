// The change stream: listeners by entity, the wildcard, the lagged marker, and the
// EventSource wiring, with a fake EventSource standing in for the browser's.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Change } from '$lib/types';

vi.mock('$lib/daemon.svelte', () => ({
	daemon: { port: 7433, ready: true, error: null, logPath: '' },
	baseUrl: () => 'http://127.0.0.1:7433',
	tokenQuery: () => '?token=test-token',
	api: () => ({}),
	boot: async () => {},
	reauth: async () => null
}));

import { changes, connectChanges, disconnectChanges, dispatch, onChange } from './changes.svelte';

class FakeSource {
	static instances: FakeSource[] = [];
	url: string;
	onopen: (() => void) | null = null;
	onerror: (() => void) | null = null;
	listeners = new Map<string, EventListener[]>();
	closed = false;
	constructor(url: string) {
		this.url = url;
		FakeSource.instances.push(this);
	}
	addEventListener(name: string, fn: EventListener) {
		this.listeners.set(name, [...(this.listeners.get(name) ?? []), fn]);
	}
	emit(name: string, data: string) {
		for (const fn of this.listeners.get(name) ?? []) fn({ data } as MessageEvent);
	}
	close() {
		this.closed = true;
	}
}

function change(entity: string, action = 'moved', extra: Partial<Change> = {}): Change {
	return { entity, action, id: 'id-1', key: 'ATL-1', project_id: null, at: '2026-09-05T00:00:00Z', ...extra };
}

beforeEach(() => {
	FakeSource.instances = [];
	vi.stubGlobal('EventSource', FakeSource);
	changes.received = 0;
});

afterEach(() => {
	disconnectChanges();
	vi.unstubAllGlobals();
});

describe('the change stream', () => {
	it('hands a change to its entity listeners and to the wildcard, and unsubscribes cleanly', () => {
		const tasks: string[] = [];
		const all: string[] = [];
		const stop = onChange('task', (c) => tasks.push(c.action));
		onChange('*', (c) => all.push(c.entity));

		dispatch(change('task'));
		dispatch(change('memory', 'insert'));
		expect(tasks).toEqual(['moved']);
		expect(all).toEqual(['task', 'memory']);
		expect(changes.received).toBe(2);

		stop();
		dispatch(change('task'));
		expect(tasks).toEqual(['moved']);
	});

	it('opens one EventSource on the daemon, delivers named events, and marks lag', () => {
		const heard: Change[] = [];
		onChange('task', (c) => heard.push(c));
		onChange('lagged', (c) => heard.push(c));

		connectChanges();
		connectChanges();
		expect(FakeSource.instances).toHaveLength(1);
		const source = FakeSource.instances[0];
		// SEC-5: an EventSource cannot carry a header, so the token rides in the query.
		expect(source.url).toBe('http://127.0.0.1:7433/api/v1/events?token=test-token');

		source.onopen?.();
		expect(changes.connected).toBe(true);
		source.emit('task', JSON.stringify(change('task', 'created', { key: 'ATL-7' })));
		source.emit('task', 'not json');
		source.emit('lagged', '3');
		expect(heard.map((c) => [c.entity, c.action, c.key])).toEqual([
			['task', 'created', 'ATL-7'],
			['lagged', 'lagged', null]
		]);

		source.onerror?.();
		expect(changes.connected).toBe(false);
		disconnectChanges();
		expect(source.closed).toBe(true);
	});
});
