<script lang="ts">
	// Four jump rows into the `/mcp` route's own sections, each carrying the live count
	// the route itself shows; Tools also carries how many of them are off right now.
	import { onMount } from 'svelte';
	import { mcpSidepanelCounts } from '$lib/mcp';
	import { loadMcp, mcp } from '$lib/stores/mcp.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	onMount(() => {
		// The `/mcp` route loads the same report; only fetch when nothing has yet.
		if (!mcp.report && !mcp.loading) void loadMcp();
	});

	const jump = (hash: string) => () =>
		document.getElementById(hash)?.scrollIntoView({ behavior: 'instant', block: 'start' });

	const counts = $derived(mcpSidepanelCounts(mcp.report));
</script>

<TreeGroup label="MCP">
	<TreeRow
		icon="terminal"
		label="Tools"
		meta="{counts.tools} · {counts.toolsDisabled} off"
		href="/mcp#tools"
		onclick={jump('tools')}
	/>
	<TreeRow
		icon="file"
		label="Resources"
		meta={counts.resources}
		href="/mcp#resources"
		onclick={jump('resources')}
	/>
	<TreeRow
		icon="braces"
		label="Prompts"
		meta={counts.prompts}
		href="/mcp#prompts"
		onclick={jump('prompts')}
	/>
	<TreeRow
		icon="plug"
		label="Clients"
		meta={counts.clients}
		href="/mcp#clients"
		onclick={jump('clients')}
	/>
</TreeGroup>
