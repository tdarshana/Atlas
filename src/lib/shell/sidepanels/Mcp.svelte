<script lang="ts">
	// Two groups over the MCP servers view: AGENTS, one row per agent that contributed a
	// server, which filters the table when clicked; and ATLAS, the four jump rows that
	// open the Atlas row's detail at the section they name.
	import { onMount } from 'svelte';
	import { agentCounts } from '$lib/mcp-servers';
	import { mcpSidepanelCounts } from '$lib/mcp';
	import { loadMcp, mcp } from '$lib/stores/mcp.svelte';
	import { loadServers, openServer, servers } from '$lib/stores/mcp-servers.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	onMount(() => {
		// The `/mcp` route loads both; only fetch what nothing has yet.
		if (servers.items.length === 0 && !servers.loading) void loadServers(servers.projectId);
		if (!mcp.report && !mcp.loading) void loadMcp();
	});

	const agents = $derived(agentCounts(servers.items));
	const counts = $derived(mcpSidepanelCounts(mcp.report));

	/** Filters the table to one agent, or clears the filter when its row is clicked again. */
	function filter(source: string): void {
		servers.sourceFilter = servers.sourceFilter === source ? null : source;
	}

	/** The ATLAS rows open the Atlas server's detail rather than scrolling the page; the
	 * hash the link carries tells that detail which section to scroll to. */
	function openAtlas(): void {
		const atlas = servers.items.find((s) => s.is_atlas);
		if (atlas) openServer(atlas.id);
	}
</script>

<TreeGroup label="Agents" count={servers.items.length}>
	{#each agents as agent (agent.source)}
		<TreeRow
			icon="plug"
			label={agent.label}
			meta={agent.count}
			selected={servers.sourceFilter === agent.source}
			onclick={() => filter(agent.source)}
		/>
	{/each}
</TreeGroup>

<TreeGroup label="Atlas">
	<TreeRow
		icon="terminal"
		label="Tools"
		meta="{counts.tools} · {counts.toolsDisabled} off"
		href="/mcp#tools"
		onclick={openAtlas}
	/>
	{#if counts.resources > 0}
		<TreeRow
			icon="file"
			label="Resources"
			meta={counts.resources}
			href="/mcp#resources"
			onclick={openAtlas}
		/>
	{/if}
	{#if counts.prompts > 0}
		<TreeRow
			icon="braces"
			label="Prompts"
			meta={counts.prompts}
			href="/mcp#prompts"
			onclick={openAtlas}
		/>
	{/if}
	<TreeRow
		icon="plug"
		label="Clients"
		meta={counts.clients}
		href="/mcp#clients"
		onclick={openAtlas}
	/>
</TreeGroup>
