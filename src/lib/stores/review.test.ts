import { beforeEach, describe, expect, it } from 'vitest';
import { qualifying, review } from './review.svelte';
import { settings } from './settings.svelte';
import type { Memory, MemoryKind } from '$lib/types';

function pending(id: string, confidence: number, kind: MemoryKind = 'fact'): Memory {
	return {
		id,
		scope: 'global',
		project_id: null,
		kind,
		text: `memory ${id}`,
		tags: [],
		source_agent: 'cli/codex',
		source_tool: null,
		confidence,
		status: 'pending',
		superseded_by: null,
		created_at: '2026-09-03T09:00:00Z',
		updated_at: '2026-09-03T09:00:00Z'
	};
}

const ids = (rows: Memory[]) => rows.map((m) => m.id);

describe('qualifying', () => {
	beforeEach(() => {
		settings.values = {};
		review.items = [];
	});

	it('takes the rows at or above the threshold', () => {
		settings.values = { 'extraction.auto_accept_min_confidence': 0.8 };
		review.items = [pending('a', 0.95), pending('b', 0.8), pending('c', 0.79)];
		expect(ids(qualifying())).toEqual(['a', 'b']);
	});

	it('accepts nothing at the default threshold unless a row is fully confident', () => {
		// The unconfigured default is 1.0, so an unconfigured Atlas asks about every memory.
		review.items = [pending('a', 0.99), pending('b', 1)];
		expect(ids(qualifying())).toEqual(['b']);
	});

	it('takes every row at a threshold of zero', () => {
		settings.values = { 'extraction.auto_accept_min_confidence': 0 };
		review.items = [pending('a', 0), pending('b', 0.4)];
		expect(ids(qualifying())).toEqual(['a', 'b']);
	});

	it('reads a threshold the daemon stored as a string', () => {
		settings.values = { 'extraction.auto_accept_min_confidence': '0.5' };
		review.items = [pending('a', 0.5), pending('b', 0.49)];
		expect(ids(qualifying())).toEqual(['a']);
	});

	it('is empty when nothing is pending', () => {
		settings.values = { 'extraction.auto_accept_min_confidence': 0 };
		expect(qualifying()).toEqual([]);
	});
});
