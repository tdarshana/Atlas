<script lang="ts">
	// The saved agents, and the tags they carry.
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { agents, loadAgents } from '$lib/stores/agents.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	onMount(() => {
		if (!agents.loaded) void loadAgents();
	});

	const tags = $derived.by(() => {
		const counts = new Map<string, number>();
		for (const agent of agents.list) {
			for (const tag of agent.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
		}
		return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
	});
</script>

<TreeGroup label="Agents" count={agents.list.length}>
	{#each agents.list as agent (agent.id)}
		<TreeRow
			icon="bot"
			label={agent.name}
			mono
			meta={`v${agent.version}`}
			onclick={() => goto(`/agents/edit/${encodeURIComponent(agent.name)}`)}
		/>
	{/each}
</TreeGroup>

<TreeGroup label="Tags" count={tags.length}>
	{#each tags as [tag, count] (tag)}
		<TreeRow icon="tag" label={tag} mono meta={count} />
	{/each}
</TreeGroup>
