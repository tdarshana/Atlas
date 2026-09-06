<script lang="ts">
	/** A scan warning with the home directory folded to `~`, so the line reads as a note
	 * rather than a path dump. */
	function tidyWarning(w: string): string {
		return w.replace(/\/Users\/[^/\s]+/g, '~');
	}
	// The project Skills tab: the global skills plus the ones found under this project's
	// own `.claude/skills` and `.codex/skills`, each with an `Enabled here` checkbox. The
	// checkbox writes the project's whole disabled list, the same way the MCP tab writes
	// its tool override, so one project's choices never touch another's.
	import { Button, Input, Select, Icon } from '$lib/ds';
	import NewSkillDialog from '$lib/components/skills/NewSkillDialog.svelte';
	import SkillDetail from '$lib/components/skills/SkillDetail.svelte';
	import SkillTable from '$lib/components/skills/SkillTable.svelte';
	import { errorMessage } from '$lib/errors';
	import {
		enabledSummary,
		filterSkills,
		nextDisabled,
		SOURCE_OPTIONS,
		type SourceFilter
	} from '$lib/skills';
	import { project, setHeaderActions } from '$lib/stores/project.svelte';
	import { page } from '$app/state';
	import { replaceState } from '$app/navigation';
	import ProjectPractices from '$lib/components/project/ProjectPractices.svelte';
	import { TabStrip, type Tab } from '$lib/shell';
	import {
		closeSkill,
		createSkill,
		deleteSkill,
		disabledIds,
		loadSkills,
		openSkill,
		saveBody,
		setDetailWidth,
		setDisabled,
		skills
	} from '$lib/stores/skills.svelte';
	import type { NewSkill, SkillSummary } from '$lib/types';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import { push } from '$lib/platform/toasts.svelte';

	const id = $derived(project.current?.id ?? '');

	let search = $state('');
	let creating = $state(false);
	let toggling = $state<string | null>(null);

	const rows = $derived(filterSkills(skills.items, search, skills.source));
	const summary = $derived(enabledSummary(skills.items));

	async function toggle(row: SkillSummary, enabled: boolean): Promise<void> {
		if (!id) return;
		toggling = row.id;
		try {
			await setDisabled(id, nextDisabled(disabledIds(), row.id, enabled));
		} catch (e) {
			push('error', errorMessage(e));
			// The checkbox already moved, so put the truth back on screen.
			await loadSkills(id);
		} finally {
			toggling = null;
		}
	}

	async function resetToGlobal(): Promise<void> {
		if (!id) return;
		try {
			await setDisabled(id, []);
			push('success', 'Reset to the global skill list');
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	async function create(input: NewSkill): Promise<void> {
		await createSkill(input);
		push('success', 'Skill created');
	}

	$effect(() => {
		if (!id) return;
		void loadSkills(id);
	});

	/** Skills and the project's practices share this tab (2026-09-06). */
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

	// The header lends its actions to the skills strip only; the practices strip carries
	// its own New practice button inline.
	$effect(() => {
		setHeaderActions(tab === 'skills' ? headerActions : null);
		return () => setHeaderActions(null);
	});
</script>

{#snippet headerActions()}
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
	<Button variant="ghost" size="sm" data-testid="project-skills-reset" onclick={resetToGlobal}>
		Reset to global
	</Button>
	<Button size="sm" data-testid="skills-new" onclick={() => (creating = true)}>New skill</Button>
{/snippet}

<div class="strip-row">
	<TabStrip items={STRIP} active={tab} onselect={selectTab} testid="project-skills-tab" />
</div>

{#if tab === 'practices'}
<ProjectPractices />
{:else}
<div class="pane" data-testid="project-skills-page">
	{#if skills.error}
		<p class="bad" role="alert" data-testid="skills-error">{skills.error}</p>
	{/if}

	<div class="line">
		<span class="hint" data-testid="project-skills-summary">{summary}</span>
		{#if skills.warnings.length > 0}
			<span class="hint warn" role="status" data-testid="skills-warnings"><Icon name="alert-triangle" size={12} />{skills.warnings.map(tidyWarning).join(' · ')}</span>
		{/if}
	</div>

	<div class="split">
		{#if !skills.loading && skills.items.length === 0 && !skills.error}
			<EmptyState
				title="No skills found"
				hint="Atlas looked in this project's .claude/skills and .codex/skills, the user folders and the installed plugin caches."
			>
				<Button size="sm" onclick={() => (creating = true)}>New skill</Button>
			</EmptyState>
		{:else}
			<SkillTable
				id="project-skills"
				{rows}
				showScope
				loading={skills.loading}
				selectedId={skills.open?.id ?? null}
				emptyText="No skill matches this search."
				onopen={(row) => void openSkill(row.id, id)}
				ontoggle={toggle}
				{toggling}
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
				ondelete={deleteSkill}
			/>
		{/if}
	</div>
</div>

{/if}

<NewSkillDialog
	open={creating}
	projectId={id || null}
	onclose={() => (creating = false)}
	oncreate={create}
/>

<style>
	.pane {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}

	.line {
		display: flex;
		align-items: center;
		gap: 12px;
	}

	.split {
		flex: 1;
		min-height: 0;
		display: flex;
		gap: 12px;
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
		margin-bottom: 8px;
	}
</style>
