<script lang="ts">
	// The Agents tab (frame 02.4): agents tagged with this project's name (see
	// `agents.ts` for why a tag stands in for a `project_id` the daemon does not have),
	// plus the Sync card scoped to this root.
	import { Badge, Button, Table, type TableColumn } from '$lib/ds';
	import { daemon } from '$lib/daemon.svelte';
	import { plural } from '$lib/format';
	import { setStatusItems } from '$lib/shell';
	import { agents, loadAgents, saveAgent } from '$lib/stores/agents.svelte';
	import { project, setHeaderActions } from '$lib/stores/project.svelte';
	import type { Agent } from '$lib/types';
	import AgentDialog from '$lib/components/project/AgentDialog.svelte';
	import ProjectSync from '$lib/components/project/ProjectSync.svelte';
	import { projectAgents } from '$lib/components/project/agents';
	import ErrorState from '$lib/ui/ErrorState.svelte';

	let open = $state(false);

	const name = $derived(project.current?.name ?? 'Project');
	const root = $derived(project.current?.root_path ?? '');
	const mine = $derived(projectAgents(agents.list, name));

	const columns: TableColumn<Agent>[] = [
		{ key: 'name', label: 'Name', width: '180px', sortable: true },
		{ key: 'description', label: 'Description' },
		{ key: 'version', label: 'Version', width: '90px', align: 'right' },
		{ key: 'tags', label: 'Tags', width: '120px' }
	];

	function openNew(): void {
		open = true;
	}

	$effect(() => {
		void loadAgents();
	});

	$effect(() => {
		setHeaderActions(headerActions);
		return () => setHeaderActions(null);
	});

	$effect(() => {
		setStatusItems({ right: [{ text: `${name} · ${plural(mine.length, 'agent')}` }] });
	});
</script>

{#snippet headerActions()}
	<Button variant="primary" data-testid="agents-new" onclick={openNew}>New agent</Button>
{/snippet}

{#if agents.error}
	<ErrorState message={agents.error} logPath={daemon.logPath || undefined}>
		<Button variant="primary" onclick={() => loadAgents()}>Retry</Button>
	</ErrorState>
{:else}
	<div class="stack">
		<Table id="project-agents" {columns} rows={mine} rowKey={(a: Agent) => a.id}>
			{#snippet cell(agent: Agent, column: TableColumn<Agent>)}
				{#if column.key === 'name'}
					<strong>{agent.name}</strong>
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
					<span class="title">No project agents yet</span>
					<span class="hint">Project agents are exported into this root only. Global agents still apply.</span>
					<Button variant="primary" onclick={openNew}>New agent</Button>
				</div>
			{/snippet}
		</Table>

		<ProjectSync projectName={name} {root} />
	</div>
{/if}

<AgentDialog {open} projectName={name} onclose={() => (open = false)} onsave={saveAgent} />

<style>
	.stack {
		display: flex;
		flex-direction: column;
		gap: 12px;
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

	.title {
		font-weight: 600;
	}

	.hint {
		color: var(--text-secondary);
	}
</style>
