<script lang="ts" generics="T">
	// Desktop table: 32px header with dividers and a sort indicator, 28px rows, drag to
	// reorder columns with a 2px accent drop indicator. Order and sort persist under
	// `atlas.table.<id>`.
	import { untrack, type Snippet } from 'svelte';
	import Icon from './Icon.svelte';
	import {
		applySort,
		loadTableState,
		nextSort,
		reorder,
		saveTableState,
		type SortState,
		type TableColumn
	} from './table';

	interface Props {
		id: string;
		columns: TableColumn<T>[];
		rows: T[];
		rowKey: (row: T) => string;
		onRowClick?: (row: T) => void;
		selectedKey?: string | null;
		emptyText?: string;
		cell?: Snippet<[T, TableColumn<T>]>;
		empty?: Snippet;
	}

	let {
		id,
		columns,
		rows,
		rowKey,
		onRowClick,
		selectedKey = null,
		emptyText = 'Nothing here yet.',
		cell,
		empty
	}: Props = $props();

	/** Keeps a persisted order valid against today's columns: drop keys that no longer
	    exist, append ones the caller added since. */
	function reconcile(saved: string[], cols: TableColumn<T>[]): string[] {
		const known = new Set(cols.map((c) => c.key));
		const kept = saved.filter((k) => known.has(k));
		const missing = cols.map((c) => c.key).filter((k) => !kept.includes(k));
		return [...kept, ...missing];
	}

	interface InitialState {
		order: string[];
		sort: SortState | null;
	}

	// `id` and `columns` are read once here, on purpose: the persisted order and sort
	// seed the initial state, not every render.
	const initial: InitialState = untrack(() => {
		const stored = loadTableState(id);
		return {
			order: reconcile(stored?.order ?? columns.map((c) => c.key), columns),
			sort:
				stored?.sort && columns.some((c) => c.key === stored.sort!.key && c.sortable)
					? stored.sort
					: null
		};
	});

	let order = $state<string[]>(initial.order);
	let sort = $state<SortState | null>(initial.sort);

	let dragKey = $state<string | null>(null);
	let dropKey = $state<string | null>(null);
	let dropBefore = $state(true);

	const visibleColumns = $derived(
		order
			.map((key) => columns.find((c) => c.key === key))
			.filter((c): c is TableColumn<T> => !!c)
	);
	const gridTemplate = $derived(visibleColumns.map((c) => c.width ?? '1fr').join(' '));
	const sortedRows = $derived(applySort(rows, columns, sort));

	function persist(): void {
		saveTableState(id, { order, sort });
	}

	function onHeaderClick(column: TableColumn<T>): void {
		if (!column.sortable) return;
		sort = nextSort(sort, column.key);
		persist();
	}

	function onDragStart(key: string, event: DragEvent): void {
		dragKey = key;
		event.dataTransfer?.setData('text/plain', key);
	}

	function onDragOver(key: string, event: DragEvent): void {
		if (!dragKey || dragKey === key) return;
		event.preventDefault();
		const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
		dropKey = key;
		dropBefore = event.clientX - rect.left < rect.width / 2;
	}

	function onDrop(key: string, event: DragEvent): void {
		event.preventDefault();
		const from = dragKey;
		resetDrag();
		if (!from || from === key) return;
		const fromIndex = order.indexOf(from);
		const targetIndex = order.indexOf(key);
		let to = dropBefore ? targetIndex : targetIndex + 1;
		if (fromIndex < to) to -= 1;
		order = reorder(order, fromIndex, to);
		persist();
	}

	function resetDrag(): void {
		dragKey = null;
		dropKey = null;
	}

	function activate(event: KeyboardEvent, row: T): void {
		if (event.key !== 'Enter' && event.key !== ' ') return;
		event.preventDefault();
		onRowClick?.(row);
	}

	function cellValue(row: T, column: TableColumn<T>): string {
		return String((row as Record<string, unknown>)[column.key] ?? '');
	}
</script>

