<script lang="ts">
	// The Skills card: counts by source off the same store the `/skills` view and its side
	// panel read.
	import { onMount } from 'svelte';
	import { skillCounts } from '$lib/skills';
	import { loadSkills, skills } from '$lib/stores/skills.svelte';
	import SettingsCard from './SettingsCard.svelte';

	const skillsSummaryText = $derived(
		skills.items.length === 0
			? 'No skills found yet'
			: skillCounts(skills.items)
					.bySource.filter((s) => s.count > 0)
					.map((s) => `${s.count} ${s.label.toLowerCase()}`)
					.join(' · ')
	);

	onMount(() => {
		if (skills.items.length === 0 && !skills.loading) void loadSkills(null);
	});
</script>

<SettingsCard id="skills" title="Skills">
	<span class="hint" data-testid="skills-settings-summary">{skillsSummaryText}</span>
	<span class="hint">
		The Claude Code and Codex skill folders Atlas found, the plugin skills installed
		beside them and the skills Atlas holds itself, searchable and editable on their own
		view. A project turns individual skills off on its own Skills tab.
	</span>
	<a href="/skills" data-testid="skills-open-link">Open skills</a>
</SettingsCard>

<style>
	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}
</style>
