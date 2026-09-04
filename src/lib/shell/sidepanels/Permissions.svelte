<script lang="ts">
	// Three jump rows into the `/permissions` route's sections. The system group lists a
	// row per permission with a status dot, so the panel says what is granted without the
	// page being scrolled to it.
	import { onMount } from 'svelte';
	import { permissionsStatus } from '$lib/permissions/commands';
	import { toRows, type PermissionStatus } from '$lib/permissions/system';
	import { plugins, loadPlugins } from '$lib/plugins/host.svelte';
	import { loadProjects, projects } from '$lib/stores/projects.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	let statuses = $state<PermissionStatus[]>([]);

	const rows = $derived(toRows(statuses));
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
		// been fetched yet, and read the statuses here so the dots are right on first paint.
		if (projects.items.length === 0) void loadProjects();
		if (!plugins.loaded) void loadPlugins();
		void permissionsStatus(projects.items.map((p) => p.root_path).filter(Boolean))
			.then((s) => (statuses = s))
			.catch(() => {
				// A status read that fails leaves every dot on `unknown`, which is the truth.
			});
	});

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
