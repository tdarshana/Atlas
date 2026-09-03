<script lang="ts">
	// The practice library and the tags across it.
	import { onMount } from 'svelte';
	import { practices } from '$lib/stores/docs.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	onMount(() => {
		if (!practices.state.loaded) void practices.load();
	});

	const tags = $derived.by(() => {
		const counts = new Map<string, number>();
		for (const doc of practices.state.list) {
			for (const tag of doc.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
		}
		return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
	});

	const scoped = $derived(practices.state.list.filter((d) => d.project_id !== null).length);
</script>

<TreeGroup label="Practices" count={practices.state.list.length}>
	{#each practices.state.list as doc (doc.id)}
		<TreeRow icon="book-open" label={doc.name} mono />
	{/each}
</TreeGroup>

<TreeGroup label="Scope">
	<TreeRow icon="layers" label="Global" meta={practices.state.list.length - scoped} />
	<TreeRow icon="folder" label="Project" meta={scoped} />
</TreeGroup>

<TreeGroup label="Tags" count={tags.length}>
	{#each tags as [tag, count] (tag)}
		<TreeRow icon="tag" label={tag} mono meta={count} />
	{/each}
</TreeGroup>
