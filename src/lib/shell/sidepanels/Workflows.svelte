<script lang="ts">
	// The workflow library and the tags across it.
	import { onMount } from 'svelte';
	import { workflows } from '$lib/stores/docs.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	onMount(() => {
		if (!workflows.state.loaded) void workflows.load();
	});

	const tags = $derived.by(() => {
		const counts = new Map<string, number>();
		for (const doc of workflows.state.list) {
			for (const tag of doc.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
		}
		return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
	});

	const scoped = $derived(workflows.state.list.filter((d) => d.project_id !== null).length);
</script>

<TreeGroup label="Workflows" count={workflows.state.list.length}>
	{#each workflows.state.list as doc (doc.id)}
		<TreeRow icon="git-branch" label={doc.name} mono />
	{/each}
</TreeGroup>

<TreeGroup label="Scope">
	<TreeRow icon="layers" label="Global" meta={workflows.state.list.length - scoped} />
	<TreeRow icon="folder" label="Project" meta={scoped} />
</TreeGroup>

<TreeGroup label="Tags" count={tags.length}>
	{#each tags as [tag, count] (tag)}
		<TreeRow icon="tag" label={tag} mono meta={count} />
	{/each}
</TreeGroup>
