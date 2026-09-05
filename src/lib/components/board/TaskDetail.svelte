<script lang="ts">
	// The open task, docked at the right of the lane strip per frame 02.2b: its fields,
	// its blockers and children, its history, and the writes a person can make from here.
	// Every write goes through the board store, which patches the card from the daemon's
	// reply, and then asks the board to reload, so this panel holds no state the server
	// does not.
	//
	// The form logic is the Phase 4 drawer's, moved rather than rewritten: the dirty-aware
	// refill, the 409 reload, the delete confirmation and the toasts all behave as before.
	import { onMount, untrack } from 'svelte';
	import { Badge, Button, Icon, IconButton, Input, Select } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { relativeAge } from '$lib/format';
	import { copyText, TabStrip, type Tab } from '$lib/shell';
	import {
		type DetailMode,
		type DetailTab,
		claim,
		comment as postComment,
		ConflictError,
		CONFLICT_MESSAGE,
		DETAIL_MAX,
		DETAIL_MIN,
		detailTab,
		importFramework,
		removeTask,
		setBlockers,
		setDetailTab,
		updateTask
	} from '$lib/stores/board.svelte';
	import { personas } from '$lib/stores/personas.svelte';
	import { FRAMEWORK_LABEL, reportText } from '$lib/components/project/frameworks';
	import PluginFrame from '$lib/plugins/PluginFrame.svelte';
	import { contributions, loadPlugins, pluginById, plugins } from '$lib/plugins/host.svelte';
	import type { Stage, TaskDetail, TaskEvent, TaskKind, TaskPriority } from '$lib/types';
	import { autogrow } from '$lib/ui/autogrow';
	import Dialog from '$lib/ui/Dialog.svelte';
	import MarkdownView from '$lib/ui/MarkdownView.svelte';
	import ResizeBar from '$lib/ui/ResizeBar.svelte';
	import { push } from '$lib/platform/toasts.svelte';

	interface Props {
		detail: TaskDetail | null;
		stages: Stage[];
		loading: boolean;
		error: string | null;
		width: number;
		onclose: () => void;
		/** Docked beside the lanes or floating as a dialog, with the switch between them. */
		mode: DetailMode;
		ontogglemode: () => void;
		/** Called after any write, so the board and this panel both reload. */
		onchanged: () => void | Promise<void>;
		/** Moves go through the board so the card moves at the same moment. */
		onmove: (key: string, stage: string) => void;
		ondeleted: () => void | Promise<void>;
		onresize: (width: number) => void;
		/** Opens a subtask or the parent in this same detail. Absent in tests that don't need it. */
		onopen?: (key: string) => void;
		/** The task this one was opened from, for the back button; null hides it. */
		backKey?: string | null;
		onback?: () => void;
	}

	let {
		detail,
		stages,
		loading,
		error,
		width,
		onclose,
		mode,
		ontogglemode,
		onchanged,
		onmove,
		ondeleted,
		onresize,
		onopen,
		backKey = null,
		onback
	}: Props = $props();

	import { KIND_OPTIONS } from './kind';
	const PRIORITIES: TaskPriority[] = ['low', 'medium', 'high', 'urgent'];

	const kindOptions = KIND_OPTIONS;
	const priorityOptions = PRIORITIES.map((p) => ({ value: p, label: p }));
	const stageOptions = $derived(stages.map((s) => ({ value: s.name, label: s.name })));
	const personaOptions = $derived([
		{ value: '', label: 'None' },
		...personas.roster.map((r) => ({ value: r.slug, label: r.name }))
	]);

	const task = $derived(detail?.task ?? null);

	// The lower half's three tabs.
	const activityEvents = $derived(detail?.events.filter((e) => e.kind !== 'commented') ?? []);
	const commentEvents = $derived(detail?.events.filter((e) => e.kind === 'commented') ?? []);
	const tabs = $derived([
		{ id: 'subtasks', label: 'Subtasks', icon: 'list-checks', count: detail?.children.length ?? 0 },
		{ id: 'activity', label: 'Activity', icon: 'activity', count: activityEvents.length },
		{ id: 'comments', label: 'Comments', icon: 'message-square', count: commentEvents.length }
	] satisfies Tab[]);

	// The `task.detail.panel` slot, below the tabs. Each frame is told which task is open
	// through the bridge's `atlas:context`, so it follows the selection without remounting.
	const PLUGIN_PANEL_MAX_HEIGHT = 480;
	const pluginPanels = $derived(contributions().components['task.detail.panel'] ?? []);
	const pluginContext = $derived(task ? { taskKey: task.key } : {});

	onMount(() => {
		if (!plugins.loaded && plugins.available) void loadPlugins();
	});

	let title = $state('');
	let description = $state('');
	// Held as plain strings because `Select` binds a string; the values can only be
	// the options above, so the cast on save is safe.
	let kind = $state('task');
	let priority = $state('medium');
	let assignee = $state('');
	let labels = $state('');
	// Written the moment it changes rather than on Save, so it always follows the server.
	let persona = $state('');
	let blockerKey = $state('');
	let comment = $state('');

	let saving = $state(false);
	let busy = $state(false);
	let confirming = $state(false);
	let titleError = $state<string | null>(null);
	let panel = $state<HTMLElement>();
	let keyCopied = $state(false);

	async function copyKey(): Promise<void> {
		if (!task) return;
		try {
			await copyText(task.key);
			keyCopied = true;
			setTimeout(() => (keyCopied = false), 1500);
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	/** The server values the draft was last filled from, to tell an edit from staleness. */
	let base = {
		id: '',
		title: '',
		description: '',
		kind: '',
		priority: '',
		assignee: '',
		labels: ''
	};

	// Every write here reloads the task, so refilling the whole form on each reload would
	// throw away unsaved typing the moment someone clicked Claim. A field the user has not
	// touched follows the server; one they have keeps their text. A different task refills
	// everything, since none of that typing belongs to it.
	$effect(() => {
		const t = detail?.task;
		if (!t) return;
		const next = {
			id: t.id,
			title: t.title,
			description: t.description,
			kind: t.kind as string,
			priority: t.priority as string,
			assignee: t.assignee ?? '',
			labels: t.labels.join(', ')
		};
		// Trimmed on both sides: Save sends `title.trim()`, `assignee.trim()` and a
		// trimmed label list, so a field the user left padded comes back from the
		// daemon trimmed and would otherwise look edited from here on and stop
		// following the server.
		const same = (a: string, b: string) => a.trim() === b.trim();
		untrack(() => {
			const other = next.id !== base.id;
			if (other || same(title, base.title)) title = next.title;
			if (other || same(description, base.description)) description = next.description;
			if (other || same(kind, base.kind)) kind = next.kind;
			if (other || same(priority, base.priority)) priority = next.priority;
			if (other || same(assignee, base.assignee)) assignee = next.assignee;
			if (other || same(labels, base.labels)) labels = next.labels;
			persona = t.persona_slug ?? '';
			if (other) titleError = null;
			base = next;
			// Opening another task or a reload after a write (Claim, a blocker, a comment)
			// always lands on the server's own values, so an inline editor left open would
			// be showing a draft against a task that has already moved on.
			titleEditing = false;
			descriptionEditing = false;
		});
	});

	onMount(() => {
		// The panel is what the click opened, so the keyboard starts here rather than
		// back at the top of the page.
		panel?.focus();
	});

	// --- Title, inline ---------------------------------------------------------------

	let titleEditing = $state(false);
	let titleDraft = $state('');
	let titleEditorEl = $state<HTMLTextAreaElement>();

	function beginTitleEdit() {
		if (!task) return;
		titleDraft = title;
		titleError = null;
		titleEditing = true;
	}

	// Jira style: the pencil (or the text) drops straight into a focused, ready-to-type
	// field rather than making a person click again once the editor has mounted.
	$effect(() => {
		if (!titleEditing) return;
		const el = titleEditorEl;
		if (!el) return;
		el.focus();
		const end = el.value.length;
		el.setSelectionRange(end, end);
	});

	function cancelTitleEdit() {
		titleEditing = false;
		titleError = null;
	}

	async function saveTitle() {
		if (!task) return;
		const name = titleDraft.replace(/\r?\n/g, ' ').trim();
		if (!name) {
			titleError = 'Title cannot be empty';
			return;
		}
		titleError = null;
		if (name === title.trim()) {
			titleEditing = false;
			return;
		}
		try {
			await updateTask(task.key, { title: name });
			title = name;
			titleEditing = false;
			await onchanged();
			push('success', 'Title saved');
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	/** Enter saves; Shift+Enter is not a line break either, since a title has none.
	    `stopPropagation` keeps Escape from also reaching `onWindowKey`, which would
	    otherwise close the whole panel instead of just this editor. */
	function onTitleKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter') {
			event.preventDefault();
			event.stopPropagation();
			if (!event.shiftKey) void saveTitle();
		} else if (event.key === 'Escape') {
			event.preventDefault();
			event.stopPropagation();
			cancelTitleEdit();
		}
	}

	function onTitleBlur() {
		if (!titleEditing) return;
		if (titleDraft.replace(/\r?\n/g, ' ').trim() === title.trim()) {
			titleEditing = false;
			titleError = null;
			return;
		}
		void saveTitle();
	}

	// --- Description, inline ----------------------------------------------------------

	let descriptionEditing = $state(false);
	let descriptionDraft = $state('');
	let descriptionEditorEl = $state<HTMLTextAreaElement>();

	function beginDescriptionEdit() {
		if (!task) return;
		descriptionDraft = description;
		descriptionEditing = true;
	}

	/** A link inside the rendered Markdown opens (`MarkdownView`'s own click handler)
	    rather than also dropping the panel into edit mode. */
	function onDescriptionDisplayClick(event: MouseEvent) {
		if ((event.target as HTMLElement).closest('a[href]')) return;
		beginDescriptionEdit();
	}

	$effect(() => {
		if (!descriptionEditing) return;
		const el = descriptionEditorEl;
		if (!el) return;
		el.focus();
		const end = el.value.length;
		el.setSelectionRange(end, end);
	});

	function cancelDescriptionEdit() {
		descriptionEditing = false;
	}

	async function saveDescription() {
		if (!task) return;
		const next = descriptionDraft;
		if (next === description) {
			descriptionEditing = false;
			return;
		}
		try {
			await updateTask(task.key, { description: next });
			description = next;
			descriptionEditing = false;
			await onchanged();
			push('success', 'Description saved');
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	/** `stopPropagation` on Escape and Mod+Enter keeps them from also reaching
	    `onWindowKey`, the same reason `onTitleKeydown` does it. */
	function onDescriptionKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') {
			event.preventDefault();
			event.stopPropagation();
			cancelDescriptionEdit();
		} else if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
			event.preventDefault();
			event.stopPropagation();
			void saveDescription();
		}
	}

	/** Keeps a Save/Cancel button click from first blurring the field it belongs to. */
	function keepFocus(event: MouseEvent) {
		event.preventDefault();
	}

	function onEditableKeydown(begin: () => void) {
		return (event: KeyboardEvent) => {
			if (event.key === 'Enter' || event.key === ' ') {
				event.preventDefault();
				begin();
			}
		};
	}

	function onWindowKey(event: KeyboardEvent) {
		// The native <dialog> in modal mode handles its own Escape (close -> onModalClose
		// -> onclose), so this handler would otherwise call onclose() a second time.
		if (mode === 'modal') return;
		// The delete dialog is modal and closes itself on Escape.
		if (event.key !== 'Escape' || confirming) return;
		// Belt and braces alongside the inline editors' own `stopPropagation`: an
		// Escape that started inside one of them cancels that editor, not the panel.
		const target = event.target as HTMLElement | null;
		if (target?.closest('[data-testid="task-title"], [data-testid="task-description"]')) return;
		onclose();
	}

	const splitList = (text: string) =>
		text
			.split(',')
			.map((s) => s.trim())
			.filter(Boolean);

	/** Runs a write, reports the daemon's message on failure, and reloads on success. */
	async function run(what: string, action: () => Promise<unknown>) {
		busy = true;
		try {
			await action();
			await onchanged();
			push('success', what);
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			busy = false;
		}
	}

	async function save() {
		if (!task) return;
		const name = title.trim();
		if (!name) {
			titleError = 'A task needs a title.';
			return;
		}
		titleError = null;
		saving = true;
		try {
			await updateTask(task.key, {
				title: name,
				description,
				kind: kind as TaskKind,
				priority: priority as TaskPriority,
				// Null clears the assignee; the daemon leaves out fields alone.
				assignee: assignee.trim() || null,
				labels: splitList(labels),
				expected_updated_at: task.updated_at
			});
			await onchanged();
			push('success', 'Task saved');
		} catch (e) {
			// A stale stamp would refuse every retry, so take the fresh row. The reload
			// keeps this form's typing, because only untouched fields follow the server,
			// and the next Save carries the new `expected_updated_at`.
			if (e instanceof ConflictError) {
				await onchanged();
				push('error', CONFLICT_MESSAGE);
			} else {
				push('error', errorMessage(e));
			}
		} finally {
			saving = false;
		}
	}

	/** Writes the persona straight away; an empty value clears it. */
	async function changePersona(value: string) {
		if (!task) return;
		try {
			await updateTask(task.key, { persona: value });
			await onchanged();
		} catch (e) {
			persona = task.persona_slug ?? '';
			push('error', errorMessage(e));
		}
	}

	function addBlocker() {
		const key = blockerKey.trim();
		if (!task || !key) return;
		const next = [...task.blocked_by, key];
		blockerKey = '';
		void run('Blocker added', () => setBlockers(task.key, next));
	}

	function removeBlocker(key: string) {
		if (!task) return;
		const next = task.blocked_by.filter((k) => k !== key);
		void run('Blocker removed', () => setBlockers(task.key, next));
	}

	function sendComment() {
		const body = comment.trim();
		if (!task || !body) return;
		comment = '';
		void run('Comment added', () => postComment(task.key, body));
	}

	let reimporting = $state(false);

	/** Re-runs the task import for this task's source framework, so a source file edited
	 * since the last import is picked up. Toasts the report rather than `run`'s generic
	 * message, since "3 created, 1 updated, 12 skipped" says more than "Re-imported". */
	async function reimport() {
		if (!task?.source_ref || !task.project_id) return;
		const { project_id, source_ref } = task;
		reimporting = true;
		try {
			const report = await importFramework(project_id, source_ref.framework, 'tasks');
			await onchanged();
			push('success', reportText(report));
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			reimporting = false;
		}
	}

	async function confirmDelete() {
		if (!task) return;
		// `task` is a live read of `detail`, and `ondeleted` closes the panel, so the
		// key has to be in hand before the first await or the toast reads it as null.
		const key = task.key;
		busy = true;
		try {
			await removeTask(key);
			confirming = false;
			await ondeleted();
			push('success', `Deleted ${key}`);
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			busy = false;
		}
	}

	/** The drag paints the node's own CSS variable; the store hears the final width on release. */
	function paint(live: number) {
		panel?.style.setProperty('--detail-w', `${live}px`);
	}

	let modal = $state<HTMLDialogElement>();
	/** True while the dialog is being torn down by a mode switch, so its `close` event
	    is not read as the user dismissing the task: the task stays selected and docks. */
	let switching = false;

	// The modal opens as soon as it is in the document and closes with the mode switch.
	$effect(() => {
		const el = modal;
		if (!el) return;
		switching = false;
		if (!el.open) el.showModal();
		return () => {
			switching = true;
			if (el.open) el.close();
		};
	});

	function onModalClose() {
		if (!switching) onclose();
	}
</script>

<svelte:window onkeydown={onWindowKey} />

{#snippet eventRow(event: TaskEvent)}
	<li>
		<div class="who">
			<span class="actor">{event.actor}</span>
			<Badge variant="outline">{event.kind}</Badge>
			<span class="spacer"></span>
			<span class="when">{relativeAge(event.created_at)}</span>
		</div>
		<span class="what">{event.body}</span>
	</li>
{/snippet}

{#snippet body()}
<aside
	bind:this={panel}
	class="detail"
	style="--detail-w:{width}px"
	data-testid="task-detail"
	aria-label="Task detail"
	tabindex="-1"
>
	{#if mode === 'docked'}
		<ResizeBar
			side="left"
			label="Resize task detail"
			value={width}
			min={DETAIL_MIN}
			max={DETAIL_MAX}
			gap={12}
			onlive={paint}
			{onresize}
			testid="task-detail-resize"
		/>
	{/if}

	<header>
		{#if backKey}
			<IconButton
				size="sm"
				icon="arrow-left"
				label="Back to {backKey}"
				data-testid="task-detail-back"
				onclick={() => onback?.()}
			/>
		{/if}
		<span class="key">{task?.key ?? ''}</span>
		{#if task}
			<IconButton
				size="sm"
				icon={keyCopied ? 'check' : 'copy'}
				label="Copy task key"
				onclick={copyKey}
			/>
		{/if}
		<!-- `ready` is false for three different reasons, and calling all of them `blocked`
		     said the wrong thing about a parent whose only holdup is its own children, and
		     about a task that is simply closed. Blockers first, then open subtasks, then
		     nothing at all. -->
		{#if task && !task.ready && task.open_blockers > 0}
			<Badge tone="danger" title={task.blocked_reason ?? 'Not ready'}>blocked</Badge>
		{:else if task && !task.ready && task.subtasks_total > task.subtasks_done}
			<Badge tone="warning" title={task.blocked_reason ?? 'Not ready'}>
				waiting on subtasks
			</Badge>
		{/if}
		<span class="spacer"></span>
		<IconButton
			size="sm"
			icon={mode === 'docked' ? 'external-link' : 'panel-right'}
			label={mode === 'docked' ? 'Open as a dialog' : 'Dock beside the board'}
			data-testid="task-detail-mode"
			onclick={ontogglemode}
		/>
		<IconButton size="sm" icon="x" label="Close task" onclick={onclose} />
	</header>

	<div class="body">
		{#if error}
			<p class="bad" role="alert">{error}</p>
		{:else if !task}
			<p class="muted">{loading ? 'Loading…' : 'No task open.'}</p>
		{:else}
			{#if task.parent_key}
				<!-- A subtask names its parent above its title, the way Jira draws the
				     breadcrumb; the line opens the parent in this same panel. -->
				<button
					type="button"
					class="parent"
					data-testid="task-detail-parent"
					title="Subtask of {task.parent_key}"
					onclick={() => {
						if (task?.parent_key) onopen?.(task.parent_key);
					}}
				>
					<Icon name="corner-down-right" size={12} />
					<code>{task.parent_key}</code>
					{#if task.parent_title}<span class="parent-title">{task.parent_title}</span>{/if}
				</button>
			{/if}
			<div class="field">
				{#if titleEditing}
					<textarea
						bind:this={titleEditorEl}
						bind:value={titleDraft}
						use:autogrow
						rows="1"
						class="title-input"
						aria-label="Title"
						data-testid="task-title"
						onkeydown={onTitleKeydown}
						onblur={onTitleBlur}
					></textarea>
					{#if titleError}
						<p class="bad" role="alert">{titleError}</p>
					{/if}
					<div class="row">
						<IconButton
							size="sm"
							icon="check"
							label="Save title"
							data-testid="task-title-save"
							onmousedown={keepFocus}
							onclick={saveTitle}
						/>
						<IconButton
							size="sm"
							icon="x"
							label="Cancel"
							data-testid="task-title-cancel"
							onmousedown={keepFocus}
							onclick={cancelTitleEdit}
						/>
					</div>
				{:else}
					<div class="title-display">
						<button
							type="button"
							class="title-hit"
							data-testid="task-title-text"
							onclick={beginTitleEdit}
						>
							<h2 class="title-text">{title}</h2>
						</button>
						<IconButton
							size="sm"
							icon="pencil"
							label="Edit title"
							data-testid="task-title-edit"
							onclick={beginTitleEdit}
						/>
					</div>
				{/if}
			</div>

			<div class="field">
				<div class="field-head">
					<span>Description</span>
					{#if !descriptionEditing}
						<IconButton
							size="sm"
							icon="pencil"
							label="Edit description"
							data-testid="task-description-edit"
							onclick={beginDescriptionEdit}
						/>
					{/if}
				</div>
				{#if descriptionEditing}
					<textarea
						bind:this={descriptionEditorEl}
						bind:value={descriptionDraft}
						use:autogrow
						class="area mono-hint description-editor"
						rows="3"
						placeholder="No description"
						data-testid="task-description"
						onkeydown={onDescriptionKeydown}
					></textarea>
					<div class="row">
						<Button
							size="sm"
							data-testid="task-description-save"
							onmousedown={keepFocus}
							onclick={saveDescription}
						>
							Save
						</Button>
						<Button
							size="sm"
							data-testid="task-description-cancel"
							onmousedown={keepFocus}
							onclick={cancelDescriptionEdit}
						>
							Cancel
						</Button>
					</div>
				{:else}
					<div
						class="description-display"
						role="button"
						tabindex="0"
						data-testid="task-description-text"
						onclick={onDescriptionDisplayClick}
						onkeydown={onEditableKeydown(beginDescriptionEdit)}
					>
						<MarkdownView source={description} showHeader={false} emptyText="No description" />
					</div>
				{/if}
			</div>

			<div class="pair">
				<Select label="Kind" bind:value={kind} options={kindOptions} data-testid="task-kind" />
				<Select
					label="Priority"
					bind:value={priority}
					options={priorityOptions}
					data-testid="task-priority"
				/>
				<Select
					label="Persona"
					bind:value={persona}
					options={personaOptions}
					data-testid="task-persona"
					onchange={(e: Event & { currentTarget: HTMLSelectElement }) =>
						void changePersona(e.currentTarget.value)}
				/>
			</div>

			<Input label="Assignee" mono bind:value={assignee} placeholder="nobody" data-testid="task-assignee" />
			<Input label="Labels" bind:value={labels} placeholder="api, ui" data-testid="task-labels" />

			<div class="row">
				<Button
					variant="primary"
					size="sm"
					data-testid="task-save"
					disabled={saving}
					onclick={save}
				>
					{saving ? 'Saving…' : 'Save'}
				</Button>
				<Button
					size="sm"
					data-testid="task-claim"
					disabled={busy}
					onclick={() => run('Task claimed', () => claim(task.key))}
				>
					Claim
				</Button>
				<span class="spacer"></span>
				<Button
					variant="danger"
					size="sm"
					data-testid="task-delete"
					onclick={() => (confirming = true)}
				>
					Delete…
				</Button>
			</div>

			<Select
				label="Move to"
				value={task.stage}
				options={stageOptions}
				data-testid="task-stage"
				onchange={(e: Event & { currentTarget: HTMLSelectElement }) =>
					onmove(task.key, e.currentTarget.value)}
			/>

			{#if task.source_ref}
				<section>
					<h3>Source</h3>
					<div class="row">
						<Badge tone="accent" mono>{FRAMEWORK_LABEL[task.source_ref.framework]}</Badge>
						<span class="mono source-path" data-testid="task-source-path">
							{task.source_ref.path}{task.source_ref.anchor ? `#${task.source_ref.anchor}` : ''}
						</span>
					</div>
					<div class="row">
						<Button size="sm" data-testid="task-reimport" disabled={reimporting} onclick={reimport}>
							{reimporting ? 'Re-importing…' : 'Re-import'}
						</Button>
					</div>
				</section>
			{/if}

			<section>
				<h3>Blocked by</h3>
				{#if task.blocked_by.length === 0}
					<p class="muted">Nothing is holding this up.</p>
				{:else}
					<ul class="chips">
						{#each task.blocked_by as key (key)}
							<li>
								<code>{key}</code>
								<IconButton
									size="sm"
									icon="x"
									label="Remove blocker {key}"
									disabled={busy}
									onclick={() => removeBlocker(key)}
								/>
							</li>
						{/each}
					</ul>
				{/if}
				<div class="row end">
					<div class="grow">
						<Input
							mono
							bind:value={blockerKey}
							placeholder="ATL-12"
							aria-label="Blocker key"
							data-testid="task-blocker-key"
						/>
					</div>
					<Button data-testid="task-blocker-add" disabled={busy} onclick={addBlocker}>Add</Button>
				</div>
			</section>

			<section class="tabs" data-testid="task-tabs">
				<TabStrip
					items={tabs}
					active={detailTab()}
					onselect={(id) => setDetailTab(id as DetailTab)}
					testid="task-tab"
				/>

				{#if detailTab() === 'subtasks'}
					{#if detail && detail.children.length > 0}
						<ul class="list" data-testid="task-children">
							{#each detail.children as child (child.id)}
								<li>
									<button
										type="button"
										class="child-row"
										onclick={() => onopen?.(child.key)}
									>
										<code>{child.key}</code>
										<span>{child.title}</span>
										<Badge variant="outline">{child.stage}</Badge>
									</button>
								</li>
							{/each}
						</ul>
					{:else}
						<p class="muted">No subtasks.</p>
					{/if}
				{:else if detailTab() === 'activity'}
					{#if activityEvents.length > 0}
						<ul class="events" data-testid="task-events">
							{#each activityEvents as event (event.id)}
								{@render eventRow(event)}
							{/each}
						</ul>
					{:else}
						<p class="muted">Nothing has happened yet.</p>
					{/if}
				{:else}
					{#if commentEvents.length > 0}
						<ul class="events" data-testid="task-events">
							{#each commentEvents as event (event.id)}
								{@render eventRow(event)}
							{/each}
						</ul>
					{:else}
						<p class="muted">No comments yet.</p>
					{/if}
					<textarea
						bind:value={comment}
						class="area"
						rows="2"
						aria-label="Comment"
						placeholder="Add a comment"
						data-testid="task-comment"
					></textarea>
					<div class="row">
						<Button size="sm" data-testid="task-comment-send" disabled={busy} onclick={sendComment}>
							Comment
						</Button>
					</div>
				{/if}
			</section>

			{#each pluginPanels as panel (`${panel.pluginId}:${panel.id}`)}
				{@const plugin = pluginById(panel.pluginId)}
				{#if plugin}
					<section class="plugin-panel" data-testid="plugin-panel-{panel.pluginId}-{panel.id}">
						<h3>{plugin.manifest?.name ?? plugin.id}</h3>
						<PluginFrame
							{plugin}
							view={panel.view}
							slot="task.detail.panel"
							maxHeight={PLUGIN_PANEL_MAX_HEIGHT}
							context={pluginContext}
						/>
					</section>
				{/if}
			{/each}
		{/if}
	</div>
</aside>
{/snippet}

{#if mode === 'modal'}
	<!-- A bare native dialog: the panel keeps its own chrome and close button, and the
	     element only supplies the backdrop and the modal focus trap. -->
	<dialog
		bind:this={modal}
		class="detail-modal"
		data-testid="task-detail-modal"
		onclose={onModalClose}
		onclick={(e) => {
			if (e.target === modal) onclose();
		}}
	>
		{@render body()}
	</dialog>
{:else}
	{@render body()}
{/if}

<Dialog open={confirming} title="Delete this task?" onclose={() => (confirming = false)}>
	<p class="prose">
		<strong>{task?.key ?? 'This task'}</strong> and its history are removed for good. Its
		subtasks are kept and lose their parent.
	</p>
	{#snippet footer()}
		<Button onclick={() => (confirming = false)}>Cancel</Button>
		<Button
			variant="danger"
			data-testid="task-delete-confirm"
			disabled={busy}
			onclick={confirmDelete}
		>
			{busy ? 'Deleting…' : 'Delete'}
		</Button>
	{/snippet}
</Dialog>

<style>
	/* Docked: a flex sibling of the lane strip, so the strip gives up the room rather
	   than being drawn over. */
	.detail {
		position: relative;
		flex: 0 0 var(--detail-w);
		width: var(--detail-w);
		min-height: 0;
		align-self: stretch;
		display: flex;
		flex-direction: column;
		background: var(--bg-raised);
		border: 1px solid var(--border-default);
		border-radius: 5px;
		/* Visible, not hidden: the resize bar hangs 11px outside the left edge, in the
		   gap, and a hidden overflow would clip it away. The body scrolls on its own. */
		overflow: visible;
	}

	.detail:focus-visible {
		outline: var(--focus-ring-width) solid var(--focus-ring);
		outline-offset: -2px;
	}


	header {
		display: flex;
		align-items: center;
		gap: 6px;
		height: 32px;
		flex: 0 0 32px;
		padding: 0 6px 0 10px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.key {
		font-family: var(--font-mono);
		font-weight: 700;
	}

	.spacer {
		flex: 1;
	}

	.parent {
		display: flex;
		align-items: center;
		gap: 6px;
		min-width: 0;
		padding: 0;
		border: 0;
		background: none;
		color: var(--text-tertiary);
		font: inherit;
		font-size: 11px;
		cursor: pointer;
		text-align: left;
	}

	.parent code {
		font-family: var(--font-mono);
		flex: 0 0 auto;
	}

	.parent:hover {
		color: var(--text-secondary);
	}

	.parent:focus-visible {
		outline: var(--focus-ring-width) solid var(--focus-ring);
		outline-offset: 1px;
	}

	.parent-title {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.body {
		flex: 1;
		min-height: 0;
		overflow: auto;
		padding: 10px;
		display: flex;
		flex-direction: column;
		gap: 10px;
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

	.field-head > span {
		font-size: 12px;
		color: var(--text-secondary);
	}

	.area {
		resize: none;
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

	.mono-hint::placeholder {
		font-family: var(--font-mono);
		color: var(--text-tertiary);
	}

	/* `.area` is shared with the plain comment textarea, which is not auto-growing and
	   still wants its own scrollbar; only the description's editor grows in place and
	   needs its scrollbar hidden, so that rule lives on its own dedicated class rather
	   than on the shared one. */
	.description-editor {
		overflow: hidden;
	}

	/* Title: wrapped text in place, Jira style, switching to an auto-growing single
	   field on click. Both states share the heading's size so the swap does not jump. */
	.title-display {
		display: flex;
		align-items: flex-start;
		gap: 6px;
	}

	.title-hit {
		flex: 1;
		min-width: 0;
		padding: 0;
		background: none;
		border: 0;
		font: inherit;
		text-align: left;
		color: inherit;
		cursor: pointer;
		border-radius: 3px;
	}

	.title-hit:focus-visible {
		outline: var(--focus-ring-width) solid var(--focus-ring);
		outline-offset: 2px;
	}

	.title-text {
		margin: 0;
		/* Lines up with the description box's own text, inset by its padding below,
		   and with the Assignee/Labels inputs, whose text sits the same 8px in. */
		padding-left: 8px;
		font-size: 15px;
		font-weight: 600;
		line-height: 1.3;
		overflow-wrap: anywhere;
	}

	.title-input {
		width: 100%;
		resize: none;
		overflow: hidden;
		background: var(--bg-base);
		border: 1px solid var(--accent);
		border-radius: 3px;
		color: var(--text-primary);
		font-family: var(--font-ui);
		font-size: 15px;
		font-weight: 600;
		line-height: 1.3;
		padding: 5px 7px;
		outline: var(--focus-ring-width) solid var(--focus-ring);
		outline-offset: 0;
	}

	/* A little extra room above the Description label, on top of the `.body`'s own
	   10px gap between fields, so the title block does not read as glued to it. */
	.field + .field {
		margin-top: 4px;
	}

	/* Description: the rendered Markdown wraps in place with no cap on its height and
	   no scrollbar of its own, so the panel as a whole scrolls instead. MarkdownView's
	   own embedded styling caps its height for the smaller previews it is normally
	   used in, which is exactly what this view does not want. */
	.description-display {
		cursor: pointer;
		border-radius: 3px;
	}

	.description-display:focus-visible {
		outline: var(--focus-ring-width) solid var(--focus-ring);
		outline-offset: 2px;
	}

	/* `.body.embedded`, not just `.body`: MarkdownView's own `.body.embedded` rule
	   (its 240px cap and scroll) is the same two-class specificity, so which one wins
	   would otherwise depend on Svelte's component style-injection order rather than
	   anything pinned down here. The extra class makes this selector strictly more
	   specific, so it wins regardless of that order. */
	.description-display :global(.body.embedded) {
		/* Matches `.area`, the textarea this display replaces, so the text does not
		   sit flush against the box's own border on every side. */
		padding: 6px 8px;
		flex: none;
		max-height: none;
		overflow: visible;
	}

	/* MarkdownView already zeroes a paragraph's bottom margin as the last child; the
	   top margin on the first one is left alone there since embedded use elsewhere
	   still wants it. Here it would only double the box's own top padding. */
	.description-display :global(.body p:first-child) {
		margin-top: 0;
	}

	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 8px;
	}

	.row {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.row.end {
		align-items: flex-end;
		gap: 6px;
	}

	.source-path {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--text-secondary);
	}

	.grow {
		flex: 1;
	}

	section {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	h3 {
		margin: 0;
		font-size: 11px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--text-tertiary);
	}

	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		margin: 0;
		padding: 0;
		list-style: none;
	}

	.chips li {
		display: flex;
		align-items: center;
		gap: 2px;
		padding: 0 2px 0 8px;
		border: 1px solid var(--border-default);
		border-radius: 999px;
		font-size: 12px;
	}

	.list,
	.events {
		display: flex;
		flex-direction: column;
		gap: 6px;
		margin: 0;
		padding: 0;
		list-style: none;
		font-size: 12px;
	}

	.child-row {
		display: flex;
		align-items: center;
		gap: 6px;
		width: 100%;
		padding: 0;
		background: none;
		border: 0;
		font: inherit;
		color: inherit;
		text-align: left;
		cursor: pointer;
	}

	.events li {
		display: flex;
		flex-direction: column;
		gap: 2px;
		padding-bottom: 6px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.who {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	.actor {
		font-family: var(--font-mono);
		font-size: 11px;
		font-weight: 700;
	}

	.when {
		font-family: var(--font-mono);
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.what {
		color: var(--text-secondary);
		line-height: 15px;
		overflow-wrap: anywhere;
	}

	.muted {
		margin: 0;
		color: var(--text-tertiary);
	}

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 12px;
		overflow-wrap: anywhere;
	}

	.prose {
		margin: 0;
		max-width: 60ch;
	}

	/* As a dialog the same panel floats over the board at a reading width. The dialog
	   element itself is invisible: no frame, no padding, only the backdrop. */
	.detail-modal {
		padding: 0;
		border: 0;
		background: transparent;
		overflow: visible;
	}

	.detail-modal::backdrop {
		background: rgb(0 0 0 / 0.45);
	}

	.detail-modal .detail {
		position: static;
		flex: none;
		width: min(820px, 92vw);
		height: min(80vh, 920px);
		overflow: hidden;
	}

	/* The dialog hands the panel focus on open; the ring that marks that in the dock is
	   noise on a floating panel that is already the only thing in reach. */
	.detail-modal .detail:focus-visible {
		outline: none;
	}
</style>
