<script lang="ts">
	// The Skills view: every skill an agent can reach globally, whether it is an Atlas
	// native skill or a `SKILL.md` folder the daemon found under `~/.claude/skills`,
	// `~/.codex/skills` or an installed plugin. Search and the Source select narrow the
	// table client-side; a row opens the skill in the panel on the right, which reads it
	// and, where Atlas may write, edits it in place.
	import { onMount } from 'svelte';
	import { Button, Input, Select } from '$lib/ds';
	import NewSkillDialog from '$lib/components/skills/NewSkillDialog.svelte';
	import SkillDetail from '$lib/components/skills/SkillDetail.svelte';
	import SkillTable from '$lib/components/skills/SkillTable.svelte';
	import { setStatusItems } from '$lib/shell';
	import { filterSkills, SOURCE_OPTIONS, type SourceFilter } from '$lib/skills';
	import {
		closeSkill,
		createSkill,
		deleteSkill,
		loadSkills,
		openSkill,
		saveBody,
		setDetailWidth,
		skills
	} from '$lib/stores/skills.svelte';
	import type { NewSkill, SkillSummary } from '$lib/types';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import { push } from '$lib/platform/toasts.svelte';

	let search = $state('');
	let creating = $state(false);

	const rows = $derived(filterSkills(skills.items, search, skills.source));

	onMount(() => {
		void loadSkills(null);
	});

	async function create(input: NewSkill): Promise<void> {
		await createSkill(input);
		push('success', 'Skill created');
	}

	async function remove(id: string): Promise<void> {
		await deleteSkill(id);
	}

	function open(row: SkillSummary): void {
		void openSkill(row.id, null);
	}

	$effect(() => {
		setStatusItems({ right: [{ text: `${skills.items.length} skills` }] });
	});
</script>

<div class="title-row">
	<span class="title">Skills</span>
	<span class="spacer"></span>
	<Input
		placeholder="Search skills"
		aria-label="Search skills"
		icon="search"
		bind:value={search}
		data-testid="skills-search"
	/>
	<Select
		size="sm"
		aria-label="Source"
		options={SOURCE_OPTIONS}
		value={skills.source}
		onchange={(e) => (skills.source = e.currentTarget.value as SourceFilter)}
		data-testid="skills-source"
	/>
	<Button size="sm" data-testid="skills-new" onclick={() => (creating = true)}>New skill</Button>
</div>

<div class="pane" data-testid="skills-page">
	{#if skills.error}
		<p class="bad" role="alert" data-testid="skills-error">{skills.error}</p>
	{/if}

	{#if skills.warnings.length > 0}
		<span class="hint" data-testid="skills-warnings">{skills.warnings.join(' · ')}</span>
	{/if}

	<div class="split">
		{#if !skills.loading && skills.items.length === 0 && !skills.error}
			<EmptyState
				title="No skills found"
				hint="Atlas looked in ~/.claude/skills, ~/.codex/skills and the installed plugin caches."
			>
				<Button size="sm" onclick={() => (creating = true)}>New skill</Button>
			</EmptyState>
		{:else}
			<SkillTable
				id="skills"
				{rows}
				loading={skills.loading}
				selectedId={skills.open?.id ?? null}
				emptyText="No skill matches this search."
				onopen={open}
			/>
		{/if}

		{#if skills.open || skills.openLoading || skills.openError}
			<SkillDetail
				skill={skills.open}
				loading={skills.openLoading}
				error={skills.openError}
				width={skills.detailWidth}
				onclose={closeSkill}
				onresize={setDetailWidth}
				onsave={saveBody}
				ondelete={remove}
			/>
		{/if}
	</div>
</div>

<NewSkillDialog
	open={creating}
	projectId={null}
	onclose={() => (creating = false)}
	oncreate={create}
/>

<style>
	.title-row {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 28px;
		flex: 0 0 28px;
	}

	.title {
		font-size: 15px;
		font-weight: 600;
	}

	.spacer {
		flex: 1;
	}

	.title-row :global(.dbm-input) {
		width: 220px;
	}

	/* The source filter is one short word; without a width the select stretches to
	   whatever the row leaves it. */
	.title-row :global(.dbm-select-wrap) {
		width: 160px;
		flex: 0 0 160px;
	}

	.pane {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}

	.split {
		flex: 1;
		min-height: 0;
		display: flex;
		gap: 12px;
	}

	.split > :global(.empty) {
		flex: 1;
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}
</style>