<div class="table">
	<div class="header" style:grid-template-columns={gridTemplate}>
		{#each visibleColumns as column (column.key)}
			<button
				type="button"
				class="header-cell group-heading"
				class:sortable={column.sortable}
				class:drop-before={dropKey === column.key && dropBefore}
				class:drop-after={dropKey === column.key && !dropBefore}
				style:justify-content={column.align === 'right' ? 'flex-end' : 'flex-start'}
				draggable="true"
				ondragstart={(e) => onDragStart(column.key, e)}
				ondragover={(e) => onDragOver(column.key, e)}
				ondrop={(e) => onDrop(column.key, e)}
				ondragend={resetDrag}
				onclick={() => onHeaderClick(column)}
			>
				{#if sort?.key === column.key}<Icon name="arrow-down" size={11} />{/if}
				<span>{column.label}</span>
			</button>
		{/each}
	</div>

	{#snippet rowCells(row: T)}
		{#each visibleColumns as column (column.key)}
			<div class="cell" class:mono={column.mono} style:text-align={column.align ?? 'left'}>
				{#if cell}{@render cell(row, column)}{:else}{cellValue(row, column)}{/if}
			</div>
		{/each}
	{/snippet}

	{#if sortedRows.length === 0}
		<div class="empty">
			{#if empty}{@render empty()}{:else}<p>{emptyText}</p>{/if}
		</div>
	{:else}
		<div class="body">
			{#each sortedRows as row (rowKey(row))}
				{#if onRowClick}
					<div
						class="row clickable"
						class:selected={selectedKey != null && rowKey(row) === selectedKey}
						style:grid-template-columns={gridTemplate}
						tabindex="0"
						role="button"
						onclick={() => onRowClick(row)}
						onkeydown={(e) => activate(e, row)}
					>
						{@render rowCells(row)}
					</div>
				{:else}
					<div
						class="row"
						class:selected={selectedKey != null && rowKey(row) === selectedKey}
						style:grid-template-columns={gridTemplate}
					>
						{@render rowCells(row)}
					</div>
				{/if}
			{/each}
		</div>
	{/if}
</div>

<style>
	.table {
		display: flex;
		flex-direction: column;
		min-height: 0;
	}

	.header {
		display: grid;
		align-items: center;
		height: 32px;
		flex: 0 0 32px;
		padding: 0 12px;
		column-gap: 12px;
		border-bottom: 1px solid var(--border-subtle);
		user-select: none;
	}

	.header-cell {
		display: flex;
		align-items: center;
		gap: 4px;
		height: 100%;
		padding: 0 8px 0 0;
		margin: 0;
		border: 0;
		border-right: 1px solid var(--border-subtle);
		background: transparent;
		cursor: default;
		font: inherit;
		text-align: inherit;
	}

	.header-cell:last-child {
		border-right: 0;
		padding-right: 0;
	}

	.header-cell.sortable {
		cursor: pointer;
	}

	.header-cell.drop-before {
		box-shadow: inset 2px 0 0 var(--accent);
	}

	.header-cell.drop-after {
		box-shadow: inset -2px 0 0 var(--accent);
	}

	.body {
		overflow-y: auto;
		min-height: 0;
	}

	.row {
		display: grid;
		align-items: center;
		height: 28px;
		flex: 0 0 28px;
		padding: 0 12px;
		column-gap: 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.row:last-child {
		border-bottom: 0;
	}

	.row.clickable {
		cursor: pointer;
	}

	.row.clickable:hover {
		background: var(--bg-hover);
	}

	.row.selected {
		background: var(--accent-muted);
	}

	.row.clickable:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: -2px;
	}

	.cell {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.cell.mono {
		font-family: var(--font-mono);
		font-variant-numeric: var(--tabular);
	}

	.empty {
		display: flex;
		align-items: center;
		justify-content: center;
		padding: var(--space-6) var(--space-4);
		color: var(--text-secondary);
	}

	.empty p {
		margin: 0;
	}
</style>
