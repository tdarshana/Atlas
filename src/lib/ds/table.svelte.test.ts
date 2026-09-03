// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { cleanup, fireEvent, render } from '@testing-library/svelte';

import Table from './Table.svelte';

interface Row {
	id: string;
	name: string;
}

const rows: Row[] = [
	{ id: '1', name: 'beta' },
	{ id: '2', name: 'alpha' },
	{ id: '3', name: 'gamma' }
];

const columns = [
	{ key: 'name', label: 'Name', sortable: true },
	{ key: 'id', label: 'Id' }
];

// `render()` calls a generic component from plain TS, where `T` cannot be inferred
// from a template, so `rowKey` has to type-check against `unknown` at the call site.
const rowKey = (row: unknown) => (row as Row).id;

afterEach(cleanup);

beforeEach(() => {
	localStorage.clear();
});

describe('Table', () => {
	it('renders one row per item with a cell per column', () => {
		const { container } = render(Table, {
			props: { id: 'test-basic', columns, rows, rowKey }
		});

		expect(container.querySelectorAll('.row')).toHaveLength(3);
		expect(container.querySelectorAll('.row')[0].querySelectorAll('.cell')).toHaveLength(2);
	});

	it('sorts ascending then descending on repeated header clicks', async () => {
		const { container } = render(Table, {
			props: { id: 'test-sort', columns, rows, rowKey }
		});

		const header = container.querySelector('.header-cell.sortable .header-button')!;

		await fireEvent.click(header);
		let names = [...container.querySelectorAll('.row')].map((r) => r.querySelector('.cell')!.textContent);
		expect(names).toEqual(['alpha', 'beta', 'gamma']);

		await fireEvent.click(header);
		names = [...container.querySelectorAll('.row')].map((r) => r.querySelector('.cell')!.textContent);
		expect(names).toEqual(['gamma', 'beta', 'alpha']);
	});

	it('gives rows tabindex 0 only when onRowClick is provided', () => {
		const clickable = render(Table, {
			props: { id: 'test-tab-a', columns, rows, rowKey, onRowClick: () => {} }
		});
		expect(clickable.container.querySelector('.row')!.getAttribute('tabindex')).toBe('0');
		cleanup();

		const plain = render(Table, {
			props: { id: 'test-tab-b', columns, rows, rowKey }
		});
		expect(plain.container.querySelector('.row')!.getAttribute('tabindex')).toBeNull();
	});

	it('shows the empty text when there are no rows', () => {
		const { getByText } = render(Table, {
			props: { id: 'test-empty', columns, rows: [], rowKey, emptyText: 'No rows.' }
		});
		expect(getByText('No rows.')).toBeTruthy();
	});

	it('carries grid semantics: grid, rowgroup, row, columnheader, gridcell', () => {
		const { container } = render(Table, {
			props: { id: 'test-grid', columns, rows, rowKey }
		});

		expect(container.querySelector('.table')!.getAttribute('role')).toBe('grid');
		// Both the header row and the data rows sit inside a rowgroup, so the grid owns
		// nothing but rows and rowgroups.
		expect(container.querySelector('.header')!.getAttribute('role')).toBe('rowgroup');
		expect(container.querySelector('.body')!.getAttribute('role')).toBe('rowgroup');
		expect(container.querySelector('.header-row')!.getAttribute('role')).toBe('row');
		expect(container.querySelectorAll('.header-cell').length).toBeGreaterThan(0);
		for (const header of container.querySelectorAll('.header-cell')) {
			expect(header.getAttribute('role')).toBe('columnheader');
			// The activator inside it stays a real button.
			expect(header.querySelector('button.header-button')).toBeTruthy();
		}
		const row = container.querySelector('.row')!;
		expect(row.getAttribute('role')).toBe('row');
		for (const cell of row.querySelectorAll('.cell')) {
			expect(cell.getAttribute('role')).toBe('gridcell');
		}
	});

	it('reports aria-sort on the sorted column and none on other sortable columns', async () => {
		const { container } = render(Table, {
			props: { id: 'test-aria-sort', columns, rows, rowKey }
		});

		const sortableHeader = container.querySelector('.header-cell.sortable')!;
		const sortButton = sortableHeader.querySelector('.header-button')!;
		expect(sortableHeader.getAttribute('aria-sort')).toBe('none');

		await fireEvent.click(sortButton);
		expect(sortableHeader.getAttribute('aria-sort')).toBe('ascending');

		await fireEvent.click(sortButton);
		expect(sortableHeader.getAttribute('aria-sort')).toBe('descending');

		// The Id column is not sortable, so it carries no aria-sort at all.
		const idHeader = [...container.querySelectorAll('.header-cell')].find(
			(h) => h.textContent?.trim() === 'Id'
		)!;
		expect(idHeader.getAttribute('aria-sort')).toBeNull();
	});

	it('seeds the sort from defaultSort when nothing is persisted, and shows the indicator', () => {
		const { container } = render(Table, {
			props: {
				id: 'test-default-sort',
				columns,
				rows,
				rowKey,
				defaultSort: { key: 'name', dir: 'asc' }
			}
		});

		const names = [...container.querySelectorAll('.row')].map(
			(r) => r.querySelector('.cell')!.textContent
		);
		expect(names).toEqual(['alpha', 'beta', 'gamma']);

		const sortableHeader = container.querySelector('.header-cell.sortable')!;
		expect(sortableHeader.getAttribute('aria-sort')).toBe('ascending');
		expect(sortableHeader.querySelector('svg')!.classList.contains('lucide-arrow-up')).toBe(
			true
		);
	});

	it('flips the indicator icon between ascending and descending', async () => {
		const { container } = render(Table, {
			props: { id: 'test-indicator', columns, rows, rowKey }
		});

		const sortableHeader = container.querySelector('.header-cell.sortable')!;
		const sortButton = sortableHeader.querySelector('.header-button')!;

		await fireEvent.click(sortButton);
		expect(sortableHeader.querySelector('svg')!.classList.contains('lucide-arrow-up')).toBe(
			true
		);

		await fireEvent.click(sortButton);
		expect(sortableHeader.querySelector('svg')!.classList.contains('lucide-arrow-down')).toBe(
			true
		);
	});

	it('lets a persisted sort win over defaultSort', () => {
		localStorage.setItem(
			'atlas.table.test-persisted-sort',
			JSON.stringify({ order: ['name', 'id'], sort: { key: 'name', dir: 'desc' } })
		);

		const { container } = render(Table, {
			props: {
				id: 'test-persisted-sort',
				columns,
				rows,
				rowKey,
				defaultSort: { key: 'name', dir: 'asc' }
			}
		});

		const names = [...container.querySelectorAll('.row')].map(
			(r) => r.querySelector('.cell')!.textContent
		);
		expect(names).toEqual(['gamma', 'beta', 'alpha']);
	});

	it('moves a column with Alt+ArrowRight on a focused header', async () => {
		const { container } = render(Table, {
			props: { id: 'test-keyboard-reorder', columns, rows, rowKey }
		});

		const nameHeader = container.querySelector('.header-cell.sortable')!;
		expect([...container.querySelectorAll('.header-cell')].indexOf(nameHeader)).toBe(0);

		await fireEvent.keyDown(nameHeader.querySelector('.header-button')!, {
			key: 'ArrowRight',
			altKey: true
		});

		const headersAfter = [...container.querySelectorAll('.header-cell')];
		expect(headersAfter.map((h) => h.textContent?.trim())).toEqual(['Id', 'Name']);
	});
});
