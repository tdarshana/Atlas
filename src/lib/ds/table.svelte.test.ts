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

		const header = container.querySelector('.header-cell.sortable')!;

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
});
