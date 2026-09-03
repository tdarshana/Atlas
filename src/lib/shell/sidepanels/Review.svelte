<script lang="ts">
	// Where the pending memories came from, and which kinds are waiting.
	import { onMount } from 'svelte';
	import { loadReview, qualifying, review } from '$lib/stores/review.svelte';
	import { MEMORY_KINDS } from '$lib/stores/memories.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	onMount(() => {
		// The review page loads the same list; only fetch when nothing has yet.
		if (review.items.length === 0 && !review.loading) void loadReview();
	});

	const sources = $derived.by(() => {
		const counts = new Map<string, number>();
		for (const memory of review.items) {
			const source = memory.source_agent ?? memory.source_tool ?? 'unknown';
			counts.set(source, (counts.get(source) ?? 0) + 1);
		}
		return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
	});

	const kindCounts = $derived.by(() => {
		const counts = new Map<string, number>();
		for (const memory of review.items) {
			counts.set(memory.kind, (counts.get(memory.kind) ?? 0) + 1);
		}
		return counts;
	});
</script>

<TreeGroup label="Sources" count={sources.length}>
	{#each sources as [source, count] (source)}
		<TreeRow icon="terminal" label={source} mono meta={count} />
	{/each}
</TreeGroup>

<TreeGroup label="Kinds">
	{#each MEMORY_KINDS as kind (kind)}
		<TreeRow icon="circle" label={kind} meta={kindCounts.get(kind) ?? 0} />
	{/each}
</TreeGroup>

<TreeGroup label="Pending" count={review.items.length}>
	<TreeRow icon="circle-check" label="Above threshold" meta={qualifying().length} />
</TreeGroup>
