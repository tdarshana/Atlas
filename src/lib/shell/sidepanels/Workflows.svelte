<script lang="ts">
	// The workflow side panel (frame 08): the workflow list, a library of preset actions
	// and the practices list that both add to the open workflow's graph, and the open
	// workflow's last few runs.
	import { onMount } from 'svelte';
	import { relativeAge } from '$lib/format';
	import { practices } from '$lib/stores/docs.svelte';
	import {
		ACTION_PRESETS,
		addAction,
		createWorkflow,
		isActionData,
		loadRuns,
		loadWorkflows,
		updateNodeData,
		workflow
	} from '$lib/stores/workflows.svelte';
	import type { RunStatus } from '$lib/types';
	import { push } from '$lib/ui/toasts.svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	onMount(() => {
		if (!workflow.loaded) void loadWorkflows();
		if (!practices.state.loaded) void practices.load();
	});

	$effect(() => {
		if (workflow.current) void loadRuns(5);
	});

	const RUN_TONE: Record<RunStatus, string> = {
		queued: 'var(--text-tertiary)',
		running: 'var(--accent)',
		success: 'var(--success-text)',
		failed: 'var(--danger-text)',
		cancelled: 'var(--text-tertiary)'
	};

	async function newWorkflow(): Promise<void> {
		try {
			const created = await createWorkflow();
			await goto(`/workflows/${created.id}`);
		} catch (e) {
			push('error', e instanceof Error ? e.message : String(e));
		}
	}

	function addPreset(preset: (typeof ACTION_PRESETS)[number]): void {
		if (!workflow.current) {
			push('error', 'Open a workflow first');
			return;
		}
		addAction(preset);
	}

	function attachPractice(name: string): void {
		const node = workflow.graph.nodes.find((n) => n.id === workflow.selectedNodeId);
		if (!node || !isActionData(node.data)) {
			push('error', 'Select an action first');
			return;
		}
		if (node.data.practices.includes(name)) return;
		updateNodeData(node.id, { practices: [...node.data.practices, name] });
	}
</script>

<TreeGroup label="Workflows" count={workflow.list.length}>
	{#each workflow.list as w (w.id)}
		<TreeRow
			icon="circle"
			iconColor={RUN_TONE[w.last_status ?? 'cancelled']}
			label={w.name}
			mono
			selected={page.params.id === w.id}
			href={`/workflows/${w.id}`}
		/>
	{/each}
	<TreeRow icon="plus" label="New workflow…" onclick={newWorkflow} />
</TreeGroup>

<TreeGroup label="Actions library">
	{#each ACTION_PRESETS as preset (preset.id)}
		<TreeRow icon="play" label={preset.label} onclick={() => addPreset(preset)} />
	{/each}
</TreeGroup>

<TreeGroup label="Practices" count={practices.state.list.length}>
	{#each practices.state.list as doc (doc.id)}
		<TreeRow icon="book-open" label={doc.name} mono onclick={() => attachPractice(doc.name)} />
	{/each}
</TreeGroup>

{#if workflow.current}
	<TreeGroup label="Runs" count={workflow.runs.length}>
		{#each workflow.runs as r (r.id)}
			<TreeRow
				icon="circle"
				iconColor={RUN_TONE[r.status]}
				label={`Run ${r.number}`}
				mono
				meta={relativeAge(r.started_at)}
				href={`/workflows/${workflow.current.id}/history`}
			/>
		{:else}
			<TreeRow icon="circle" label="No runs yet" iconColor="var(--text-tertiary)" />
		{/each}
	</TreeGroup>
{/if}
