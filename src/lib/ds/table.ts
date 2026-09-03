// Pure helpers for the desktop Table: sorting, column reordering and the
// `atlas.table.<id>` localStorage persistence. No Svelte here, so these are plain
// unit tests.

import { persistSet } from '$lib/shell/persist';

export interface TableColumn<T> {
	key: string;
	label: string;
	width?: string;
	align?: 'left' | 'right';
	/** Mono for identifiers, values and counts. */
	mono?: boolean;
	sortable?: boolean;
	/** Overrides the default compare when the column's value is not a plain field. */
	sort?: (a: T, b: T) => number;
}

export interface SortState {
	key: string;
	dir: 'asc' | 'desc';
}

export interface TableState {
	order: string[];
	sort: SortState | null;
}

const KEY_PREFIX = 'atlas.table.';

/**
 * Numeric fields compare numerically; everything else compares with `localeCompare`,
 * `numeric: true` so "item2" sorts before "item10", `sensitivity: 'base'` so case does
 * not affect the order.
 */
function defaultCompare<T>(key: string): (a: T, b: T) => number {
	return (a, b) => {
		const av = (a as Record<string, unknown>)[key];
		const bv = (b as Record<string, unknown>)[key];
		if (typeof av === 'number' && typeof bv === 'number') return av - bv;
		return String(av ?? '').localeCompare(String(bv ?? ''), undefined, {
			numeric: true,
			sensitivity: 'base'
		});
	};
}

/**
 * Sorts `rows` by `sort`, using the column's own `sort` function when it has one.
 * Returns `rows` in its input order when `sort` is null or names a column that is
 * not there. `desc` negates the comparator rather than sorting ascending and reversing
 * the array, so rows tied on the key keep their relative order either way (a stable
 * sort reversed in bulk would flip tied rows against each other).
 */
export function applySort<T>(rows: T[], columns: TableColumn<T>[], sort: SortState | null): T[] {
	if (!sort) return rows;
	const column = columns.find((c) => c.key === sort.key);
	if (!column) return rows;
	const compare = column.sort ?? defaultCompare<T>(column.key);
	const directional = sort.dir === 'desc' ? (a: T, b: T) => -compare(a, b) : compare;
	return [...rows].sort(directional);
}

/** asc -> desc -> null for the same column; a different column always starts at asc. */
export function nextSort(current: SortState | null, key: string): SortState | null {
	if (!current || current.key !== key) return { key, dir: 'asc' };
	if (current.dir === 'asc') return { key, dir: 'desc' };
	return null;
}

/** Moves the entry at `from` to end up at `to`, clamping both to valid indices. */
export function reorder(order: string[], from: number, to: number): string[] {
	const result = [...order];
	const clampedFrom = Math.max(0, Math.min(from, result.length - 1));
	const [moved] = result.splice(clampedFrom, 1);
	if (moved === undefined) return result;
	const clampedTo = Math.max(0, Math.min(to, result.length));
	result.splice(clampedTo, 0, moved);
	return result;
}

export function loadTableState(id: string): TableState | null {
	try {
		if (typeof localStorage === 'undefined') return null;
		const raw = localStorage.getItem(KEY_PREFIX + id);
		if (!raw) return null;
		return JSON.parse(raw) as TableState;
	} catch {
		return null;
	}
}

export function saveTableState(id: string, state: TableState): void {
	try {
		if (typeof localStorage === 'undefined') return;
		localStorage.setItem(KEY_PREFIX + id, JSON.stringify(state));
	} catch {
		/* storage is unavailable; order and sort are simply not remembered */
	}
	void persistSet(KEY_PREFIX + id, state);
}
