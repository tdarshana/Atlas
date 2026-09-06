<script lang="ts">
	/** A scan warning with the home directory folded to `~`, so the line reads as a note
	 * rather than a path dump. */
	function tidyWarning(w: string): string {
		return w.replace(/\/Users\/[^/\s]+/g, '~');
	}
	// The Skills view: every skill an agent can reach globally, whether it is an Atlas
	// native skill or a `SKILL.md` folder the daemon found under `~/.claude/skills`,
	// `~/.codex/skills` or an installed plugin. Search and the Source select narrow the
	// table client-side; a row opens the skill in the panel on the right, which reads it
	// and, where Atlas may write, edits it in place.
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { replaceState } from '$app/navigation';
	import DocsPage from '$lib/components/DocsPage.svelte';
	import { practices } from '$lib/stores/docs.svelte';
	import { TabStrip, type Tab } from '$lib/shell';
	import { Button, Input, Select, Icon } from '$lib/ds';
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

	/** Skills and Practices share this view (2026-09-06): `?tab=practices` opens the
	 * rules, anything else the skill folders. */
	const STRIP: Tab[] = [
		{ id: 'skills', label: 'Skills', icon: 'graduation-cap' },
		{ id: 'practices', label: 'Practices', icon: 'book-open' }
	];
	let tab = $state<'skills' | 'practices'>(page.url.searchParams.get('tab') === 'practices' ? 'practices' : 'skills');
	// A deep link or a redirect that lands with `?tab=practices` selects that strip.
	$effect(() => {
		if (page.url.searchParams.get('tab') === 'practices') tab = 'practices';
	});
	function selectTab(id: string): void {
		tab = id === 'practices' ? 'practices' : 'skills';
		const url = new URL(page.url);
		if (id === 'practices') url.searchParams.set('tab', 'practices');
		else url.searchParams.delete('tab');
		replaceState(url, {});
	}
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
	{#if tab === 'skills'}
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
	{/if}
</div>
<div class="strip-row">
	<TabStrip items={STRIP} active={tab} onselect={selectTab} testid="skills-tab" />
</div>

{#if tab === 'practices'}
<div class="pane" data-testid="practices-pane">
	<DocsPage
		store={practices}
		title="Practices"
		noun="practice"
		hint="A practice is a standing rule agents always follow, written in Markdown. Instructions agents load on demand are the Skills beside it."
	/>
</div>
{:else}
<span class="hint" data-testid="skills-note">
	Skills are SKILL.md folders agents load on demand. Rules that always apply are the
	<a href="/skills?tab=practices">Practices</a> on the next tab.
</span>

<div class="pane" data-testid="skills-page">
	{#if skills.error}
		<p class="bad" role="alert" data-testid="skills-error">{skills.error}</p>
	{/if}

	{#if skills.warnings.length > 0}
		<span class="hint warn" role="status" data-testid="skills-warnings"><Icon name="alert-triangle" size={12} />{skills.warnings.map(tidyWarning).join(' · ')}</span>
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

{/if}

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
	.warn {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		color: var(--warning-text);
	}
	.strip-row {
		display: flex;
		align-items: center;
		margin: 2px 0 8px;
	}
</style>
