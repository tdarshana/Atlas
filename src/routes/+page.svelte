<script lang="ts">
	// Dashboard: the daemon's status report, the three counts, the global board's open
	// tasks, and the newest memories. The active-memory count and the daemon panel come
	// from the status report the shell already polls; projects and agents have no such
	// counter, so those come from their list routes; open tasks come from the board's
	// global (no project) counts.
	import { onMount } from 'svelte';
	import { api } from '$lib/daemon.svelte';
	import { Badge, Button, Table, type TableColumn } from '$lib/ds';
	import { errorLogPath, errorMessage } from '$lib/errors';
	import { relativeAge } from '$lib/format';
	import { setStatusItems } from '$lib/shell';
	import { status } from '$lib/stores/status.svelte';
	import type { Memory, MemoryKind, StageCount } from '$lib/types';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';

	const RECENT = 10;
	const OPEN_STAGES = ['Backlog', 'In progress', 'Testing'];

	const report = $derived(status.report);
	const embeddingWord = $derived(report?.embedding.split(':')[0].trim() ?? '');
	const embeddingTone = $derived(
		status.error
			? 'danger'
			: embeddingWord === 'ready'
				? 'success'
				: embeddingWord === 'loading'
					? 'accent'
					: 'neutral'
	);
	const embeddingText = $derived(status.error ? 'offline' : embeddingWord || 'connecting');

	let counts = $state({ projects: null as number | null, agents: null as number | null });
	let recent = $state<Memory[]>([]);
	let openTasks = $state<StageCount[]>([]);
	let tasksLoading = $state(true);
	let tasksError = $state<string | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let loadErrorLogPath = $state<string | null>(null);

	function openTaskCount(stage: string): number {
		return openTasks.find((c) => c.stage === stage)?.count ?? 0;
	}

	/** Insight and moved-style information reads info; a todo is pending write work.
	    Fact, decision and preference carry no strong tone of their own. */
	function kindTone(kind: MemoryKind): 'neutral' | 'info' | 'warning' {
		if (kind === 'insight') return 'info';
		if (kind === 'todo') return 'warning';
		return 'neutral';
	}

	const columns: TableColumn<Memory>[] = [
		{ key: 'kind', label: 'Kind', width: '110px' },
		{ key: 'text', label: 'Text', mono: true },
		{
			key: 'age',
			label: 'Age',
			width: '80px',
			align: 'right',
			mono: true,
			sortable: true,
			sort: (a, b) => a.created_at.localeCompare(b.created_at)
		}
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

	/** The board counts on its own, so a board that cannot answer costs this card only,
	    not the whole page. */
	async function loadTasks() {
		tasksLoading = true;
		try {
			openTasks = await api().taskCounts();
			tasksError = null;
		} catch (e) {
			openTasks = [];
			tasksError = errorMessage(e);
		} finally {
			tasksLoading = false;
		}
	}

	function refresh() {
		void load();
		void loadTasks();
	}

	onMount(refresh);

	$effect(() => {
		const n = report?.memories_active ?? 0;
		setStatusItems({
			right: [
				status.error
					? { text: status.error, tone: 'danger' }
					: { text: `embedding ${embeddingWord || 'unknown'}`, tone: embeddingWord === 'ready' ? 'success' : undefined },
				{ text: `${n} ${n === 1 ? 'memory' : 'memories'}` }
			]
		});
	});
</script>

<div class="title-row"><span class="title">Dashboard</span></div>

<div class="stats">
	<div class="card stat-card">
		<span class="group-heading">Active memories</span>
		<span class="metric">{report?.memories_active ?? '-'}</span>
	</div>

	<div class="card stat-card">
		<span class="group-heading">Projects</span>
		<span class="metric">{counts.projects ?? '-'}</span>
	</div>

	<div class="card stat-card">
		<span class="group-heading">Agents</span>
		<span class="metric">{counts.agents ?? '-'}</span>
	</div>

	<div class="card stat-card rows">
		<span class="group-heading">Open tasks</span>
		{#if tasksError}
			<p class="error-line" role="alert">The board could not be read. {tasksError}</p>
		{:else if !tasksLoading}
			{#each OPEN_STAGES as stage (stage)}
				<div class="stat-row">
					<span>{stage}</span>
					<span class="mono value">{openTaskCount(stage)}</span>
				</div>
			{/each}
		{/if}
	</div>

	<div class="card stat-card rows">
		<span class="group-heading">Daemon</span>
		<div class="stat-row">
			<span>Version</span><span class="mono value">{report?.version ?? '-'}</span>
		</div>
		<div class="stat-row">
			<span>Port</span><span class="mono value">{report?.port ?? '-'}</span>
		</div>
		<div class="stat-row">
			<span>Embedding</span>
			<Badge tone={embeddingTone}>{embeddingText}</Badge>
		</div>
		<div class="stat-row">
			<span>Database</span><span class="db-path mono">{report?.db_path || '-'}</span>
		</div>
	</div>
</div>

<div class="card recent">
	<div class="recent-header">
		<span class="recent-title">Recent memories</span>
		<span class="spacer"></span>
		<Button variant="ghost" size="sm" disabled={loading} onclick={refresh}>Refresh</Button>
	</div>

	{#if error}
		<div class="recent-body">
			<ErrorState message={error} logPath={loadErrorLogPath ?? undefined}>
				<Button variant="primary" onclick={load}>Retry</Button>
			</ErrorState>
		</div>
	{:else if loading && recent.length === 0}
		<p class="loading">Loading…</p>
	{:else}
		<Table
			id="dashboard-recent"
			{columns}
			rows={recent}
			rowKey={(m) => m.id}
			defaultSort={{ key: 'age', dir: 'desc' }}
		>
			{#snippet cell(memory: Memory, column: TableColumn<Memory>)}
				{#if column.key === 'kind'}
					<Badge tone={kindTone(memory.kind)}>{memory.kind}</Badge>
				{:else if column.key === 'text'}
					{memory.text}
				{:else}
					{relativeAge(memory.created_at)}
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
</div>

<style>
	.title-row {
		display: flex;
		align-items: center;
		height: 28px;
		flex: 0 0 28px;
	}

	.title {
		font-size: 15px;
		font-weight: 600;
	}

	.stats {
		display: grid;
		grid-template-columns: 1fr 1fr 1fr 1.3fr 1.8fr;
		gap: 12px;
		flex: 0 0 auto;
	}

	.stat-card {
		padding: 10px 12px;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}

	.stat-card.rows {
		gap: 6px;
	}

	.metric {
		font-family: var(--font-mono);
		font-size: 15px;
		font-weight: 700;
	}

	.stat-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		color: var(--text-secondary);
	}

	.value {
		color: var(--text-primary);
	}

	.db-path {
		color: var(--text-primary);
		font-size: 11px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		min-width: 0;
	}

	.error-line {
		margin: 0;
		color: var(--danger-text);
		font-size: 12px;
		overflow-wrap: anywhere;
	}

	.recent {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		overflow: hidden;
	}

	.recent-header {
		height: 32px;
		flex: 0 0 32px;
		display: flex;
		align-items: center;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.recent-title {
		font-weight: 600;
	}

	.spacer {
		flex: 1;
	}

	.recent-body {
		flex: 1;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: var(--space-4);
	}

	.loading {
		margin: 0;
		padding: var(--space-4);
		color: var(--text-secondary);
	}
</style>
