// The palette's query language: what counts as a finished scope, what opens the
// project picker, and which prefix narrows the kind.

import { describe, expect, it } from 'vitest';
import type { Project } from '$lib/types';
import { parseQuery, resolveScopes, stripScopes } from './parse';

function project(id: string, name: string): Project {
	return {
		id,
		name,
		root_path: `/tmp/${name}`,
		git_remote: null,
		profile: null,
		created_at: '2026-09-03T00:00:00Z',
		last_seen_at: '2026-09-03T00:00:00Z',
		board_key: null,
		board_stages: null
	};
}

describe('parseQuery', () => {
	it('lifts a finished @name into a scope and keeps the rest as text', () => {
		expect(parseQuery('@atlas duckdb')).toEqual({
			scopes: ['atlas'],
			type: 'all',
			text: 'duckdb'
		});
	});

	it('opens the project picker on a trailing partial', () => {
		expect(parseQuery('@atl')).toEqual({ scopes: [], type: 'project-picker', text: 'atl' });
	});

	it('opens the picker on a bare @', () => {
		expect(parseQuery('@')).toEqual({ scopes: [], type: 'project-picker', text: '' });
	});

	it('reads # as tasks', () => {
		expect(parseQuery('#ready')).toEqual({ scopes: [], type: 'task', text: 'ready' });
	});

	it('reads ~ as memories', () => {
		expect(parseQuery('~cors')).toEqual({ scopes: [], type: 'memory', text: 'cors' });
	});

	it('reads > as commands', () => {
		expect(parseQuery('>tog')).toEqual({ scopes: [], type: 'command', text: 'tog' });
	});

	it('trims the space after a prefix', () => {
		expect(parseQuery('# duckdb')).toEqual({ scopes: [], type: 'task', text: 'duckdb' });
	});

	it('stacks scopes and preserves their case', () => {
		expect(parseQuery('@atlas @Habitmaker x')).toEqual({
			scopes: ['atlas', 'Habitmaker'],
			type: 'all',
			text: 'x'
		});
	});

	it('finishes a scope on the trailing space', () => {
		expect(parseQuery('@atlas ')).toEqual({ scopes: ['atlas'], type: 'all', text: '' });
	});

	it('treats an empty query as all', () => {
		expect(parseQuery('')).toEqual({ scopes: [], type: 'all', text: '' });
	});
});

describe('stripScopes', () => {
	it('removes finished scopes and keeps the text', () => {
		expect(stripScopes('@atlas duckdb')).toBe('duckdb');
	});

	it('keeps a partial the picker is still filtering on', () => {
		expect(stripScopes('@atl')).toBe('@atl');
	});
});

describe('resolveScopes', () => {
	const items = [project('p1', 'atlas'), project('p2', 'Habitmaker')];

	it('matches project names case-insensitively', () => {
		expect(resolveScopes(['ATLAS'], items)).toEqual([{ name: 'ATLAS', id: 'p1' }]);
	});

	it('leaves an unknown name unresolved so the chip can warn', () => {
		expect(resolveScopes(['nope'], items)).toEqual([{ name: 'nope', id: null }]);
	});
});
