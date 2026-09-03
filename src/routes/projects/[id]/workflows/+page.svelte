<script lang="ts">
	// The Workflows tab (frame 02.5): this project's workflows, with real triggers, action
	// counts and run history now that the graph editor and the runner have landed.
	import { goto } from '$app/navigation';
	import { Badge, Button, Table, type TableColumn } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage, errorLogPath } from '$lib/errors';
	import { dateTime, duration, plural } from '$lib/format';
	import { setStatusItems } from '$lib/shell';
	import { project, setHeaderActions } from '$lib/stores/project.svelte';
	import {
		createWorkflow,
		RUN_STATUS_LABEL,
		RUN_STATUS_TONE,
		TRIGGER_TONE
	} from '$lib/stores/workflows.svelte';
	import type { Workflow, WorkflowRun } from '$lib/types';
	import { lastRunLabel, workflowRows, type WorkflowRow } from '$lib/components/project/workflows';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	/** How many of the project's most recent runs, across every one of its workflows,
	 * the Recent runs card shows. */
	const RECENT_RUNS = 10;

	let workflows = $state<Workflow[]>([]);
	let recentRuns = $state<WorkflowRun[]>([]);
	let loading = $state(false);
	let error = $state<string | null>(null);
	let logPath = $state<string | null>(null);

	const name = $derived(project.current?.name ?? 'Project');
	const id = $derived(project.current?.id ?? '');
	const rows = $derived(workflowRows(workflows));

	const columns: TableColumn<WorkflowRow>[] = [
		{ key: 'name', label: 'Name', width: '200px', sortable: true },
		{ key: 'trigger', label: 'Trigger' },
		{ key: 'actions', label: 'Actions', width: '90px', align: 'right' },
		{ key: 'lastRun', label: 'Last run', width: '160px', align: 'right' }
	];

	/** The last `RECENT_RUNS` runs across every one of the project's workflows, newest
	 * first. A workflow whose own run list fails to load just contributes none, rather
	 * than failing the whole card. */
	async function loadRecentRuns(list: Workflow[]): Promise<WorkflowRun[]> {
		if (list.length === 0) return [];
		const perWorkflow = await Promise.all(
			list.map((w) => api().listRuns(w.id, RECENT_RUNS).catch(() => [] as WorkflowRun[]))
		);
		return perWorkflow
			.flat()
			.sort((a, b) => (a.started_at < b.started_at ? 1 : -1))
			.slice(0, RECENT_RUNS);
	}

	async function load(): Promise<void> {
		if (!id) return;
		loading = true;
		try {
			workflows = await api().listWorkflows(id);
			recentRuns = await loadRecentRuns(workflows);
			error = null;
			logPath = null;
		} catch (e) {
			workflows = [];
			recentRuns = [];
			error = errorMessage(e);
			logPath = errorLogPath(e);
		} finally {
			loading = false;
		}
	}

	async function newWorkflow(): Promise<void> {
		try {
			const created = await createWorkflow(id || null);
			await goto(`/workflows/${created.id}`);
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	function openEditor(): void {
		void goto(workflows.length ? `/workflows/${workflows[0].id}` : '/workflows');
	}

	$effect(() => {
		if (id) void load();
	});

	$effect(() => {
		setHeaderActions(headerActions);
		return () => setHeaderActions(null);
	});

	$effect(() => {
		// `recentRuns` is capped at `RECENT_RUNS` per workflow (see `loadRecentRuns`), not a
		// true project-wide total, and there is no cheap way to ask the daemon for one
		// without a further route: `listRuns` returns a bare array, no total count. Labelled
		// "recent runs" rather than "runs" so the number never reads as a claim it isn't.
		setStatusItems({
			right: [
				{
					text: `${name} · ${plural(rows.length, 'workflow')} · ${plural(recentRuns.length, 'recent run')}`
				}
			]
		});
	});
</script>

{#snippet headerActions()}
	<Button data-testid="workflows-editor" onclick={openEditor}>Open editor</Button>
	<Button variant="primary" data-testid="workflows-new" onclick={newWorkflow}>New workflow</Button>
{/snippet}

{#if error}
	<ErrorState message={error} logPath={logPath ?? undefined}>
		<Button variant="primary" onclick={() => load()}>Retry</Button>
	</ErrorState>
{:else}
	<div class="stack">
		<Table id="project-workflows" {columns} {rows} rowKey={(r: WorkflowRow) => r.id}>
			{#snippet cell(row: WorkflowRow, column: TableColumn<WorkflowRow>)}
				{#if column.key === 'name'}
					<a class="link" href={`/workflows/${row.id}`}>{row.name}</a>
				{:else if column.key === 'trigger'}
					<span class="trigger-cell">
						<Badge tone={TRIGGER_TONE[row.triggerKind]}>{row.triggerKind}</Badge>
						{#if row.cron}<span class="mono cron">{row.cron}</span>{/if}
					</span>
				{:else if column.key === 'actions'}
					{row.actions}
				{:else if column.key === 'lastRun'}
					{#if row.lastRunStatus}
						<span class="last-run">
							<Badge tone={RUN_STATUS_TONE[row.lastRunStatus]}>{RUN_STATUS_LABEL[row.lastRunStatus]}</Badge>
							<span class="mono age">{lastRunLabel(row)}</span>
						</span>
					{:else}
						<span class="muted">never</span>
					{/if}
				{/if}
			{/snippet}
			{#snippet empty()}
				<div class="empty">
					<span>{loading ? 'Loading…' : 'No workflows yet.'}</span>
				</div>
			{/snippet}
		</Table>

		<div class="card">
			<div class="head">
				<span class="group-heading">Recent runs</span>
				<span class="spacer"></span>
				<span class="mono count">{plural(recentRuns.length, 'run')}</span>
			</div>
			{#if recentRuns.length === 0}
				<div class="empty">
					<span class="title">No runs yet</span>
				</div>
			{:else}
				<div class="runs-columns">
					<span>STARTED</span>
					<span>TRIGGER</span>
					<span>STATUS</span>
					<span class="right">DURATION</span>
				</div>
				<div class="runs">
					{#each recentRuns as run (run.id)}
						<div class="runs-row">
							<span class="mono">{dateTime(run.started_at)}</span>
							<span class="trigger-text">{run.trigger}</span>
							<span><Badge tone={RUN_STATUS_TONE[run.status]}>{RUN_STATUS_LABEL[run.status]}</Badge></span>
							<span class="mono right">{duration(run.started_at, run.finished_at)}</span>
						</div>
					{/each}
				</div>
			{/if}
		</div>
	</div>
{/if}

<style>
	.stack {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}

	.card {
		background: var(--bg-raised);
		border: 1px solid var(--border-default);
		border-radius: 5px;
		overflow: hidden;
		display: flex;
		flex-direction: column;
	}

	.head {
		height: 32px;
		flex: 0 0 32px;
		display: flex;
		align-items: center;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.spacer {
		flex: 1;
	}

	.count {
		color: var(--text-tertiary);
	}

	.link {
		color: var(--accent);
		font-family: var(--font-mono);
	}

	.muted {
		color: var(--text-tertiary);
	}

	.trigger-cell,
	.last-run {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	.cron,
	.age {
		color: var(--text-secondary);
	}

	.empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 4px;
		padding: 20px 0;
		color: var(--text-secondary);
	}

	.title {
		font-weight: 600;
		color: var(--text-primary);
	}

	.runs-columns,
	.runs-row {
		display: grid;
		grid-template-columns: 160px 120px 100px 1fr;
		align-items: center;
		column-gap: 12px;
		height: 28px;
		padding: 0 12px;
	}

	.runs-columns {
		flex: 0 0 28px;
		border-bottom: 1px solid var(--border-subtle);
		font-size: 11px;
		font-weight: 600;
		letter-spacing: 0.04em;
		color: var(--text-tertiary);
	}

	.runs {
		overflow: auto;
	}

	.runs-row {
		border-bottom: 1px solid var(--border-subtle);
	}

	.runs-row:last-child {
		border-bottom: 0;
	}

	.trigger-text {
		color: var(--text-secondary);
	}

	.right {
		text-align: right;
	}
</style>
