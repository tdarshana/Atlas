<script lang="ts">
	// Three jump rows into the `/permissions` route's sections. The system group lists a
	// row per permission with a status dot, so the panel says what is granted without the
	// page being scrolled to it.
	import { onMount } from 'svelte';
	import { checkPermissions, systemPermissions } from '$lib/permissions/store.svelte';
	import { toRows } from '$lib/permissions/system';
	import { plugins, loadPlugins } from '$lib/plugins/host.svelte';
	import { loadProjects, projects } from '$lib/stores/projects.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	const rows = $derived(toRows(systemPermissions.statuses));
	const grantCount = $derived(
		plugins.items.filter((p) => (p.manifest?.permissions.length ?? 0) > 0).length
	);

	const DOTS = {
		granted: 'circle-check',
		denied: 'circle-x',
		not_determined: 'circle-dot',
		unknown: 'circle',
		not_applicable: 'circle'
	} as const;

	onMount(() => {
		// The `/permissions` route reads the same three sources; only fetch what has not
		// been fetched yet. The statuses come from the shared store, which coalesces a read
		// already in flight, so mounting beside the page costs one call rather than two, and
		// the page's poll keeps these dots current for as long as the panel is open.
		if (!plugins.loaded) void loadPlugins();
		void (projects.items.length === 0 ? loadProjects() : Promise.resolve()).then(() => {
			if (!systemPermissions.loaded) void checkPermissions(roots());
		});
	});

	/** The connected project roots, which the files row probes. */
	function roots(): string[] {
		return projects.items.map((p) => p.root_path).filter(Boolean);
	}

	const jump = (hash: string) => () =>
		document.getElementById(hash)?.scrollIntoView({ behavior: 'instant', block: 'start' });
</script>

<TreeGroup label="System">
	{#each rows as row (row.id)}
		<TreeRow
			icon={DOTS[row.state]}
			label={row.name}
			meta={row.badge}
			href="/permissions#system"
			onclick={jump('system')}
		/>
	{/each}
</TreeGroup>

<TreeGroup label="Plugins">
	<TreeRow
		icon="layers"
		label="Plugin grants"
		meta={grantCount}
		href="/permissions#plugins"
		onclick={jump('plugins')}
	/>
</TreeGroup>

<TreeGroup label="Agent defaults">
	<TreeRow
		icon="bot"
		label="Memory writers and task movers"
		href="/permissions#agents"
		onclick={jump('agents')}
	/>
</TreeGroup>
