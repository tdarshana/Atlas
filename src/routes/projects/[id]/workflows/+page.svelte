<script lang="ts">
	// The Workflows tab (frame 02.5): this project's workflow docs, read as rows. Phase 9
	// replaces the data source with real triggers and run history; until then every doc is
	// one manual, never-run workflow and the editor lives on the global `/workflows` route.
	import { goto } from '$app/navigation';
	import { Badge, Button, Table, type TableColumn } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage, errorLogPath } from '$lib/errors';
	import { setStatusItems } from '$lib/shell';
	import { project, setHeaderActions } from '$lib/stores/project.svelte';
	import type { Doc } from '$lib/types';
	import { workflowRows, type WorkflowRow } from '$lib/components/project/workflows';
	import ErrorState from '$lib/ui/ErrorState.svelte';

	let docs = $state<Doc[]>([]);
	let loading = $state(false);
	let error = $state<string | null>(null);
	let logPath = $state<string | null>(null);

	const name = $derived(project.current?.name ?? 'Project');
	const id = $derived(project.current?.id ?? '');
	const rows = $derived(workflowRows(docs));

	const columns: TableColumn<WorkflowRow>[] = [
		{ key: 'name', label: 'Name', width: '200px', sortable: true },
		{ key: 'trigger', label: 'Trigger' },
		{ key: 'actions', label: 'Actions', width: '90px', align: 'right' },
		{ key: 'lastRun', label: 'Last run', width: '160px', align: 'right' }
	];

	async function load(): Promise<void> {
		if (!id) return;
		loading = true;
		try {
			docs = await api().listDocs('workflow', id);
			error = null;
			logPath = null;
		} catch (e) {
			docs = [];
			error = errorMessage(e);
			logPath = errorLogPath(e);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		if (id) void load();
	});

	$effect(() => {
		setHeaderActions(headerActions);
		return () => setHeaderActions(null);
	});

	$effect(() => {
		setStatusItems({
			right: [{ text: `${name} · ${rows.length} workflow${rows.length === 1 ? '' : 's'} · 0 runs` }]
		});
	});
</script>

{#snippet headerActions()}
	<Button data-testid="workflows-editor" onclick={() => goto('/workflows')}>Open editor</Button>
	<Button variant="primary" data-testid="workflows-new" onclick={() => goto('/workflows')}>
		New workflow
	</Button>
{/snippet}

{#if error}
	<ErrorState message={error} logPath={logPath ?? undefined}>
		<Button variant="primary" onclick={() => load()}>Retry</Button>
	</ErrorState>
{:else}
	<div class="stack">
		<Table id="project-workflows" {columns} {rows} rowKey={(r: WorkflowRow) => r.name}>
			{#snippet cell(row: WorkflowRow, column: TableColumn<WorkflowRow>)}
				{#if column.key === 'name'}
					<a class="link" href="/workflows">{row.name}</a>
				{:else if column.key === 'trigger'}
					<Badge>manual</Badge>
				{:else if column.key === 'actions'}
					{row.actions}
				{:else}
					<span class="muted">never</span>
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
				<span class="mono count">0 runs</span>
			</div>
			<div class="empty">
				<span class="title">No runs yet</span>
			</div>
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
</style>
