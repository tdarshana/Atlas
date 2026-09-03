<script module lang="ts">
	export interface TableColumn {
		key: string;
		label: string;
		width?: string;
		align?: 'left' | 'right';
	}
</script>

<script lang="ts" generics="Row">
	import type { Snippet } from 'svelte';
	import type { HTMLTableAttributes } from 'svelte/elements';

	interface Props extends HTMLTableAttributes {
		columns: TableColumn[];
		rows: Row[];
		/** Rendered once per cell with the row and the column key. */
		cell: Snippet<[Row, string]>;
		/** Stable key per row; falls back to the index. */
		rowKey?: (row: Row, index: number) => string | number;
		/** Shown in place of the body when `rows` is empty. */
		empty?: Snippet;
		onrowclick?: (row: Row) => void;
	}

	let {
		columns,
		rows,
		cell,
		rowKey,
		empty,
		onrowclick,
		class: klass = '',
		...rest
	}: Props = $props();

	/**
	 * A clickable row is reachable by keyboard, so Enter and Space have to do what a
	 * click does. Space is also the page-scroll key, so its default is suppressed once
	 * the row has focus, matching how a button behaves.
	 */
	function activate(event: KeyboardEvent, row: Row) {
		if (event.key !== 'Enter' && event.key !== ' ') return;
		event.preventDefault();
		onrowclick?.(row);
	}
</script>

<div class="wrap {klass}">
	<table {...rest}>
		<thead>
			<tr>
				{#each columns as column (column.key)}
					<th style:width={column.width} style:text-align={column.align ?? 'left'}>
						{column.label}
					</th>
				{/each}
			</tr>
		</thead>
		<tbody>
			{#each rows as row, i (rowKey ? rowKey(row, i) : i)}
				<tr
					class:clickable={!!onrowclick}
					role={onrowclick ? 'button' : undefined}
					tabindex={onrowclick ? 0 : undefined}
					onclick={() => onrowclick?.(row)}
					onkeydown={onrowclick ? (e) => activate(e, row) : undefined}
				>
					{#each columns as column (column.key)}
						<td style:text-align={column.align ?? 'left'}>{@render cell(row, column.key)}</td>
					{/each}
				</tr>
			{:else}
				{#if empty}
					<tr>
						<td class="empty" colspan={columns.length}>{@render empty()}</td>
					</tr>
				{/if}
			{/each}
		</tbody>
	</table>
</div>

<style>
	.wrap {
		overflow-x: auto;
		border: 1px solid var(--border-default);
		border-radius: var(--radius-md);
		background: var(--bg-raised);
	}

	table {
		width: 100%;
		border-collapse: collapse;
	}

	th {
		height: 32px;
		padding: 0 var(--space-3);
		border-bottom: 1px solid var(--border-subtle);
		font-size: 11px;
		font-weight: 600;
		color: var(--text-tertiary);
		text-transform: uppercase;
		letter-spacing: 0.04em;
		white-space: nowrap;
	}

	td {
		height: 28px;
		padding: 0 var(--space-3);
		border-bottom: 1px solid var(--border-subtle);
		vertical-align: middle;
	}

	tbody tr:last-child td {
		border-bottom: none;
	}

	tr.clickable {
		cursor: default;
	}

	tr.clickable:hover td {
		background: var(--bg-hover);
	}

	/* An outline on the row itself is clipped by the cells, so the focus ring is drawn
	   as an inset shadow on the first and last cell and a tint across the whole row. */
	tr.clickable:focus-visible {
		outline: none;
	}

	tr.clickable:focus-visible td {
		background: var(--bg-hover);
		box-shadow:
			inset 0 2px 0 var(--accent),
			inset 0 -2px 0 var(--accent);
	}

	tr.clickable:focus-visible td:first-child {
		box-shadow:
			inset 2px 0 0 var(--accent),
			inset 0 2px 0 var(--accent),
			inset 0 -2px 0 var(--accent);
	}

	tr.clickable:focus-visible td:last-child {
		box-shadow:
			inset -2px 0 0 var(--accent),
			inset 0 2px 0 var(--accent),
			inset 0 -2px 0 var(--accent);
	}

	td.empty {
		padding: 0;
	}
</style>
