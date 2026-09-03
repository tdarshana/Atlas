<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { daemon } from '$lib/daemon.svelte';
	import { plural } from '$lib/format';
	import SyncPanel from '$lib/components/SyncPanel.svelte';
	import { setStatusItems } from '$lib/shell';
	import { agents, loadAgents } from '$lib/stores/agents.svelte';
	import Badge from '$lib/ui/Badge.svelte';
	import Button from '$lib/ui/Button.svelte';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import Table, { type TableColumn } from '$lib/ui/Table.svelte';
	import type { Agent } from '$lib/types';

	const COLUMNS: TableColumn[] = [
		{ key: 'name', label: 'Name', width: '20%' },
		{ key: 'description', label: 'Description' },
		{ key: 'version', label: 'Version', width: '90px', align: 'right' },
		{ key: 'tags', label: 'Tags', width: '25%' }
	];

	let syncEl = $state<HTMLDivElement>();

	onMount(() => {
		void loadAgents();
	});

	// `?sync=1` (from the command palette's `⌥↵` on the Agents jump-to, or a task linking
	// here) scrolls straight to the sync panel below the table. It reads the URL rather
	// than running once, so a second palette action while this page is open works too.
	$effect(() => {
		const wanted = page.url.searchParams.get('sync') === '1';
		if (!wanted) return;
		// jsdom and older webviews have no smooth scrolling; the panel is still there.
		if (syncEl && typeof syncEl.scrollIntoView === 'function') {
			syncEl.scrollIntoView({ behavior: 'smooth', block: 'start' });
		}
	});

	$effect(() => {
		setStatusItems({ right: [{ text: plural(agents.list.length, 'agent') }] });
	});
</script>

<header class="head">
	<h1>Agents</h1>
	<Button variant="primary" data-testid="agent-new" onclick={() => goto('/agents/new')}>
		New agent
	</Button>
</header>

{#if agents.error}
	<ErrorState message={agents.error} logPath={daemon.logPath || undefined}>
		<Button onclick={() => loadAgents()}>Retry</Button>
	</ErrorState>
{:else}
	<Table
		columns={COLUMNS}
		rows={agents.list}
		rowKey={(a) => a.id}
		data-testid="agents-table"
		onrowclick={(a) => goto(`/agents/edit/${encodeURIComponent(a.name)}`)}
	>
		{#snippet cell(agent: Agent, key: string)}
			{#if key === 'name'}
				<strong>{agent.name}</strong>
			{:else if key === 'description'}
				{agent.description}
			{:else if key === 'version'}
				{agent.version}
			{:else}
				<span class="tags">
					{#each agent.tags as tag (tag)}
						<Badge>{tag}</Badge>
					{/each}
				</span>
			{/if}
		{/snippet}
		{#snippet empty()}
			<EmptyState
				title={agents.loading ? 'Loading agents…' : 'No agents yet'}
				hint="An agent is a reusable prompt Atlas can export into Claude Code and Codex."
			>
				{#if !agents.loading}
					<Button variant="primary" onclick={() => goto('/agents/new')}>New agent</Button>
				{/if}
			</EmptyState>
		{/snippet}
	</Table>
{/if}

<div class="sync" bind:this={syncEl}>
	<SyncPanel />
</div>

<style>
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
		height: 28px;
		flex: 0 0 28px;
		margin-bottom: var(--space-4);
	}

	h1 {
		margin: 0;
		font-size: 15px;
		font-weight: 600;
	}

	.tags {
		display: inline-flex;
		flex-wrap: wrap;
		gap: var(--space-1);
	}

	.sync {
		margin-top: var(--space-5);
	}
</style>
