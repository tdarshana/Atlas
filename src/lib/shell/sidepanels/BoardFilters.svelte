<script lang="ts">
	// The Board tab's side panel: the project list the Projects panel shows, then the
	// board's own columns and assignees with their counts. Counts are read from the board's
	// current tasks, so they are what the columns hold before a filter narrows them.
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { daemon } from '$lib/daemon.svelte';
	import { board, scheduleRefresh } from '$lib/stores/board.svelte';
	import { connectProject, loadProjects, pickProjectRoot, projects } from '$lib/stores/projects.svelte';
	import { GLOBAL_ID } from '$lib/stores/project.svelte';
	import { countInStage, groupAssignees, stageIcon } from './boardFilters';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	const openId = $derived(page.params.id ?? '');

	const stages = $derived(board.stages);
	const tasks = $derived(board.tasks);

	const assignees = $derived(groupAssignees(tasks));

	onMount(() => {
		if (projects.items.length === 0 && !projects.loading) void loadProjects();
	});

	/** A second click on the lit column clears the filter rather than re-setting it. */
	function toggleStage(name: string) {
		board.filters.stage = board.filters.stage === name ? null : name;
	}

	function toggleAssignee(name: string) {
		board.filters.assignee = board.filters.assignee === name ? '' : name;
		board.filters.unassigned = false;
		scheduleRefresh(0);
	}

	function toggleUnassigned() {
		board.filters.unassigned = !board.filters.unassigned;
		if (board.filters.unassigned && board.filters.assignee) {
			board.filters.assignee = '';
			scheduleRefresh(0);
		}
	}

	async function connect() {
		const picked = await pickProjectRoot();
		if (!picked) {
			void goto('/projects');
			return;
		}
		try {
			const project = await connectProject(picked);
			void goto(`/projects/${project.id}`);
		} catch {
			void goto('/projects');
		}
	}
</script>

<TreeGroup label="Projects" count={projects.items.length}>
	<TreeRow
		icon="layers"
		label="Global"
		selected={openId === GLOBAL_ID}
		onclick={() => goto(`/projects/${GLOBAL_ID}/board`)}
	/>
	{#each projects.items as project (project.id)}
		<TreeRow
			icon="folder"
			label={project.name}
			mono
			selected={openId === project.id}
			onclick={() => goto(`/projects/${project.id}/board`)}
		/>
	{/each}
	<TreeRow icon="plus" label="Connect a folder…" onclick={connect} />
</TreeGroup>

<TreeGroup label="Columns">
	{#each stages as stage, i (stage.name)}
		<TreeRow
			icon={stageIcon(stage, i)}
			label={stage.name}
			meta={countInStage(tasks, stage.name)}
			selected={board.filters.stage === stage.name}
			iconColor={stageIcon(stage, i) === 'flask-conical' ? 'var(--accent)' : undefined}
			onclick={() => toggleStage(stage.name)}
		/>
	{/each}
</TreeGroup>

<TreeGroup label="Assignees">
	{#each assignees.named as [name, count] (name)}
		<TreeRow
			icon="bot"
			label={name}
			mono
			meta={count}
			selected={board.filters.assignee === name}
			onclick={() => toggleAssignee(name)}
		/>
	{/each}
	<TreeRow
		icon="circle"
		label="Unassigned"
		meta={assignees.unassigned}
		selected={board.filters.unassigned}
		onclick={toggleUnassigned}
	/>
</TreeGroup>

<span class="spacer"></span>

<TreeGroup label="Daemon" count=":{daemon.port}" initialOpen={false} />

<style>
	.spacer {
		flex: 1;
	}
</style>
