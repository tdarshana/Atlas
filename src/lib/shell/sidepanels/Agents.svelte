<script lang="ts">
	// Two groups over the same list the `/agents` table draws: a row per tag that
	// narrows the table, and a row per project with how many agents its roster holds.
	// Counts come from the store, so an agent saved on the route is counted here
	// without a second fetch.
	import { onMount } from 'svelte';
	import { loadPersonas, loadUsage, personas } from '$lib/stores/personas.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	onMount(() => {
		// The route loads the same lists; only fetch what nothing has yet.
		if (personas.items.length === 0 && !personas.loading) void loadPersonas();
		if (personas.rosters.length === 0) void loadUsage();
	});

	const tags = $derived.by(() => {
		const counts: Record<string, number> = {};
		for (const p of personas.items) {
			for (const t of p.tags) counts[t] = (counts[t] ?? 0) + 1;
		}
		return Object.entries(counts)
			.map(([tag, count]) => ({ tag, count }))
			.sort((a, b) => a.tag.localeCompare(b.tag));
	});
</script>

<TreeGroup label="Tags" count={personas.items.length}>
	<TreeRow
		icon="filter"
		label="All"
		meta={personas.items.length}
		selected={personas.tagFilter === null}
		onclick={() => (personas.tagFilter = null)}
	/>
	{#each tags as row (row.tag)}
		<TreeRow
			icon="tag"
			label={row.tag}
			mono
			meta={row.count}
			selected={personas.tagFilter === row.tag}
			onclick={() => (personas.tagFilter = row.tag)}
		/>
	{/each}
</TreeGroup>

{#if personas.rosters.length > 0}
	<TreeGroup label="Projects" count={personas.rosters.length}>
		{#each personas.rosters as roster (roster.project_id)}
			<TreeRow icon="folder" label={roster.project_name} mono meta={roster.persona_ids.length} />
		{/each}
	</TreeGroup>
{/if}
