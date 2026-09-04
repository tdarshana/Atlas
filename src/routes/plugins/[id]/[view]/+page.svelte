<script lang="ts">
	// One contributed section, filling the content panel. The plugin owns everything below
	// the header; the header is the app's, so a section can never dress itself up as a
	// different plugin than the one it is.
	import { onMount } from 'svelte';
	import PluginFrame from '$lib/plugins/PluginFrame.svelte';
	import { loadPlugins, plugins } from '$lib/plugins/host.svelte';
	import { setStatusItems } from '$lib/shell';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import type { PageData } from './$types';

	let { data }: { data: PageData } = $props();

	const plugin = $derived(plugins.items.find((p) => p.id === data.id) ?? null);
	const section = $derived(
		plugin?.manifest?.contributes.sections.find((s) => s.view === data.view) ?? null
	);
	const problem = $derived.by(() => {
		// Nothing is wrong yet while the list is still on its way.
		if (!plugins.loaded || plugins.loading) return null;
		if (!plugins.available) return 'Plugins need the desktop app.';
		if (plugins.error) return plugins.error;
		if (!plugin) return `No plugin '${data.id}' is installed.`;
		if (!plugin.compatible) return plugin.reason ?? `'${data.id}' is not compatible.`;
		if (!plugin.enabled) return `'${plugin.manifest?.name ?? data.id}' is disabled.`;
		if (!section) return `'${plugin.manifest?.name ?? data.id}' contributes no '${data.view}' view.`;
		return null;
	});

	onMount(() => {
		if (plugins.items.length === 0 && !plugins.loading) void loadPlugins();
	});

	$effect(() => {
		setStatusItems({ right: [{ text: plugin?.id ?? data.id }] });
	});
</script>

<div class="title-row">
	<span class="title">{plugin?.manifest?.name ?? data.id}</span>
	{#if section}<span class="section">{section.title}</span>{/if}
</div>

<div class="pane" data-testid="plugin-section">
	{#if problem}
		<ErrorState message={problem}>
			<a href="/plugins" data-testid="plugin-section-back">Open plugins</a>
		</ErrorState>
	{:else if plugin && section}
		<PluginFrame {plugin} view={section.view} fill />
	{/if}
</div>

<style>
	.title-row {
		display: flex;
		align-items: baseline;
		gap: 8px;
		height: 28px;
		flex: 0 0 28px;
	}

	.title {
		font-size: 15px;
		font-weight: 600;
	}

	.section {
		color: var(--text-tertiary);
	}

	.pane {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
	}
</style>
