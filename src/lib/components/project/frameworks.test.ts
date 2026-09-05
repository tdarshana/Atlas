import { describe, expect, it } from 'vitest';
import type { FrameworkDoc, FrameworkInventory, FrameworkListing } from '$lib/types';

import { DOC_TYPE_TONE, docRowKey, documentRows, reportText, sidePanelFrameworks } from './frameworks';

function inventory(overrides: Partial<FrameworkInventory> = {}): FrameworkInventory {
	return {
		kind: 'superpowers',
		roots: ['docs/superpowers/specs'],
		docs: 1,
		tasks: 0,
		detected_at: '2026-09-04T00:00:00Z',
		...overrides
	};
}

function doc(overrides: Partial<FrameworkDoc> = {}): FrameworkDoc {
	return {
		kind: 'superpowers',
		path: 'docs/superpowers/plans/x.md',
		title: 'x',
		doc_type: 'plan',
		updated_at: '2026-09-01T00:00:00Z',
		...overrides
	};
}

describe('documentRows', () => {
	it('flattens every listing into one list', () => {
		const listings: FrameworkListing[] = [
			{ inventory: inventory(), documents: [doc({ path: 'a' }), doc({ path: 'b' })] },
			{
				inventory: inventory({ kind: 'openspec' }),
				documents: [doc({ kind: 'openspec', path: 'c' })]
			}
		];
		expect(documentRows(listings).map((d) => d.path)).toEqual(['a', 'b', 'c']);
	});

	it('sorts newest first', () => {
		const listings: FrameworkListing[] = [
			{
				inventory: inventory(),
				documents: [
					doc({ path: 'old', updated_at: '2026-09-01T00:00:00Z' }),
					doc({ path: 'new', updated_at: '2026-09-03T00:00:00Z' })
				]
			}
		];
		expect(documentRows(listings).map((d) => d.path)).toEqual(['new', 'old']);
	});

	it('maps no detected frameworks to no rows', () => {
		expect(documentRows([])).toEqual([]);
	});
});

describe('docRowKey', () => {
	it('combines kind and path, since two frameworks can share a path', () => {
		expect(docRowKey(doc({ kind: 'superpowers', path: 'plans/x.md' }))).toBe(
			'superpowers:plans/x.md'
		);
		expect(docRowKey(doc({ kind: 'openspec', path: 'plans/x.md' }))).toBe('openspec:plans/x.md');
	});
});

describe('DOC_TYPE_TONE', () => {
	it('gives every document type a tone', () => {
		const types: (keyof typeof DOC_TYPE_TONE)[] = [
			'spec',
			'plan',
			'tasks',
			'roadmap',
			'ledger',
			'proposal',
			'summary',
			'todo'
		];
		for (const t of types) expect(DOC_TYPE_TONE[t]).toBeTruthy();
	});

	it('gives tasks and todo the same warning tone, since both are actionable', () => {
		expect(DOC_TYPE_TONE.tasks).toBe('warning');
		expect(DOC_TYPE_TONE.todo).toBe('warning');
	});
});

describe('sidePanelFrameworks', () => {
	it('projects each listing down to its inventory, in order', () => {
		const listings: FrameworkListing[] = [
			{ inventory: inventory({ kind: 'superpowers' }), documents: [doc()] },
			{ inventory: inventory({ kind: 'gsd', docs: 4 }), documents: [] }
		];
		expect(sidePanelFrameworks(listings)).toEqual([
			inventory({ kind: 'superpowers' }),
			inventory({ kind: 'gsd', docs: 4 })
		]);
	});

	it('maps no detected frameworks to no rows', () => {
		expect(sidePanelFrameworks([])).toEqual([]);
	});
});

describe('reportText', () => {
	it('reads as "N created, N updated, N skipped"', () => {
		expect(reportText({ created: 3, updated: 1, skipped: 12, reparented: 0 })).toBe(
			'3 created, 1 updated, 12 skipped'
		);
	});

	it('reads the same shape when every count is zero', () => {
		expect(reportText({ created: 0, updated: 0, skipped: 0, reparented: 0 })).toBe(
			'0 created, 0 updated, 0 skipped'
		);
	});
});
