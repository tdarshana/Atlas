// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
	applySort,
	loadTableState,
	nextSort,
	reorder,
	saveTableState,
	type TableColumn
} from './table';

interface Row {
	name: string;
	count: number;
}

const columns: TableColumn<Row>[] = [
	{ key: 'name', label: 'Name', sortable: true },
	{ key: 'count', label: 'Count', sortable: true }
];

const rows: Row[] = [
	{ name: 'beta', count: 2 },
	{ name: 'alpha', count: 30 },
	{ name: 'gamma', count: 1 }
];

describe('applySort', () => {
	it('sorts strings ascending and descending with localeCompare', () => {
		expect(applySort(rows, columns, { key: 'name', dir: 'asc' }).map((r) => r.name)).toEqual([
			'alpha',
			'beta',
			'gamma'
		]);
		expect(applySort(rows, columns, { key: 'name', dir: 'desc' }).map((r) => r.name)).toEqual([
			'gamma',
			'beta',
			'alpha'
		]);
	});

	it('sorts numbers numerically, not lexicographically', () => {
		expect(applySort(rows, columns, { key: 'count', dir: 'asc' }).map((r) => r.count)).toEqual([
			1, 2, 30
		]);
		expect(applySort(rows, columns, { key: 'count', dir: 'desc' }).map((r) => r.count)).toEqual([
			30, 2, 1
		]);
	});

	it('returns the input order when sort is null', () => {
		expect(applySort(rows, columns, null)).toBe(rows);
	});

	it('uses the column\'s own compare function when given one', () => {
		const byLength: TableColumn<Row>[] = [
			{ key: 'name', label: 'Name', sortable: true, sort: (a, b) => a.name.length - b.name.length }
		];
		const withDifferentLengths: Row[] = [
			{ name: 'aa', count: 0 },
			{ name: 'a', count: 0 },
			{ name: 'aaa', count: 0 }
		];
		expect(
			applySort(withDifferentLengths, byLength, { key: 'name', dir: 'asc' }).map((r) => r.name)
		).toEqual(['a', 'aa', 'aaa']);
	});
});

describe('nextSort', () => {
	it('cycles asc, desc, null for the same column', () => {
		let sort = nextSort(null, 'name');
		expect(sort).toEqual({ key: 'name', dir: 'asc' });
		sort = nextSort(sort, 'name');
		expect(sort).toEqual({ key: 'name', dir: 'desc' });
		sort = nextSort(sort, 'name');
		expect(sort).toBeNull();
	});

	it('starts a different column at asc', () => {
		const sort = nextSort({ key: 'name', dir: 'desc' }, 'count');
		expect(sort).toEqual({ key: 'count', dir: 'asc' });
	});
});

describe('reorder', () => {
	it('moves a column from one index to another', () => {
		expect(reorder(['a', 'b', 'c'], 0, 2)).toEqual(['b', 'c', 'a']);
		expect(reorder(['a', 'b', 'c'], 2, 0)).toEqual(['c', 'a', 'b']);
	});

	it('is a no-op moving a column onto itself', () => {
		expect(reorder(['a', 'b', 'c'], 1, 1)).toEqual(['a', 'b', 'c']);
	});

	it('clamps an out-of-range destination to the last position', () => {
		expect(reorder(['a', 'b', 'c'], 0, 99)).toEqual(['b', 'c', 'a']);
	});

	it('clamps a negative source or destination to the first position', () => {
		expect(reorder(['a', 'b', 'c'], -5, 1)).toEqual(['b', 'a', 'c']);
		expect(reorder(['a', 'b', 'c'], 2, -5)).toEqual(['c', 'a', 'b']);
	});

	it('does not mutate the input array', () => {
		const input = ['a', 'b', 'c'];
		reorder(input, 0, 2);
		expect(input).toEqual(['a', 'b', 'c']);
	});
});

describe('table state persistence', () => {
	beforeEach(() => {
		localStorage.clear();
	});

	it('round trips through a fake localStorage', () => {
		saveTableState('recent', { order: ['age', 'kind'], sort: { key: 'age', dir: 'desc' } });
		expect(localStorage.getItem('atlas.table.recent')).toBeTruthy();
		expect(loadTableState('recent')).toEqual({
			order: ['age', 'kind'],
			sort: { key: 'age', dir: 'desc' }
		});
	});

	it('returns null when nothing is stored', () => {
		expect(loadTableState('nothing-yet')).toBeNull();
	});

	it('tolerates a localStorage that throws on read and on write', () => {
		const getItem = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
			throw new Error('site data blocked');
		});
		const setItem = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
			throw new Error('site data blocked');
		});
		expect(() => saveTableState('recent', { order: ['age'], sort: null })).not.toThrow();
		expect(loadTableState('recent')).toBeNull();
		getItem.mockRestore();
		setItem.mockRestore();
	});
});
