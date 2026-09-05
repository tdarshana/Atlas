<script lang="ts">
	// The Personas view: the library of personas a project can put on its roster. Search
	// and the side panel's tag narrow the table client-side; a row opens the persona in
	// the panel on the right, which edits a local copy and writes it back with Save.
	import { onMount, untrack } from 'svelte';
	import { Button, Icon, IconButton, Input, MultiSelect, Select, TagInput } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { relativeAge } from '$lib/format';
	import { setStatusItems } from '$lib/shell';
	import {
		closePersona,
		followPersonaChanges,
		loadPersonas,
		loadUsage,
		openPersona,
		personas,
		projectsUsing,
		removePersona,
		savePersona
	} from '$lib/stores/personas.svelte';
	import type { Case, Persona, PersonaAccess, PersonaPatch, PersonaRule } from '$lib/types';
	import { autogrow } from '$lib/ui/autogrow';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import MarkdownView from '$lib/ui/MarkdownView.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	const CASES: Case[] = ['plan', 'implement', 'review', 'test', 'document', 'default'];
	const MEMORY_RULES: PersonaRule[] = ['allow', 'deny', 'review'];
	const MOVE_RULES: PersonaRule[] = ['allow', 'deny'];

	type ListKey = 'skills' | 'workflows' | 'practices' | 'mcp_servers';

	/** The editable copy of the open persona; `id` is null for a persona not yet created. */
	interface Draft {
		id: string | null;
		name: string;
		role: string;
		summary: string;
		instructions: string;
		skills: string[];
		workflows: string[];
		practices: string[];
		mcp_servers: string[];
		access: PersonaAccess;
		models: Record<Case, string>;
		tags: string[];
	}

	interface PickOption {
		value: string;
		label: string;
	}

	function emptyDraft(): Draft {
		return {
			id: null,
			name: '',
			role: '',
			summary: '',
			instructions: '',
			skills: [],
			workflows: [],
			practices: [],
			mcp_servers: [],
			access: { memory_write: 'allow', task_move: 'allow', workflow_trigger: 'allow' },
			models: { plan: '', implement: '', review: '', test: '', document: '', default: '' },
			tags: []
		};
	}

	function fromPersona(p: Persona): Draft {
		const models = emptyDraft().models;
		for (const c of CASES) models[c] = p.models[c] ?? '';
		return {
			id: p.id,
			name: p.name,
			role: p.role,
			summary: p.summary,
			instructions: p.instructions,
			skills: [...p.skills],
			workflows: [...p.workflows],
			practices: [...p.practices],
			mcp_servers: [...p.mcp_servers],
			access: { ...p.access },
			models,
			tags: [...p.tags]
		};
	}

	function toPatch(d: Draft): PersonaPatch {
		const models: Partial<Record<Case, string>> = {};
		for (const c of CASES) {
			const m = d.models[c].trim();
			if (m) models[c] = m;
		}
		return {
			name: d.name.trim(),
			role: d.role.trim(),
			summary: d.summary.trim(),
			instructions: d.instructions,
			skills: [...d.skills],
			workflows: [...d.workflows],
			practices: [...d.practices],
			mcp_servers: [...d.mcp_servers],
			access: { ...d.access },
			models,
			tags: [...d.tags]
		};
	}

	let search = $state('');
	let sortDir = $state<'asc' | 'desc'>('asc');
	let draft = $state<Draft | null>(null);
	let saving = $state(false);
	let confirming = $state(false);
	let instructionsEditing = $state(false);

	let skillOptions = $state<PickOption[]>([]);
	let workflowOptions = $state<PickOption[]>([]);
	let practiceOptions = $state<PickOption[]>([]);
	let serverOptions = $state<PickOption[]>([]);
	let pickersLoaded = false;

	const rows = $derived.by(() => {
		const q = search.trim().toLowerCase();
		const tag = personas.tagFilter;
		const out = personas.items.filter(
			(p) =>
				(tag === null || p.tags.includes(tag)) &&
				(q === '' || [p.name, p.role, p.summary, ...p.tags].some((s) => s.toLowerCase().includes(q)))
		);
		const dir = sortDir === 'asc' ? 1 : -1;
		out.sort((a, b) => a.name.localeCompare(b.name) * dir);
		return out;
	});

	/** A draft without an id is drafted in the modal; everything else docks on the right. */
	const creating = $derived(draft !== null && draft.id === null);
	const showDetail = $derived(
		(draft !== null && !creating) || personas.openLoading || !!personas.openError
	);
	const canSave = $derived(!!draft && draft.name.trim() !== '' && !saving);

	onMount(() => {
		void loadPersonas();
		void loadUsage();
		return followPersonaChanges();
	});

	$effect(() => {
		setStatusItems({ right: [{ text: `${personas.items.length} personas` }] });
	});

	// A reload of the same persona (a change event, or Save) leaves the draft alone so
	// typing in progress is kept; another persona, or none, replaces it. A new draft has
	// no id and no `open`, so it survives until it is saved or closed.
	$effect(() => {
		const open = personas.open;
		untrack(() => {
			if (!open) {
				if (draft && draft.id !== null) draft = null;
				return;
			}
			if (draft?.id === open.id) return;
			draft = fromPersona(open);
			instructionsEditing = false;
			confirming = false;
			void loadPickers();
		});
	});

	/** The four pick lists, read once the first time the detail opens. */
	async function loadPickers(): Promise<void> {
		if (pickersLoaded) return;
		pickersLoaded = true;
		const failed: string[] = [];
		const [s, w, d, m] = await Promise.all([
			api()
				.listSkills(null)
				.catch(() => (failed.push('skills'), null)),
			api()
				.listWorkflows(null)
				.catch(() => (failed.push('workflows'), null)),
			api()
				.listDocs('practice')
				.catch(() => (failed.push('practices'), null)),
			api()
				.listMcpServers(null)
				.catch(() => (failed.push('MCP servers'), null))
		]);
		if (s) skillOptions = s.skills.map((x) => ({ value: x.id, label: x.name }));
		if (w) workflowOptions = w.map((x) => ({ value: x.name, label: x.name }));
		if (d) practiceOptions = d.map((x) => ({ value: x.id, label: x.name }));
		if (m) serverOptions = m.servers.map((x) => ({ value: x.id, label: x.name }));
		if (failed.length > 0) push('error', `Could not load ${failed.join(', ')}`);
	}

	function setList(key: ListKey, values: string[]): void {
		if (draft) draft[key] = values;
	}

	function open(p: Persona): void {
		void openPersona(p.id);
	}

	function onRowKeydown(event: KeyboardEvent, p: Persona): void {
		if (event.key === 'Enter' || event.key === ' ') {
			event.preventDefault();
			open(p);
		}
	}

	function startNew(): void {
		closePersona();
		draft = emptyDraft();
		instructionsEditing = false;
		confirming = false;
		void loadPickers();
	}

	function close(): void {
		closePersona();
		draft = null;
		confirming = false;
	}

	function beginInstructions(): void {
		instructionsEditing = true;
	}

	/** Attached to the create dialog: `showModal()` is the only way to get the top layer. */
	function showModal(el: HTMLDialogElement): void {
		if (!el.open) el.showModal();
		// The dialog would otherwise land focus on the close button and ring it.
		el.querySelector<HTMLInputElement>('[data-testid="persona-name"]')?.focus();
	}

	/** Attached to the editor so it takes focus the moment it is drawn. */
	function focusOnMount(el: HTMLTextAreaElement): void {
		el.focus();
	}

	function onInstructionsDisplayClick(event: MouseEvent): void {
		if ((event.target as HTMLElement).closest('a[href]')) return;
		beginInstructions();
	}

	function onInstructionsDisplayKeydown(event: KeyboardEvent): void {
		if (event.key === 'Enter' || event.key === ' ') {
			event.preventDefault();
			beginInstructions();
		}
	}

	function onInstructionsKeydown(event: KeyboardEvent): void {
		if (event.key === 'Escape') {
			event.preventDefault();
			event.stopPropagation();
			instructionsEditing = false;
		}
	}

	async function save(): Promise<void> {
		if (!draft || !canSave) return;
		saving = true;
		try {
			const saved = await savePersona(draft.id, toPatch(draft));
			draft = fromPersona(saved);
			instructionsEditing = false;
			push('success', 'Persona saved');
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			saving = false;
		}
	}

	async function confirmDelete(): Promise<void> {
		if (!draft?.id || saving) return;
		const id = draft.id;
		saving = true;
		try {
			await removePersona(id);
			confirming = false;
			draft = null;
			push('success', 'Persona deleted');
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			saving = false;
		}
	}
