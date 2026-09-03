<script lang="ts">
	// Connected projects, plus the global scope and the connect action. Selecting a project
	// opens its hub; the daemon group at the foot names the port the app is talking to.
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { daemon } from '$lib/daemon.svelte';
	import { connectProject, loadProjects, pickProjectRoot, projects } from '$lib/stores/projects.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	const openId = $derived(page.params.id ?? '');

	onMount(() => {
		// The projects page loads the same list; only fetch when nothing has yet.
		if (projects.items.length === 0 && !projects.loading) void loadProjects();
	});

	/**
	 * Inside Tauri the native folder picker is the whole flow, exactly as the Projects page
	 * runs it. In a browser there is no picker, so the page's typed-path form is where the
	 * user has to finish.
	 */
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
		selected={!openId}
		onclick={() => goto('/projects')}
	/>
	{#each projects.items as project (project.id)}
		<TreeRow
			icon="folder"
			label={project.name}
			mono
			selected={openId === project.id}
			onclick={() => goto(`/projects/${project.id}`)}
		/>
	{/each}
	<TreeRow icon="plus" label="Connect a folder…" onclick={connect} />
</TreeGroup>

<span class="spacer"></span>

<TreeGroup label="Daemon" count=":{daemon.port}" initialOpen={false} />

<style>
	.spacer {
		flex: 1;
	}
</style>
