<script lang="ts">
	// Kind and tag counts come from `GET /memories/facets` (`memories.facets`), over the
	// whole active set regardless of the current search text or the kind filter, so the
	// panel issues no fetch of its own and stays whole while its own chips narrow the
	// table. Sources have no facets-endpoint counterpart, so those still come from
	// `memories.all`, the same load before the kind filter.
	import { MEMORY_KINDS, memories, toggleKind } from '$lib/stores/memories.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	/** Descending by count, then alphabetical, so the panel does not jump about. */
	function ranked(counts: Map<string, number>): [string, number][] {
		return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
	}

	const kindCounts = $derived(new Map(MEMORY_KINDS.map((k) => [k, memories.facets.kinds[k] ?? 0])));

	const tags = $derived(ranked(new Map(Object.entries(memories.facets.tags))));

	const sources = $derived.by(() => {
		const counts = new Map<string, number>();
		for (const hit of memories.all) {
			const source = hit.memory.source_agent ?? hit.memory.source_tool ?? 'unknown';
			counts.set(source, (counts.get(source) ?? 0) + 1);
		}
		return ranked(counts);
	});
</script>

<TreeGroup label="Kinds">
	{#each MEMORY_KINDS as kind (kind)}
		{@const on = memories.kinds.includes(kind)}
		<TreeRow
			icon={on ? 'braces' : 'circle'}
			iconColor={on ? 'var(--info)' : 'var(--text-tertiary)'}
			label={kind}
			meta={kindCounts.get(kind) ?? 0}
			selected={on}
			onclick={() => toggleKind(kind)}
		/>
	{/each}
</TreeGroup>

<TreeGroup label="Tags" count={tags.length}>
	{#each tags as [tag, count] (tag)}
		<TreeRow icon="tag" label={tag} mono meta={count} />
	{/each}
</TreeGroup>

<TreeGroup label="Sources" count={sources.length}>
	{#each sources as [source, count] (source)}
		<TreeRow icon="terminal" label={source} mono meta={count} />
	{/each}
</TreeGroup>
