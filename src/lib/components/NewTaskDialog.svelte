<script lang="ts">
	// New task. The stage is left to the daemon, which puts a new task in the first
	// stage of the project's list; everything else here is optional but the title.
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import type { TaskKind, TaskPriority, Uuid } from '$lib/types';
	import Button from '$lib/ui/Button.svelte';
	import Dialog from '$lib/ui/Dialog.svelte';
	import Input from '$lib/ui/Input.svelte';
	import Select from '$lib/ui/Select.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	interface Props {
		open: boolean;
		/** The board's project filter; null creates a task with no project. */
		projectId: Uuid | null;
		/** That project's name, for the line naming where the task will land. */
		projectName: string | null;
		onclose: () => void;
		oncreated: () => void | Promise<void>;
	}

	let { open, projectId, projectName, onclose, oncreated }: Props = $props();

	// The board filter decides which board this files on, and the filter bar is
	// behind the dialog, so say which board it is rather than leaving it to be found
	// out after the task is created.
	const target = $derived(projectName ? `Project: ${projectName}` : 'Global board');

	const kindOptions = (['task', 'bug', 'feature', 'chore'] as TaskKind[]).map((k) => ({
		value: k,
		label: k
	}));
	const priorityOptions = (['low', 'medium', 'high', 'urgent'] as TaskPriority[]).map((p) => ({
		value: p,
		label: p
	}));

	let title = $state('');
	let description = $state('');
	let kind = $state('task');
	let priority = $state('medium');
	let labels = $state('');
	let blockedBy = $state('');
	let creating = $state(false);

	const splitList = (text: string) =>
		text
			.split(',')
			.map((s) => s.trim())
			.filter(Boolean);

	function reset() {
		title = '';
		description = '';
		kind = 'task';
		priority = 'medium';
		labels = '';
		blockedBy = '';
	}

	function cancel() {
		reset();
		onclose();
	}

	async function create() {
		const name = title.trim();
		if (!name) return;
		creating = true;
		try {
			await api().createTask({
				project_id: projectId,
				title: name,
				description,
				kind: kind as TaskKind,
				priority: priority as TaskPriority,
				labels: splitList(labels),
				blocked_by: splitList(blockedBy)
			});
			reset();
			onclose();
			await oncreated();
			push('success', 'Task created');
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} title="New task" onclose={cancel}>
	<div class="form" data-testid="task-new">
		<p class="target" data-testid="new-task-target">{target}</p>

		<label class="field">
			<span>Title</span>
			<Input bind:value={title} data-testid="new-task-title" placeholder="What needs doing" />
		</label>

		<label class="field">
			<span>Description</span>
			<Textarea bind:value={description} rows={4} data-testid="new-task-description" />
		</label>

		<div class="pair">
			<label class="field">
				<span>Kind</span>
				<Select bind:value={kind} options={kindOptions} data-testid="new-task-kind" />
			</label>
			<label class="field">
				<span>Priority</span>
				<Select bind:value={priority} options={priorityOptions} data-testid="new-task-priority" />
			</label>
		</div>

		<label class="field">
			<span>Labels</span>
			<Input bind:value={labels} data-testid="new-task-labels" placeholder="api, ui" />
		</label>

		<label class="field">
			<span>Blocked by</span>
			<Input bind:value={blockedBy} data-testid="new-task-blockers" placeholder="ATL-3, ATL-7" />
			<span class="hint">Task keys, separated by commas.</span>
		</label>
	</div>

	{#snippet footer()}
		<Button onclick={cancel}>Cancel</Button>
		<Button
			variant="primary"
			data-testid="new-task-create"
			disabled={creating || title.trim() === ''}
			onclick={create}
		>
			{creating ? 'Creating…' : 'Create'}
		</Button>
	{/snippet}
</Dialog>

<style>
	.form {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
	}

	.field > span {
		font-size: 13px;
		color: var(--text-secondary);
	}

	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: var(--space-3);
	}

	.hint {
		color: var(--text-secondary);
		font-size: 12px;
	}

	.target {
		margin: 0;
		color: var(--text-secondary);
		font-size: 13px;
	}
</style>
