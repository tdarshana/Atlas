<script lang="ts">
	// Two groups over the same list the `/skills` table draws: a row per source that sets
	// the Source select, and a row per plugin that contributes skills. Counts come from
	// the store, so a skill created on the route is counted here without a second fetch.
	import { onMount } from 'svelte';
	import type { IconName } from '$lib/ds';
	import { skillCounts, type SourceGroup } from '$lib/skills';
	import { loadSkills, skills } from '$lib/stores/skills.svelte';
	import { practices } from '$lib/stores/docs.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	onMount(() => {
		void practices.load();
		// The route loads the same list; only fetch when nothing has yet.
		if (skills.items.length === 0 && !skills.loading) void loadSkills(skills.projectId);
	});

	const counts = $derived(skillCounts(skills.items));

	const SOURCE_ICONS: Record<SourceGroup, IconName> = {
		native: 'database',
		claude: 'bot',
		codex: 'terminal',
		plugin: 'plug'
	};
</script>

<TreeGroup label="Sources" count={counts.total}>
	<TreeRow
		icon="filter"
		label="All"
		meta={counts.total}
		selected={skills.source === 'all'}
		onclick={() => (skills.source = 'all')}
	/>
	{#each counts.bySource as row (row.source)}
		<TreeRow
			icon={SOURCE_ICONS[row.source]}
			label={row.label}
			meta={row.count}
			selected={skills.source === row.source}
			onclick={() => (skills.source = row.source)}
		/>
	{/each}
</TreeGroup>

{#if counts.byPlugin.length > 0}
	<TreeGroup label="Plugins" count={counts.byPlugin.length}>
		{#each counts.byPlugin as row (row.plugin)}
			<TreeRow
				icon="plug"
				label={row.plugin}
				mono
				meta={row.count}
				selected={skills.source === 'plugin'}
				onclick={() => (skills.source = 'plugin')}
			/>
		{/each}
	</TreeGroup>
{/if}

<TreeGroup label="Practices" count={practices.state.list.length}>
	<TreeRow icon="book-open" label="All practices" meta={practices.state.list.length} href="/skills?tab=practices" />
</TreeGroup>
