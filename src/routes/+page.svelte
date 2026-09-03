<script lang="ts">
	// Dashboard: the daemon's status report, the three counts and the newest
	// memories. The active-memory count is the one the status report already carries;
	// projects and agents have no such counter, so those come from their list routes.
	import { onMount } from 'svelte';
	import { api } from '$lib/daemon.svelte';
	import { errorLogPath, errorMessage } from '$lib/errors';
	import { relativeAge } from '$lib/format';
	import { status } from '$lib/stores/status.svelte';
	import type { Memory, StageCount } from '$lib/types';
	import Badge from '$lib/ui/Badge.svelte';
	import Button from '$lib/ui/Button.svelte';
	import Card from '$lib/ui/Card.svelte';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import Table from '$lib/ui/Table.svelte';

	const RECENT = 10;

	const report = $derived(status.report);
	const embedding = $derived(report?.embedding ?? '');
	const tone: 'success' | 'accent' | 'danger' = $derived(
		embedding.startsWith('ready') ? 'success' : embedding.startsWith('loading') ? 'accent' : 'danger'
	);

	let counts = $state({
		projects: null as number | null,
		agents: null as number | null
	});
	let recent = $state<Memory[]>([]);
	let openTasks = $state<StageCount[]>([]);
	let tasksLoading = $state(true);
	let tasksError = $state<string | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let loadErrorLogPath = $state<string | null>(null);

	const columns = [
		{ key: 'kind', label: 'Kind', width: '110px' },
		{ key: 'text', label: 'Text' },
		{ key: 'age', label: 'Age', width: '80px', align: 'right' as const }
	];

	async function load() {
		loading = true;
		try {
			const client = api();
			const [active, projectList, agentList] = await Promise.all([
				client.listMemories('active'),
				client.listProjects(),
				client.listAgents()
			]);
			counts = { projects: projectList.length, agents: agentList.length };
			recent = [...active]
				.sort((a, b) => b.created_at.localeCompare(a.created_at))
				.slice(0, RECENT);
			error = null;
			loadErrorLogPath = null;
		} catch (e) {
			recent = [];
			error = errorMessage(e);
			loadErrorLogPath = errorLogPath(e);
		} finally {
			loading = false;
		}
	}

	/**
	 * The board counts on their own, so a board that cannot answer costs this page one
	 * card rather than all of it. The card just stays empty; the board screen is where
	 * a board failure is worth explaining.
	 */
	async function loadTasks() {
		tasksLoading = true;
		try {
			const client = api();
			const [stageList, stageCounts] = await Promise.all([
				client.boardStages(),
				client.taskCounts()
			]);
			// A done column is finished work, so only the other stages are open tasks.
			const doneStages = new Set(stageList.stages.filter((s) => s.done).map((s) => s.name));
			openTasks = stageCounts.filter((c) => !doneStages.has(c.stage));
			tasksError = null;
		} catch (e) {
			// An empty card would state there is no open work, which is not what a failed
			// call knows. Say the board could not be read instead.
			openTasks = [];
			tasksError = errorMessage(e);
		} finally {
			tasksLoading = false;
		}
	}

	onMount(() => {
		void load();
		void loadTasks();
	});
</script>

<h1>Dashboard</h1>

<div class="grid">
	<Card title="Active memories">
		<p class="metric" data-testid="count-memories">{report?.memories_active ?? '—'}</p>
	</Card>

	<Card title="Projects">
		<p class="metric" data-testid="count-projects">{counts.projects ?? '—'}</p>
	</Card>

	<Card title="Agents">
		<p class="metric" data-testid="count-agents">{counts.agents ?? '—'}</p>
	</Card>

	<Card title="Open tasks" data-testid="dashboard-tasks">
		{#if tasksError}
			<p class="bad" role="alert" data-testid="dashboard-tasks-error">
				The board could not be read. {tasksError}
			</p>
		{:else if tasksLoading}
			<p class="muted">Loading…</p>
		{:else if openTasks.length === 0}
			<p class="muted">No open tasks.</p>
		{:else}
			<dl>
				{#each openTasks as row (row.stage)}
					<dt>{row.stage}</dt>
					<dd>{row.count}</dd>
				{/each}
			</dl>
		{/if}
	</Card>

	<Card title="Daemon">
		<dl>
			<dt>Version</dt>
			<dd>{report?.version ?? '—'}</dd>
			<dt>Port</dt>
			<dd>{report?.port ?? '—'}</dd>
			<dt>Embedding</dt>
			<dd>
				{#if embedding}
					<Badge {tone}>{embedding}</Badge>
				{:else}
					<span class="muted">unknown</span>
				{/if}
			</dd>
			<dt>Database</dt>
			<dd><code>{report?.db_path || '—'}</code></dd>
		</dl>
	</Card>
</div>

<div class="recent">
	<Card title="Recent memories" data-testid="dashboard-recent">
		{#snippet actions()}
			<Button
				size="sm"
				disabled={loading}
				onclick={() => {
					void load();
					void loadTasks();
				}}
			>
				Refresh
			</Button>
		{/snippet}

		{#if error}
			<ErrorState message={error} logPath={loadErrorLogPath ?? undefined}>
				<Button variant="primary" onclick={load}>Retry</Button>
			</ErrorState>
		{:else if loading && recent.length === 0}
			<p class="muted">Loading…</p>
		{:else}
			<Table {columns} rows={recent} rowKey={(m) => m.id}>
				{#snippet cell(memory: Memory, key: string)}
					{#if key === 'kind'}
						<Badge>{memory.kind}</Badge>
					{:else if key === 'text'}
						<span class="text">{memory.text}</span>
					{:else}
						<span class="muted">{relativeAge(memory.created_at)}</span>
					{/if}
				{/snippet}
				{#snippet empty()}
					<EmptyState
						title="No memories yet"
						hint="Agents write memories through the MCP tools, or add one with `atlas remember`."
					/>
				{/snippet}
			</Table>
		{/if}
	</Card>
</div>

<style>
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
		gap: var(--space-4);
	}

	.recent {
		margin-top: var(--space-4);
	}

	.metric {
		margin: 0;
		font-size: 28px;
		font-weight: 600;
	}

	.muted {
		color: var(--muted);
	}

	.bad {
		margin: 0;
		color: var(--danger);
		font-size: 13px;
		overflow-wrap: anywhere;
	}

	.text {
		display: block;
		max-width: 70ch;
		overflow-wrap: anywhere;
	}

	dl {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: var(--space-1) var(--space-3);
		margin: 0;
		align-items: baseline;
	}

	dt {
		color: var(--muted);
	}

	dd {
		margin: 0;
		overflow-wrap: anywhere;
	}
</style>
