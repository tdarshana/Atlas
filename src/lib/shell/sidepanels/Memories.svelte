<script lang="ts">
	// Facets over the memories the list currently holds. Kinds are the one filter the store
	// owns, so those rows toggle it; tags and sources are counts the list reports.
	import { MEMORY_KINDS, memories, scheduleLoad, toggleKind } from '$lib/stores/memories.svelte';
	import type { MemoryKind } from '$lib/types';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	/** Descending by count, then alphabetical, so the panel does not jump about. */
	function ranked(counts: Map<string, number>): [string, number][] {
		return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
	}

	const kindCounts = $derived.by(() => {
		const counts = new Map<string, number>(MEMORY_KINDS.map((k) => [k, 0]));
		for (const hit of memories.hits) {
			counts.set(hit.memory.kind, (counts.get(hit.memory.kind) ?? 0) + 1);
		}
		return counts;
	});

	const tags = $derived.by(() => {
		const counts = new Map<string, number>();
		for (const hit of memories.hits) {
			for (const tag of hit.memory.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
		}
		return ranked(counts);
	});

	const sources = $derived.by(() => {
		const counts = new Map<string, number>();
		for (const hit of memories.hits) {
			const source = hit.memory.source_agent ?? hit.memory.source_tool ?? 'unknown';
			counts.set(source, (counts.get(source) ?? 0) + 1);
		}
		return ranked(counts);
	});

	function filter(kind: MemoryKind) {
		toggleKind(kind);
		scheduleLoad(0);
	}
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
			onclick={() => filter(kind)}
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
