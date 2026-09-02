<script lang="ts">
	// Dashboard: the daemon's status report, the three counts and the newest
	// memories. Counts come from the list routes rather than the status report so
	// projects and agents are counted the same way.
	import { onMount } from 'svelte';
	import { ApiError } from '$lib/api';
	import { api } from '$lib/daemon.svelte';
	import { logPath, relativeAge } from '$lib/stores/memories.svelte';
	import { status } from '$lib/stores/status.svelte';
	import type { Memory } from '$lib/types';
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
		memories: null as number | null,
		projects: null as number | null,
		agents: null as number | null
	});
	let recent = $state<Memory[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let errorLogPath = $state<string | null>(null);

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
			counts = { memories: active.length, projects: projectList.length, agents: agentList.length };
			recent = [...active]
				.sort((a, b) => b.created_at.localeCompare(a.created_at))
				.slice(0, RECENT);
			error = null;
			errorLogPath = null;
		} catch (e) {
			recent = [];
			error = e instanceof Error ? e.message : String(e);
			errorLogPath = e instanceof ApiError && e.status === 0 ? logPath() : null;
		} finally {
			loading = false;
		}
	}

	onMount(load);
</script>

<h1>Dashboard</h1>

<div class="grid">
	<Card title="Active memories">
		<p class="metric" data-testid="count-memories">
			{counts.memories ?? report?.memories_active ?? '—'}
		</p>
	</Card>

	<Card title="Projects">
		<p class="metric" data-testid="count-projects">{counts.projects ?? '—'}</p>
	</Card>

	<Card title="Agents">
		<p class="metric" data-testid="count-agents">{counts.agents ?? '—'}</p>
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
			<Button size="sm" onclick={load} disabled={loading}>Refresh</Button>
		{/snippet}

		{#if error}
			<ErrorState message={error} logPath={errorLogPath ?? undefined}>
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
