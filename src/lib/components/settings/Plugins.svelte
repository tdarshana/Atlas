<script lang="ts">
	// The Plugins card: a count off the same store the `/plugins` view and its side panel
	// read, so the number here is whatever those last saw rather than a fetch of its own.
	import { onMount } from 'svelte';
	import { loadPlugins, plugins } from '$lib/plugins/host.svelte';
	import ToolChannelWarning from '$lib/plugins/ToolChannelWarning.svelte';
	import SettingsCard from './SettingsCard.svelte';

	const pluginsSummaryText = $derived(
		plugins.available
			? `${plugins.items.length} installed, ${plugins.items.filter((p) => p.enabled).length} enabled`
			: 'Plugins need the desktop app'
	);

	onMount(() => {
		if (!plugins.loaded) void loadPlugins();
	});
</script>

<SettingsCard id="plugins" title="Plugins">
	<span class="hint" data-testid="plugins-settings-summary">{pluginsSummaryText}</span>
	<ToolChannelWarning />
	<span class="hint">
		A plugin runs in a sandboxed frame and reaches Atlas only through the permissions its
		manifest asks for. Install, enable and remove them on their own view.
	</span>
	<a href="/plugins" data-testid="plugins-open-link">Open plugins</a>
</SettingsCard>

<style>
	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}
</style>
