<script lang="ts">
	// The open task, docked at the right of the lane strip per frame 02.2b: its fields,
	// its blockers and children, its history, and the writes a person can make from here.
	// Every write goes straight to the daemon and then asks the board to reload, so this
	// panel holds no state the server does not.
	//
	// The form logic is the Phase 4 drawer's, moved rather than rewritten: the dirty-aware
	// refill, the 409 reload, the delete confirmation and the toasts all behave as before.
	import { onMount, untrack } from 'svelte';
	import { ApiError } from '$lib/api';
	import { api } from '$lib/daemon.svelte';
	import { Badge, Button, IconButton, Input, Select } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { relativeAge } from '$lib/format';
	import { copyText } from '$lib/shell';
	import {
		clampDetail,
		CONFLICT_MESSAGE,
		DETAIL_MAX,
		DETAIL_MIN
	} from '$lib/stores/board.svelte';
	import { FRAMEWORK_LABEL, reportText } from '$lib/components/project/frameworks';
	import type { Stage, TaskDetail, TaskKind, TaskPriority } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	interface Props {
		detail: TaskDetail | null;
		stages: Stage[];
		loading: boolean;
		error: string | null;
		width: number;
		onclose: () => void;
		/** Called after any write, so the board and this panel both reload. */
		onchanged: () => void | Promise<void>;
		/** Moves go through the board so the card moves at the same moment. */
		onmove: (key: string, stage: string) => void;
		ondeleted: () => void | Promise<void>;
		onresize: (width: number) => void;
	}

	let {
		detail,
		stages,
		loading,
		error,
		width,
		onclose,
		onchanged,
		onmove,
		ondeleted,
		onresize
	}: Props = $props();

	const KINDS: TaskKind[] = ['task', 'bug', 'feature', 'chore'];
	const PRIORITIES: TaskPriority[] = ['low', 'medium', 'high', 'urgent'];

	const kindOptions = KINDS.map((k) => ({ value: k, label: k }));
	const priorityOptions = PRIORITIES.map((p) => ({ value: p, label: p }));
	const stageOptions = $derived(stages.map((s) => ({ value: s.name, label: s.name })));

	const task = $derived(detail?.task ?? null);

	let title = $state('');
	let description = $state('');
	// Held as plain strings because `Select` binds a string; the values can only be
	// the options above, so the cast on save is safe.
	let kind = $state('task');
	let priority = $state('medium');
	let assignee = $state('');
	let labels = $state('');
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
			if (other) titleError = null;
			base = next;
		});
	});

	onMount(() => {
		// The panel is what the click opened, so the keyboard starts here rather than
		// back at the top of the page.
		panel?.focus();
	});

	function onWindowKey(event: KeyboardEvent) {
		// The delete dialog is modal and closes itself on Escape.
		if (event.key !== 'Escape' || confirming) return;
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
			await api().updateTask(task.key, {
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
			if (e instanceof ApiError && e.status === 409) {
				await onchanged();
				push('error', CONFLICT_MESSAGE);
			} else {
				push('error', errorMessage(e));
			}
		} finally {
			saving = false;
		}
	}

	function addBlocker() {
		const key = blockerKey.trim();
		if (!task || !key) return;
		const next = [...task.blocked_by, key];
		blockerKey = '';
		void run('Blocker added', () => api().setTaskBlockers(task.key, next));
	}

	function removeBlocker(key: string) {
		if (!task) return;
		const next = task.blocked_by.filter((k) => k !== key);
		void run('Blocker removed', () => api().setTaskBlockers(task.key, next));
	}

	function sendComment() {
		const body = comment.trim();
		if (!task || !body) return;
		comment = '';
		void run('Comment added', () => api().commentTask(task.key, body));
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
			const report = await api().importFramework(project_id, source_ref.framework, 'tasks');
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
			await api().deleteTask(key);
			confirming = false;
			await ondeleted();
			push('success', `Deleted ${key}`);
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			busy = false;
		}
	}

	// The left edge drags the panel wider. As with the lanes, the pointer writes to the
	// node's own CSS variable and the store hears about it once, on release.
	let dragging = false;
	let startX = 0;
	let startWidth = 0;
	let live = 0;

	function grab(event: PointerEvent) {
		(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
		dragging = true;
		startX = event.clientX;
		startWidth = width;
		live = width;
		event.preventDefault();
	}

	/**
	 * The dock above this panel reads the same variable to reserve room on the lane strip,
	 * so the live width has to reach it too; otherwise the strip's tail would only catch up
	 * when the drag ended.
	 */
	function setLive(width: number) {
		panel?.style.setProperty('--detail-w', `${width}px`);
		panel?.parentElement?.style.setProperty('--detail-w', `${width}px`);
	}

	function drag(event: PointerEvent) {
		if (!dragging) return;
		live = clampDetail(startWidth - (event.clientX - startX));
		setLive(live);
	}

	function drop(event: PointerEvent) {
		if (!dragging) return;
		dragging = false;
		(event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
		onresize(live);
	}

	/** An interrupted gesture is not a decision; the panel goes back where it started. */
	function cancel(event: PointerEvent) {
		if (!dragging) return;
		dragging = false;
		(event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
		setLive(startWidth);
	}

	function nudge(event: KeyboardEvent) {
		const step = event.key === 'ArrowLeft' ? 20 : event.key === 'ArrowRight' ? -20 : 0;
		if (step === 0) return;
		event.preventDefault();
		onresize(width + step);
	}
</script>

<svelte:window onkeydown={onWindowKey} />

<aside
	bind:this={panel}
	class="detail"
	style="--detail-w:{width}px"
	data-testid="task-detail"
	aria-label="Task detail"
	tabindex="-1"
>
	<!-- As in the lane: a splitter is focusable, and the checker reads the role as not. -->
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_no_noninteractive_tabindex -->
	<div
		class="handle"
		role="separator"
		aria-orientation="vertical"
		aria-label="Resize task detail"
		aria-valuenow={width}
		aria-valuemin={DETAIL_MIN}
		aria-valuemax={DETAIL_MAX}
		tabindex="0"
		onpointerdown={grab}
		onpointermove={drag}
		onpointerup={drop}
		onpointercancel={cancel}
		onkeydown={nudge}
	></div>

	<header>
		<span class="key">{task?.key ?? ''}</span>
		{#if task}
			<IconButton
				size="sm"
				icon={keyCopied ? 'check' : 'copy'}
				label="Copy task key"
				onclick={copyKey}
			/>
		{/if}
		{#if task && !task.ready}
			<Badge tone="danger" title={task.blocked_reason ?? 'Not ready'}>blocked</Badge>
		{/if}
		<span class="spacer"></span>
		<IconButton size="sm" icon="x" label="Close task" onclick={onclose} />
	</header>

	<div class="body">
		{#if error}
			<p class="bad" role="alert">{error}</p>
		{:else if !task}
			<p class="muted">{loading ? 'Loading…' : 'No task open.'}</p>
		{:else}
			<Input
				label="Title"
				bind:value={title}
				error={titleError ?? undefined}
				data-testid="task-title"
			/>

			<label class="field">
				<span>Description</span>
				<textarea
					bind:value={description}
					class="area mono-hint"
					rows="3"
					placeholder="No description"
					data-testid="task-description"
				></textarea>
			</label>

			<div class="pair">
				<Select label="Kind" bind:value={kind} options={kindOptions} data-testid="task-kind" />
				<Select
					label="Priority"
					bind:value={priority}
					options={priorityOptions}
					data-testid="task-priority"
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
					onclick={() => run('Task claimed', () => api().claimTask(task.key))}
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

			<section>
				<h3>Subtasks</h3>
				{#if detail && detail.children.length > 0}
					<ul class="list" data-testid="task-children">
						{#each detail.children as child (child.id)}
							<li><code>{child.key}</code> <span>{child.title}</span></li>
						{/each}
					</ul>
				{:else}
					<p class="muted">No subtasks.</p>
				{/if}
			</section>

			<section>
				<h3>Activity</h3>
				{#if detail && detail.events.length > 0}
					<ul class="events" data-testid="task-events">
						{#each detail.events as event (event.id)}
							<li>
								<div class="who">
									<span class="actor">{event.actor}</span>
									<Badge variant="outline">{event.kind}</Badge>
									<span class="spacer"></span>
									<span class="when">{relativeAge(event.created_at)}</span>
								</div>
								<span class="what">{event.body}</span>
							</li>
						{/each}
					</ul>
				{:else}
					<p class="muted">Nothing has happened yet.</p>
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
			</section>
		{/if}
	</div>
</aside>

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
	/* Docked over the strip rather than beside it, so the lanes keep scrolling beneath. */
	.detail {
		position: absolute;
		top: 0;
		right: 0;
		bottom: 0;
		width: var(--detail-w);
		display: flex;
		flex-direction: column;
		background: var(--bg-raised);
		border: 1px solid var(--border-default);
		border-radius: 5px;
		box-shadow: var(--shadow-md);
		overflow: hidden;
	}

	.detail:focus-visible {
		outline: var(--focus-ring-width) solid var(--focus-ring);
		outline-offset: -2px;
	}

	.handle {
		position: absolute;
		top: 0;
		bottom: 0;
		left: -3px;
		width: 6px;
		cursor: col-resize;
		border-radius: 2px;
		touch-action: none;
		z-index: 1;
	}

	.handle:hover,
	.handle:focus-visible {
		background: var(--border-strong);
		outline: none;
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

	.field > span {
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
</style>
