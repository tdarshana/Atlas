<script lang="ts">
	// One row per installed plugin, with a dot that says whether it is on, and one row per
	// section the enabled plugins contribute, each linking to that section's own view.
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { sectionHref } from '$lib/plugins/contributions';
	import { contributions, loadPlugins, plugins } from '$lib/plugins/host.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	onMount(() => {
		// The `/plugins` route loads the same store; only fetch when nothing has yet.
		if (plugins.items.length === 0 && !plugins.loading) void loadPlugins();
	});

	const sections = $derived(contributions().sections);
</script>

<TreeGroup label="Plugins" count={plugins.items.length}>
	{#each plugins.items as plugin (plugin.id)}
		<TreeRow
			icon={plugin.enabled ? 'circle-check' : 'circle'}
			iconColor={plugin.enabled ? 'var(--success-text)' : 'var(--text-tertiary)'}
			label={plugin.manifest?.name ?? plugin.id}
			meta={plugin.manifest?.version}
			href="/plugins"
		/>
	{/each}
</TreeGroup>

{#if sections.length > 0}
	<TreeGroup label="Sections" count={sections.length}>
		{#each sections as section (`${section.pluginId}/${section.id}`)}
			<TreeRow
				icon="wand-sparkles"
				label={section.title}
				href={sectionHref(section)}
				selected={page.url.pathname === sectionHref(section)}
			/>
		{/each}
	</TreeGroup>
{/if}
