<script lang="ts">
	// The open task: its fields, its blockers and children, its history, and the
	// writes a person can make from here. Every write goes straight to the daemon and
	// then asks the board to reload, so the drawer holds no state the server does not.
	import { onMount, untrack } from 'svelte';
	import { ApiError } from '$lib/api';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { relativeAge } from '$lib/format';
	import { CONFLICT_MESSAGE } from '$lib/stores/board.svelte';
	import type { Stage, TaskDetail, TaskKind, TaskPriority } from '$lib/types';
	import Badge from '$lib/ui/Badge.svelte';
	import Button from '$lib/ui/Button.svelte';
	import Dialog from '$lib/ui/Dialog.svelte';
	import Input from '$lib/ui/Input.svelte';
	import Select from '$lib/ui/Select.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	interface Props {
		detail: TaskDetail | null;
		stages: Stage[];
		loading: boolean;
		error: string | null;
		onclose: () => void;
		/** Called after any write, so the board and this drawer both reload. */
		onchanged: () => void | Promise<void>;
		/** Moves go through the board so the card moves at the same moment. */
		onmove: (key: string, stage: string) => void;
		ondeleted: () => void | Promise<void>;
	}

	let { detail, stages, loading, error, onclose, onchanged, onmove, ondeleted }: Props = $props();

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

	// Every write in this drawer reloads the task, so refilling the whole form on each
	// reload would throw away unsaved typing the moment someone clicked Claim. A field
	// the user has not touched follows the server; one they have keeps their text. A
	// different task refills everything, since none of that typing belongs to it.
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
		// The drawer is what the click opened, so the keyboard starts here rather than
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

	async function confirmDelete() {
		if (!task) return;
		// `task` is a live read of `detail`, and `ondeleted` closes the drawer, so the
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
</script>

<svelte:window onkeydown={onWindowKey} />

<aside
	bind:this={panel}
	class="drawer"
	data-testid="task-drawer"
	tabindex="-1"
>
	<header>
		<div class="head">
			<code>{task?.key ?? ''}</code>
			{#if task && !task.ready}
				<Badge tone="danger" title={task.blocked_reason ?? 'Not ready'}>blocked</Badge>
			{/if}
		</div>
		<button type="button" class="x" aria-label="Close task" onclick={onclose}>×</button>
	</header>

	{#if error}
		<p class="bad" role="alert">{error}</p>
	{:else if !task}
		<p class="muted">{loading ? 'Loading…' : 'No task open.'}</p>
	{:else}
		<div class="fields">
			<label class="field">
				<span>Title</span>
				<Input bind:value={title} data-testid="task-title" />
				{#if titleError}
					<span class="bad" role="alert" data-testid="task-title-error">{titleError}</span>
				{/if}
			</label>

			<label class="field">
				<span>Description</span>
				<Textarea bind:value={description} rows={4} data-testid="task-description" />
			</label>

			<div class="pair">
				<label class="field">
					<span>Kind</span>
					<Select bind:value={kind} options={kindOptions} data-testid="task-kind" />
				</label>
				<label class="field">
					<span>Priority</span>
					<Select bind:value={priority} options={priorityOptions} data-testid="task-priority" />
				</label>
			</div>

			<label class="field">
				<span>Assignee</span>
				<Input bind:value={assignee} data-testid="task-assignee" placeholder="nobody" />
			</label>

			<label class="field">
				<span>Labels</span>
				<Input bind:value={labels} data-testid="task-labels" placeholder="api, ui" />
			</label>

			<div class="row">
				<Button variant="primary" data-testid="task-save" disabled={saving} onclick={save}>
					{saving ? 'Saving…' : 'Save'}
				</Button>
				<Button
					data-testid="task-claim"
					disabled={busy}
					onclick={() => run('Task claimed', () => api().claimTask(task.key))}
				>
					Claim
				</Button>
			</div>

			<label class="field">
				<span>Move</span>
				<Select
					value={task.stage}
					options={stageOptions}
					data-testid="task-stage"
					onchange={(e: Event & { currentTarget: HTMLSelectElement }) =>
						onmove(task.key, e.currentTarget.value)}
				/>
			</label>
		</div>

		<section>
			<h3>Blocked by</h3>
			{#if task.blocked_by.length === 0}
				<p class="muted">Nothing is holding this up.</p>
			{:else}
				<ul class="chips">
					{#each task.blocked_by as key (key)}
						<li>
							<code>{key}</code>
							<button
								type="button"
								class="x small"
								aria-label="Remove blocker {key}"
								disabled={busy}
								onclick={() => removeBlocker(key)}>×</button
							>
						</li>
					{/each}
				</ul>
			{/if}
			<div class="row">
				<Input
					bind:value={blockerKey}
					data-testid="task-blocker-key"
					placeholder="ATL-12"
					aria-label="Blocker key"
				/>
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
							<span class="actor">{event.actor}</span>
							<Badge>{event.kind}</Badge>
							<span class="body">{event.body}</span>
							<span class="when">{relativeAge(event.created_at)}</span>
						</li>
					{/each}
				</ul>
			{:else}
				<p class="muted">Nothing has happened yet.</p>
			{/if}
			<Textarea
				bind:value={comment}
				rows={2}
				data-testid="task-comment"
				aria-label="Comment"
				placeholder="Add a comment"
			/>
			<div class="row">
				<Button data-testid="task-comment-send" disabled={busy} onclick={sendComment}>
					Comment
				</Button>
			</div>
		</section>

		<div class="row">
			<Button variant="danger" data-testid="task-delete" onclick={() => (confirming = true)}>
				Delete
			</Button>
		</div>
	{/if}
</aside>

<Dialog open={confirming} title="Delete this task?" onclose={() => (confirming = false)}>
	<p class="prose">
		<strong>{task?.key ?? 'This task'}</strong> and its history are removed for good. Its
		subtasks are kept and lose their parent.
	</p>
	{#snippet footer()}
		<Button onclick={() => (confirming = false)}>Cancel</Button>
		<Button variant="danger" data-testid="task-delete-confirm" disabled={busy} onclick={confirmDelete}>
			{busy ? 'Deleting…' : 'Delete'}
		</Button>
	{/snippet}
</Dialog>

<style>
	.drawer {
		display: flex;
		flex-direction: column;
		gap: var(--space-4);
		padding: var(--space-4);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--bg-elev);
	}

	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-2);
	}

	.head {
		display: flex;
		align-items: center;
		gap: var(--space-2);
	}

	.x {
		border: none;
		background: none;
		color: var(--muted);
		font-size: 20px;
		line-height: 1;
		cursor: pointer;
	}

	.x.small {
		font-size: 14px;
	}

	.x:hover:not(:disabled) {
		color: var(--fg);
	}

	.fields,
	section {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
	}

	.field > span {
		font-size: 13px;
		color: var(--muted);
	}

	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: var(--space-2);
	}

	.row {
		display: flex;
		align-items: center;
		gap: var(--space-2);
	}

	h3 {
		margin: 0;
		font-size: 13px;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--muted);
	}

	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-2);
		margin: 0;
		padding: 0;
		list-style: none;
	}

	.chips li {
		display: flex;
		align-items: center;
		gap: var(--space-1);
		padding: 1px 4px 1px 8px;
		border: 1px solid var(--border);
		border-radius: 999px;
		font-size: 12px;
	}

	.list,
	.events {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		margin: 0;
		padding: 0;
		list-style: none;
		font-size: 13px;
	}

	.events li {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--space-2);
	}

	.actor {
		font-weight: 500;
	}

	.body {
		color: var(--muted);
		overflow-wrap: anywhere;
	}

	.when {
		margin-left: auto;
		color: var(--muted);
	}

	.muted {
		margin: 0;
		color: var(--muted);
	}

	.bad {
		margin: 0;
		color: var(--danger);
		font-size: 13px;
		overflow-wrap: anywhere;
	}

	.drawer:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: 2px;
	}

	.prose {
		margin: 0;
		max-width: 60ch;
	}
</style>
