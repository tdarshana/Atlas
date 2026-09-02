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
				<tr class:clickable={!!onrowclick} onclick={() => onrowclick?.(row)}>
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
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--bg-elev);
	}

	table {
		width: 100%;
		border-collapse: collapse;
	}

	th {
		padding: var(--space-2) var(--space-3);
		border-bottom: 1px solid var(--border);
		font-size: 12px;
		font-weight: 600;
		color: var(--muted);
		text-transform: uppercase;
		letter-spacing: 0.04em;
		white-space: nowrap;
	}

	td {
		padding: var(--space-2) var(--space-3);
		border-bottom: 1px solid var(--border);
		vertical-align: top;
	}

	tbody tr:last-child td {
		border-bottom: none;
	}

	tr.clickable {
		cursor: pointer;
	}

	tr.clickable:hover td {
		background: var(--bg-hover);
	}

	td.empty {
		padding: 0;
	}
</style>
