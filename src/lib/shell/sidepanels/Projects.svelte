<script lang="ts">
	// Connected projects, plus the global scope and the connect action. Selecting a project
	// opens its hub; the daemon group at the foot names the port the app is talking to.
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, daemon } from '$lib/daemon.svelte';
	import { FRAMEWORK_LABEL, sidePanelFrameworks } from '$lib/components/project/frameworks';
	import { connectProject, loadProjects, pickProjectRoot, projects } from '$lib/stores/projects.svelte';
	import type { FrameworkListing } from '$lib/types';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	/** The route segment the every-project board answers to. */
	const GLOBAL_ID = 'global';

	const openId = $derived(page.params.id ?? '');
	// Read from the frameworks route rather than `ProjectProfile.planning_frameworks`:
	// a project connected before this phase has a stored profile with no such field,
	// and would otherwise show no group here until its next refresh.
	let frameworkListings = $state<FrameworkListing[]>([]);
	const frameworks = $derived(sidePanelFrameworks(frameworkListings));

	$effect(() => {
		const id = openId && openId !== GLOBAL_ID ? openId : null;
		if (!id) {
			frameworkListings = [];
			return;
		}
		let cancelled = false;
		void api()
			.listFrameworks(id)
			.then((listings) => {
				if (!cancelled) frameworkListings = listings;
			})
			.catch(() => {
				if (!cancelled) frameworkListings = [];
			});
		return () => {
			cancelled = true;
		};
	});

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
	<!-- Global is the every-project board, not the project list, so `/projects` itself
	     leaves every row unselected. -->
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
			onclick={() => goto(`/projects/${project.id}`)}
		/>
	{/each}
	<TreeRow icon="plus" label="Connect a folder…" onclick={connect} />
</TreeGroup>

{#if openId && openId !== GLOBAL_ID && frameworks.length > 0}
	<TreeGroup label="Frameworks" count={frameworks.length}>
		{#each frameworks as fw (fw.kind)}
			<TreeRow
				icon="list-checks"
				label={FRAMEWORK_LABEL[fw.kind]}
				meta={fw.docs}
				href={`/projects/${openId}/frameworks`}
			/>
		{/each}
	</TreeGroup>
{/if}

<span class="spacer"></span>

<TreeGroup label="Daemon" count=":{daemon.port}" initialOpen={false} />

<style>
	.spacer {
		flex: 1;
	}
</style>