</script>

<div class="title-row">
	<span class="title">Personas</span>
	<span class="spacer"></span>
	<Input
		placeholder="Search personas"
		aria-label="Search personas"
		icon="search"
		bind:value={search}
		data-testid="persona-search"
	/>
	<Button size="sm" data-testid="persona-new" onclick={startNew}>New persona</Button>
</div>

<div class="pane" data-testid="personas-page">
	{#if personas.error}
		<p class="bad" role="alert" data-testid="personas-error">{personas.error}</p>
	{/if}

	<div class="split">
		{#if !personas.loading && personas.items.length === 0 && !personas.error}
			<EmptyState title="No personas yet" hint="A persona is a role a project can put on its roster.">
				<Button size="sm" onclick={startNew}>New persona</Button>
			</EmptyState>
		{:else}
			<div class="table-wrap" data-testid="personas-table">
				<div class="table" role="grid">
					<div class="header" role="rowgroup">
						<div class="header-row" role="row">
							<div
								role="columnheader"
								class="header-cell"
								aria-sort={sortDir === 'asc' ? 'ascending' : 'descending'}
							>
								<button
									type="button"
									class="header-button group-heading"
									onclick={() => (sortDir = sortDir === 'asc' ? 'desc' : 'asc')}
								>
									<Icon name={sortDir === 'asc' ? 'arrow-up' : 'arrow-down'} size={11} />
									<span>Name</span>
								</button>
							</div>
							<div role="columnheader" class="header-cell group-heading">Role</div>
							<div role="columnheader" class="header-cell group-heading">Skills</div>
							<div role="columnheader" class="header-cell group-heading">Workflows</div>
							<div role="columnheader" class="header-cell group-heading">Projects</div>
							<div role="columnheader" class="header-cell group-heading">Updated</div>
						</div>
					</div>
					{#if rows.length === 0}
						<div class="empty">
							<span class="hint">
								{personas.loading ? 'Loading…' : 'No persona matches this search.'}
							</span>
						</div>
					{:else}
						<div class="body" role="rowgroup">
							{#each rows as p (p.id)}
								<div
									class="row"
									role="row"
									tabindex="0"
									class:selected={personas.selectedId === p.id}
									data-testid="persona-row-{p.slug}"
									onclick={() => open(p)}
									onkeydown={(e) => onRowKeydown(e, p)}
								>
									<div class="cell mono" role="gridcell">{p.name}</div>
									<div class="cell" role="gridcell" title={p.role}>{p.role}</div>
									<div class="cell mono" role="gridcell">{p.skills.length}</div>
									<div class="cell mono" role="gridcell">{p.workflows.length}</div>
									<div class="cell mono" role="gridcell">{projectsUsing(p.id)}</div>
									<div class="cell" role="gridcell">{relativeAge(p.updated_at)} ago</div>
								</div>
							{/each}
						</div>
					{/if}
				</div>
			</div>
		{/if}

		{#if showDetail}
			{@render panel()}
		{/if}
	</div>
</div>

<!-- A persona not yet created is drafted in a modal rather than the docked panel; once
     saved it has an id and the panel takes over. As on the board, the dialog is bare and
     the panel brings its own chrome. -->
{#if creating}
	<dialog
		{@attach showModal}
		class="detail-modal"
		data-testid="persona-create"
		onclose={close}
		onclick={(e) => {
			if (e.target === e.currentTarget) close();
		}}
	>
		{@render panel()}
	</dialog>
{/if}

{#snippet panel()}
	<aside class="detail" aria-label="Persona detail" data-testid="persona-detail">
		<header>
			<span class="name" data-testid="persona-detail-name">
				{draft ? draft.name || 'New persona' : 'Persona'}
			</span>
			<span class="spacer"></span>
			<IconButton
				icon="x"
				label="Close persona detail"
				onclick={close}
				data-testid="persona-detail-close"
			/>
		</header>

		<div class="detail-body">
			{#if personas.openError}
				<p class="bad" role="alert" data-testid="persona-detail-error">{personas.openError}</p>
			{/if}
			{#if draft}
				{@render form()}
			{:else if personas.openLoading}
				<span class="hint">Loading…</span>
			{/if}
		</div>
	</aside>
{/snippet}

{#snippet form()}
	{#if draft}
		<Input label="Name" bind:value={draft.name} data-testid="persona-name" />
		<Input label="Role" bind:value={draft.role} data-testid="persona-role" />
		<span class="dbm-field full">
			<label class="dbm-field__label" for="persona-summary">Summary</label>
			<Textarea id="persona-summary" bind:value={draft.summary} rows={2} data-testid="persona-summary" />
		</span>

		<div class="field full">
			<div class="field-head">
				<span>Instructions</span>
				{#if !instructionsEditing}
					<IconButton
						size="sm"
						icon="pencil"
						label="Edit instructions"
						data-testid="persona-instructions-edit"
						onclick={beginInstructions}
					/>
				{/if}
			</div>
			{#if instructionsEditing}
				<textarea
					{@attach focusOnMount}
					bind:value={draft.instructions}
					use:autogrow
					class="area instructions-editor"
					rows="4"
					placeholder="No instructions"
					aria-label="Instructions"
					data-testid="persona-instructions"
					onkeydown={onInstructionsKeydown}
					onblur={() => (instructionsEditing = false)}
				></textarea>
			{:else}
				<div
					class="instructions-display"
					role="button"
					tabindex="0"
					data-testid="persona-instructions-text"
					onclick={onInstructionsDisplayClick}
					onkeydown={onInstructionsDisplayKeydown}
				>
					<MarkdownView
						source={draft.instructions}
						showHeader={false}
						emptyText="No instructions"
					/>
				</div>
			{/if}
		</div>

		{@render picker('skills', 'Skills', skillOptions)}
		{@render picker('workflows', 'Workflows', workflowOptions)}
		{@render picker('practices', 'Practices', practiceOptions)}
		{@render picker('mcp_servers', 'MCP servers', serverOptions)}

		<div class="field full">
			<div class="field-head"><span>Models</span></div>
			<div class="models" data-testid="persona-models">
				{#each CASES as c (c)}
					<label class="model-label" for="persona-model-{c}">{c}</label>
					<Input
						id="persona-model-{c}"
						mono
						placeholder="inherit"
						bind:value={draft.models[c]}
						data-testid="persona-model-{c}"
					/>
				{/each}
			</div>
		</div>

		<div class="field full">
			<div class="field-head"><span>Access</span></div>
			<div class="access">
				<Select
					size="sm"
					label="Memory write"
					options={MEMORY_RULES}
					value={draft.access.memory_write}
					onchange={(e) => {
						if (draft) draft.access.memory_write = e.currentTarget.value as PersonaRule;
					}}
					data-testid="persona-access-memory_write"
				/>
				<Select
					size="sm"
					label="Task move"
					options={MOVE_RULES}
					value={draft.access.task_move}
					onchange={(e) => {
						if (draft) draft.access.task_move = e.currentTarget.value as PersonaRule;
					}}
					data-testid="persona-access-task_move"
				/>
				<Select
					size="sm"
					label="Workflow trigger"
					options={MOVE_RULES}
					value={draft.access.workflow_trigger}
					onchange={(e) => {
						if (draft) draft.access.workflow_trigger = e.currentTarget.value as PersonaRule;
					}}
					data-testid="persona-access-workflow_trigger"
				/>
			</div>
		</div>

		<div class="full">
			<TagInput
				label="Tags"
				value={draft.tags}
				onchange={(v) => {
					if (draft) draft.tags = v;
				}}
				testId="persona-tags"
			/>
		</div>

		<div class="actions full">
			<Button
				size="sm"
				variant="primary"
				disabled={!canSave}
				onclick={save}
				data-testid="persona-save"
			>
				{saving ? 'Saving…' : 'Save'}
			</Button>
			<span class="spacer"></span>
			{#if draft.id !== null}
				{#if confirming}
					<span class="hint">Delete this persona?</span>
					<Button
						size="sm"
						variant="danger"
						disabled={saving}
						onclick={confirmDelete}
						data-testid="persona-delete-confirm"
					>
						{saving ? 'Deleting…' : 'Delete'}
					</Button>
					<Button size="sm" variant="ghost" onclick={() => (confirming = false)}>
						Keep
					</Button>
				{:else}
					<Button
						size="sm"
						variant="ghost"
						onclick={() => (confirming = true)}
						data-testid="persona-delete"
					>
						Delete…
					</Button>
				{/if}
			{/if}
		</div>
	{/if}
{/snippet}

{#snippet picker(key: ListKey, label: string, options: PickOption[])}
	{#if draft}
		<MultiSelect
			{label}
			{options}
			value={draft[key]}
			placeholder="Select {label.toLowerCase()}"
			onchange={(v) => setList(key, v)}
			testId="persona-pick-{key}"
		/>
	{/if}
{/snippet}



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

	/* The empty state fills the pane like the table does, so the detail stays docked at
	   the right edge instead of sitting beside a content-sized box. */
	.split > :global(.empty) {
		flex: 1;
	}

	/* The table is drawn here rather than through the ds Table so each row can carry a
	   test id; the measurements match the ds Table's 28px header and rows. */
	.table-wrap {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		border: 1px solid var(--border-subtle);
		border-radius: 3px;
		overflow: hidden;
	}

	.table {
		display: flex;
		flex-direction: column;
		min-height: 0;
		--cols: minmax(140px, 200px) 1fr 60px 80px 70px 110px;
	}

	.header {
		flex: 0 0 28px;
		user-select: none;
	}

	.header-row,
	.row {
		display: grid;
		grid-template-columns: var(--cols);
		align-items: center;
		height: 28px;
		padding: 0 12px;
		column-gap: 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.header-cell {
		display: flex;
		align-items: center;
		height: 100%;
		padding: 0 8px 0 0;
		border-right: 1px solid var(--border-subtle);
	}

	.header-cell:last-child {
		border-right: 0;
		padding-right: 0;
	}

	.header-button {
		display: flex;
		align-items: center;
		gap: 4px;
		width: 100%;
		height: 100%;
		padding: 0;
		margin: 0;
		border: 0;
		background: transparent;
		color: inherit;
		font: inherit;
		text-align: inherit;
		cursor: pointer;
	}

	.body {
		overflow-y: auto;
		min-height: 0;
	}

	.row {
		flex: 0 0 28px;
		cursor: pointer;
	}

	.row:last-child {
		border-bottom: 0;
	}

	.row:hover {
		background: var(--bg-hover);
	}

	.row.selected {
		background: var(--accent-muted);
	}

	.row:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: -2px;
	}

	.cell {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.cell.mono {
		font-family: var(--font-mono);
		font-variant-numeric: var(--tabular);
	}

	.empty {
		display: flex;
		align-items: center;
		justify-content: center;
		padding: var(--space-6) var(--space-4);
	}

	.detail {
		position: relative;
		width: 640px;
		flex: 0 0 640px;
		max-width: 70%;
		display: flex;
		flex-direction: column;
		min-height: 0;
		border: 1px solid var(--border-subtle);
		border-radius: 3px;
		background: var(--bg-raised);
	}

	header {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 32px;
		flex: 0 0 32px;
		padding: 0 8px 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.name {
		font-weight: 600;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	/* Two columns: name beside role, the four pickers two by two, and the wide fields
	   (summary, instructions, models, access, tags, the actions) spanning both, so the
	   editor reads as a form rather than a single tall scroll. */
	.detail-body {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
		align-content: start;
		gap: 10px 14px;
		padding: 12px;
	}

	.detail-body > .full,
	.detail-body > :global(.bad) {
		grid-column: 1 / -1;
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.field-head {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	.field-head > span:first-child {
		font-size: 12px;
		color: var(--text-secondary);
	}

	.area {
		resize: none;
		overflow: hidden;
		background: var(--bg-base);
		border: 1px solid var(--border-default);
		border-radius: 3px;
		color: var(--text-primary);
		font-family: var(--font-ui);
		font-size: 12px;
		padding: 6px 8px;
	}

	.area:focus {
		border-color: var(--accent);
		outline: var(--focus-ring-width) solid var(--focus-ring);
		outline-offset: 0;
	}

	.instructions-display {
		cursor: pointer;
		border-radius: 3px;
	}

	.instructions-display:focus-visible {
		outline: var(--focus-ring-width) solid var(--focus-ring);
		outline-offset: 2px;
	}

	/* The create modal mirrors the board's task detail modal: a bare dialog around the
	   same panel at the same size. */
	.detail-modal {
		padding: 0;
		border: 0;
		background: transparent;
		overflow: visible;
	}

	.detail-modal::backdrop {
		background: rgb(0 0 0 / 0.45);
	}

	/* In the modal Save sits at the bottom right, as a dialog's primary action does. */
	.detail-modal .actions {
		justify-content: flex-end;
	}

	.detail-modal .actions > .spacer {
		display: none;
	}

	.detail-modal .detail {
		width: min(820px, 92vw);
		max-width: none;
		height: auto;
		max-height: min(80vh, 920px);
		overflow: hidden;
	}

	/* Same reasoning as the task detail: MarkdownView caps its embedded body, and this
	   panel scrolls as a whole instead, so the cap is lifted with a more specific rule. */
	.instructions-display :global(.body.embedded) {
		padding: 6px 8px;
		flex: none;
		max-height: none;
		overflow: visible;
	}

	.instructions-display :global(.body p:first-child) {
		margin-top: 0;
	}


	.models {
		display: grid;
		grid-template-columns: 80px minmax(0, 1fr) 80px minmax(0, 1fr);
		align-items: center;
		gap: 4px 10px;
	}

	.model-label {
		font-size: 12px;
		font-family: var(--font-mono);
		color: var(--text-secondary);
	}

	.access {
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		gap: 8px;
	}

	.actions {
		display: flex;
		align-items: center;
		gap: 6px;
		padding-top: 4px;
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
