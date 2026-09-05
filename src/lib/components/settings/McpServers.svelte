<script lang="ts">
	// The MCP server card: a two-line summary off the same store the `/mcp` route and its
	// side panel read, so it never needs its own fetch or its own copy of the servers table.
	import { Badge } from '$lib/ds';
	import { daemon } from '$lib/daemon.svelte';
	import { loadServers, servers } from '$lib/stores/mcp-servers.svelte';
	import SettingsCard from './SettingsCard.svelte';

	const online = $derived(!daemon.error);

	const mcpSummaryText = $derived(
		`${servers.items.length} ${servers.items.length === 1 ? 'server' : 'servers'}, ${servers.items.filter((s) => s.enabled).length} enabled`
	);

	/** Re-lists the servers; the route calls it on load and on Reload. */
	export async function refresh(): Promise<void> {
		await loadServers(null);
	}
</script>

<SettingsCard id="mcp" title="MCP server">
	{#snippet head()}
		<Badge tone={online ? 'success' : 'neutral'} icon={online ? 'circle-check' : 'circle'}>
			{online ? 'running' : 'offline'}
		</Badge>
	{/snippet}

	<span class="hint" data-testid="mcp-summary">{mcpSummaryText}</span>
	<span class="hint">
		Every MCP server your agents are wired to, Atlas among them. Its connect snippets, the
		tools table with its enable checkboxes, connected clients and a
		<span class="mono">Restart</span> command are behind the Atlas row.
	</span>
	<a href="/mcp" data-testid="mcp-open-link">Open MCP</a>
</SettingsCard>

<style>
	.mono {
		font-family: var(--font-mono);
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}
</style>
