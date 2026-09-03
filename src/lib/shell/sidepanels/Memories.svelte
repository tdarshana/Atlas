<script lang="ts">
	// Facets over the whole active set. The store's `hits` are already narrowed by the kind
	// filter, so counting those would zero every kind the user just filtered out and leave
	// them nothing to widen back to. The panel keeps its own unfiltered copy instead, taken
	// from the same list route without a kind filter, and refreshes it whenever the list
	// settles or its scope moves.
	import { api } from '$lib/daemon.svelte';
	import { MEMORY_KINDS, memories, toggleKind } from '$lib/stores/memories.svelte';
	import type { Memory } from '$lib/types';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	let facets = $state<Memory[]>([]);

	/** The scope filter the list is under, minus the kinds. */
	function scopedProject(): string | null {
		return memories.scope === 'project' && memories.projectId ? memories.projectId : null;
	}

	function inScope(memory: Memory): boolean {
		return memories.scope === 'all' || memory.scope === memories.scope;
	}

	async function refresh(): Promise<void> {
		try {
			const all = await api().listMemories('active', scopedProject());
			facets = all.filter(inScope);
		} catch {
			// A facet list is not worth an error state; the last good counts stay up.
		}
	}

	$effect(() => {
		// Re-read once the store settles, and whenever the scope it reads under changes.
		const busy = memories.loading;
		void memories.scope;
		void memories.projectId;
		if (!busy) void refresh();
	});

	/** Descending by count, then alphabetical, so the panel does not jump about. */
	function ranked(counts: Map<string, number>): [string, number][] {
		return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
	}

	const kindCounts = $derived.by(() => {
		const counts = new Map<string, number>(MEMORY_KINDS.map((k) => [k, 0]));
		for (const memory of facets) counts.set(memory.kind, (counts.get(memory.kind) ?? 0) + 1);
		return counts;
	});

	const tags = $derived.by(() => {
		const counts = new Map<string, number>();
		for (const memory of facets) {
			for (const tag of memory.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
		}
		return ranked(counts);
	});

	const sources = $derived.by(() => {
		const counts = new Map<string, number>();
		for (const memory of facets) {
			const source = memory.source_agent ?? memory.source_tool ?? 'unknown';
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
