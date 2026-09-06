<script lang="ts">
	// Agents (frame 06): the saved agents, and the Sync card that exports them into the
	// files Claude Code and Codex read. The editor is a dialog over this list, reached by
	// clicking a row or `New agent`; `?sync=1` from the palette runs a Check on arrival.
	import { onMount } from 'svelte';
	import { replaceState } from '$app/navigation';
	import { page } from '$app/state';
	import { Badge, Button, Table, type TableColumn } from '$lib/ds';
	import { daemon } from '$lib/daemon.svelte';
	import { plural } from '$lib/format';
	import { setStatusItems } from '$lib/shell';
	import AgentEditor from '$lib/components/AgentEditor.svelte';
	import SyncPanel from '$lib/components/SyncPanel.svelte';
	import { agents, loadAgents } from '$lib/stores/agents.svelte';
	import type { Agent } from '$lib/types';
	import ErrorState from '$lib/ui/ErrorState.svelte';

	const columns: TableColumn<Agent>[] = [
		{ key: 'name', label: 'Name', width: '180px', sortable: true },
		{ key: 'description', label: 'Description' },
		{ key: 'version', label: 'Version', width: '90px', mono: true, sortable: true },
		{ key: 'tags', label: 'Tags', width: '160px' }
	];

	let open = $state(false);
	let editing = $state<string | null>(null);

	/** `?sync=1` (the palette's `⌥↵` on the Agents jump-to) asks for a dry run on arrival. */
	const checkOnMount = page.url.searchParams.get('sync') === '1';

	function openNew(): void {
		editing = null;
		open = true;
	}

	function openEdit(agent: Agent): void {
		editing = agent.name;
		open = true;
	}

	onMount(() => {
		void loadAgents();
	});

	// The side panel links an agent as `/agents?edit=<name>`; reading the URL rather than
	// running once means a second row picked while this page is open opens that one.
	$effect(() => {
		const name = page.url.searchParams.get('edit');
		if (!name) return;
		editing = name;
		open = true;
	});

	/**
	 * Closing the editor drops `?edit=` with it. Left in place, the param would reopen the
	 * dialog on the next thing that touched the URL, and the back button would land on a
	 * URL that says an editor is open when it is not.
	 */
	function closeEditor(): void {
		open = false;
		if (!page.url.searchParams.has('edit')) return;
		const url = new URL(page.url);
		url.searchParams.delete('edit');
		replaceState(url, page.state);
	}

	$effect(() => {
		setStatusItems({ right: [{ text: plural(agents.list.length, 'agent') }] });
	});
</script>

<div class="title-row">
	<span class="title">Agents</span>
	<span class="spacer"></span>
	<Button variant="primary" data-testid="agent-new" onclick={openNew}>New agent</Button>
</div>
<span class="hint" data-testid="agents-note">
	Agents are prompt templates exported to Claude Code and Codex. For a role that carries
	skills, workflows, practices, models and access, use <a href="/personas">Personas</a>.
</span>

{#if agents.error}
	<ErrorState message={agents.error} logPath={daemon.logPath || undefined}>
		<Button variant="primary" onclick={() => loadAgents()}>Retry</Button>
	</ErrorState>
{:else}
	<div class="list" data-testid="agents-table">
		<Table
			id="agents"
			{columns}
			rows={agents.list}
			rowKey={(a: Agent) => a.id}
			onRowClick={openEdit}
			defaultSort={{ key: 'name', dir: 'asc' }}
		>
			{#snippet cell(agent: Agent, column: TableColumn<Agent>)}
				{#if column.key === 'name'}
					<span class="link">{agent.name}</span>
				{:else if column.key === 'description'}
					{agent.description}
				{:else if column.key === 'version'}
					{agent.version}
				{:else}
					<span class="tags">
						{#each agent.tags as tag (tag)}<Badge mono>{tag}</Badge>{/each}
					</span>
				{/if}
			{/snippet}
			{#snippet empty()}
				<div class="empty">
					{#if agents.loading}
						<span class="empty-title">Loading agents…</span>
					{:else}
						<span class="empty-title">No agents yet</span>
						<span class="empty-hint">
							An agent is a reusable prompt Atlas can export into Claude Code and Codex.
						</span>
						<div class="empty-action">
							<Button variant="primary" onclick={openNew}>New agent</Button>
						</div>
					{/if}
				</div>
			{/snippet}
		</Table>
	</div>
{/if}

<SyncPanel {checkOnMount} />

<AgentEditor {open} {editing} onclose={closeEditor} onchanged={() => loadAgents()} />

<style>
	.title-row {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 28px;
		flex: 0 0 28px;
	}

	.title {
		font-size: 15px;
		font-weight: 600;
	}

	.spacer {
		flex: 1;
	}

	.list {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		overflow: hidden;
	}

	.link {
		font-family: var(--font-mono);
		color: var(--accent);
	}

	.tags {
		display: inline-flex;
		flex-wrap: wrap;
		gap: 4px;
	}

	.empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 6px;
		padding: 28px 0;
	}

	.empty-title {
		font-weight: 600;
	}

	.empty-hint {
		color: var(--text-secondary);
	}

	.empty-action {
		margin-top: 4px;
	}
	.hint {
		color: var(--text-tertiary);
		font-size: 11px;
	}
</style>
