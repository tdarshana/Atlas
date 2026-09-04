<script lang="ts">
	// The Run history tab's left card (frame 08.1): a filterable runs table, the
	// selected run highlighted with the accent-muted fill and edge the design system
	// uses for selection everywhere else.
	import { Badge, Select } from '$lib/ds';
	import { duration, shortDateTime } from '$lib/format';
	import {
		filterRuns,
		RUN_FILTER_OPTIONS,
		RUN_STATUS_LABEL,
		RUN_STATUS_TONE,
		TRIGGER_TONE,
		type RunFilterValue
	} from '$lib/stores/workflows.svelte';
	import type { WorkflowRun } from '$lib/types';

	let {
		runs,
		selectedId,
		onSelect
	}: { runs: WorkflowRun[]; selectedId: string | null; onSelect: (id: string) => void } =
		$props();

	let filter = $state<RunFilterValue>('all');

	const filtered = $derived(filterRuns(runs, filter));
</script>

<div class="card">
	<div class="head">
		<span class="group-heading">Runs</span>
		<span class="spacer"></span>
		<div class="filter">
			<Select
				size="sm"
				options={RUN_FILTER_OPTIONS}
				value={filter}
				aria-label="Filter runs"
				onchange={(e) => (filter = e.currentTarget.value as RunFilterValue)}
			/>
		</div>
	</div>
	<div class="columns">
		<span class="col num">#</span>
		<span class="col">STARTED</span>
		<span class="col">TRIGGER</span>
		<span class="col">STATUS</span>
		<span class="col right">DURATION</span>
	</div>
	<div class="rows">
		{#if filtered.length === 0}
			<p class="empty">No runs yet</p>
		{:else}
			{#each filtered as run (run.id)}
				<button
					type="button"
					class="run-row"
					class:selected={run.id === selectedId}
					onclick={() => onSelect(run.id)}
					data-testid={`run-row-${run.number}`}
				>
					<span class="mono num">{run.number}</span>
					<span class="mono started">{shortDateTime(run.started_at)}</span>
					<span class="trigger">
						<Badge tone={TRIGGER_TONE[run.trigger]}>{run.trigger}</Badge>
					</span>
					<span class="status">
						<Badge tone={RUN_STATUS_TONE[run.status]}>{RUN_STATUS_LABEL[run.status]}</Badge>
					</span>
					<span class="mono right duration">{duration(run.started_at, run.finished_at)}</span>
				</button>
			{/each}
		{/if}
	</div>
</div>

<style>
	.card {
		width: 420px;
		flex: 0 0 420px;
		background: var(--bg-raised);
		border: 1px solid var(--border-default);
		border-radius: 5px;
		overflow: hidden;
		display: flex;
		flex-direction: column;
		min-height: 0;
	}

	.head {
		height: 32px;
		flex: 0 0 32px;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.spacer {
		flex: 1;
	}

	.filter {
		width: 120px;
	}

	.columns,
	.run-row {
		display: grid;
		/* Frame 08.1's own runs-table grid (`#`/Started/Trigger/Status/Duration). */
		grid-template-columns: 32px 100px 76px 62px 1fr;
		align-items: center;
		column-gap: 10px;
		height: 28px;
		padding: 0 12px;
	}

	.columns {
		flex: 0 0 28px;
		border-bottom: 1px solid var(--border-subtle);
		font-size: 11px;
		font-weight: 600;
		letter-spacing: 0.04em;
		color: var(--text-tertiary);
	}

	.col.right {
		text-align: right;
	}

	.rows {
		flex: 1;
		overflow: auto;
	}

	.run-row {
		width: 100%;
		border: 0;
		border-bottom: 1px solid var(--border-subtle);
		background: transparent;
		color: inherit;
		font: inherit;
		text-align: left;
		cursor: pointer;
	}

	.run-row:hover {
		background: var(--bg-hover);
	}

	.run-row.selected {
		background: var(--accent-muted);
		box-shadow: inset 2px 0 0 var(--accent);
	}

	.num {
		color: var(--text-tertiary);
	}

	.duration {
		color: var(--text-secondary);
		white-space: nowrap;
	}

	.right {
		text-align: right;
	}

	.empty {
		margin: 0;
		padding: 20px 0;
		text-align: center;
		color: var(--text-secondary);
	}
</style>
