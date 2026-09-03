<script lang="ts">
	// Facets over the whole active set. The store's `hits` are already narrowed by the kind
	// filter, so counting those would zero every kind the user just filtered out and leave
	// them nothing to widen back to. `memories.all` is the same load before the kind filter,
	// so the panel reads that and issues no request of its own.
	import { MEMORY_KINDS, memories, toggleKind } from '$lib/stores/memories.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	const facets = $derived(memories.all);

	/** Descending by count, then alphabetical, so the panel does not jump about. */
	function ranked(counts: Map<string, number>): [string, number][] {
		return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
	}

	const kindCounts = $derived.by(() => {
		const counts = new Map<string, number>(MEMORY_KINDS.map((k) => [k, 0]));
		for (const hit of facets) counts.set(hit.memory.kind, (counts.get(hit.memory.kind) ?? 0) + 1);
		return counts;
	});

	const tags = $derived.by(() => {
		const counts = new Map<string, number>();
		for (const hit of facets) {
			for (const tag of hit.memory.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
		}
		return ranked(counts);
	});

	const sources = $derived.by(() => {
		const counts = new Map<string, number>();
		for (const hit of facets) {
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
