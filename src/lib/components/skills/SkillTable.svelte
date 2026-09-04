<script lang="ts">
	// The skills table, drawn the same way in the global view and the project tab. The
	// project tab is the only caller that asks for the Scope and `Enabled here` columns,
	// so both are optional rather than two near-identical tables.
	import { Badge, Checkbox, Table, type TableColumn } from '$lib/ds';
	import { relativeAge } from '$lib/format';
	import { sourceLabel } from '$lib/skills';
	import type { SkillSummary } from '$lib/types';

	interface Props {
		id: string;
		rows: SkillSummary[];
		selectedId?: string | null;
		loading?: boolean;
		emptyText?: string;
		onopen: (row: SkillSummary) => void;
		/** The project tab passes both; the global view passes neither. */
		showScope?: boolean;
		ontoggle?: (row: SkillSummary, enabled: boolean) => void;
		/** The id being written right now, so its checkbox is held still. */
		toggling?: string | null;
	}

	let {
		id,
		rows,
		selectedId = null,
		loading = false,
		emptyText = 'No skills found.',
		onopen,
		showScope = false,
		ontoggle,
		toggling = null
	}: Props = $props();

	const columns = $derived<TableColumn<SkillSummary>[]>([
		{ key: 'name', label: 'Name', width: '200px', mono: true, sortable: true },
		{ key: 'description', label: 'Description' },
		{ key: 'source', label: 'Source', width: '120px', sortable: true },
		...(showScope
			? [{ key: 'scope', label: 'Scope', width: '80px', sortable: true } as TableColumn<SkillSummary>]
			: []),
		{ key: 'updated_at', label: 'Updated', width: '110px', sortable: true },
		...(ontoggle
			? [{ key: 'enabled_here', label: 'Enabled here', width: '110px' } as TableColumn<SkillSummary>]
			: [])
	]);
</script>

<div class="skills-table" data-testid="{id}-wrap">
	<Table
		{id}
		{columns}
		{rows}
		rowKey={(s: SkillSummary) => s.id}
		selectedKey={selectedId}
		onRowClick={onopen}
	>
		{#snippet cell(row: SkillSummary, column: TableColumn<SkillSummary>)}
			{#if column.key === 'name'}
				<span class="name" class:off={row.enabled_here === false}>{row.name}</span>
			{:else if column.key === 'description'}
				<span
					class="description"
					class:off={row.enabled_here === false}
					title={row.description}
					data-testid="skill-description-{row.id}"
				>{row.description}</span>
			{:else if column.key === 'source'}
				<Badge
					variant="outline"
					title={row.plugin ?? undefined}
					data-testid="skill-source-{row.id}"
				>
					{sourceLabel(row.source)}
				</Badge>
			{:else if column.key === 'scope'}
				{row.scope}
			{:else if column.key === 'updated_at'}
				{row.updated_at ? `${relativeAge(row.updated_at)} ago` : '—'}
			{:else if column.key === 'enabled_here'}
				<Checkbox
					checked={row.enabled_here !== false}
					disabled={toggling === row.id}
					aria-label={`Enable ${row.name} here`}
					data-testid="skill-toggle-{row.id}"
					onclick={(e) => e.stopPropagation()}
					onchange={(e) => ontoggle?.(row, e.currentTarget.checked)}
				/>
			{/if}
		{/snippet}
		{#snippet empty()}
			<span class="hint">{loading ? 'Loading…' : emptyText}</span>
		{/snippet}
	</Table>
</div>

<style>
	.skills-table {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		border: 1px solid var(--border-subtle);
		border-radius: 3px;
		overflow: hidden;
	}

	/* Two lines and then an ellipsis: a skill's description is a sentence, not a word, and
	   some of them run to a paragraph. The DS row is a fixed 28px with `align-items:
	   center`, so a cell taller than that is centred and spills over the rows above and
	   below rather than being clipped by the cell's own `overflow: hidden`. The line
	   height is therefore set so two lines fit inside the row (2 x 13px = 26px), and
	   `max-height` holds that even if a font override changes the metrics. The full text
	   is on the cell's `title`. */
	.description {
		display: -webkit-box;
		-webkit-box-orient: vertical;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		overflow: hidden;
		white-space: normal;
		font-size: 11px;
		line-height: 13px;
		max-height: 26px;
	}

	/* A skill this project has turned off is still listed, just visibly not in play. */
	.off {
		color: var(--text-tertiary);
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}
</style>
