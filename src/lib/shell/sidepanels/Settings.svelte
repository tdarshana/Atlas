<script lang="ts">
	// The settings sections, and where the daemon keeps its files. Each row is a link to
	// the matching card's anchor on the settings page, which scrolls itself to the hash.
	import { status } from '$lib/stores/status.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	/** The daemon reports the database file; its directory is the config home. */
	const home = $derived.by(() => {
		const path = status.report?.db_path ?? '';
		const cut = path.lastIndexOf('/');
		return cut > 0 ? path.slice(0, cut) : '~/.atlas';
	});
</script>

<TreeGroup label="Sections">
	<TreeRow icon="cpu" label="Daemon" href="/settings#daemon" />
	<TreeRow icon="wand-sparkles" label="Extraction" href="/settings#extraction" />
	<TreeRow icon="columns-3" label="Board stages" href="/settings#board-stages" />
	<TreeRow icon="plug" label="MCP server" href="/settings#mcp" />
</TreeGroup>

<span class="spacer"></span>

<TreeGroup label="Config" count={home} initialOpen={false} />

<style>
	.spacer {
		flex: 1;
	}
</style>
