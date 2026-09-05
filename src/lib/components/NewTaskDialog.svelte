<script lang="ts">
	// New task. The stage is left to the daemon, which puts a new task in the first
	// stage of the project's list; everything else here is optional but the title.
	import { Button, Input } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { personas } from '$lib/stores/personas.svelte';
	import type { TaskKind, TaskPriority, Uuid } from '$lib/types';
	import { KIND_MENU, PRIORITY_MENU } from '$lib/components/board/kind';
	import MenuSelect from '$lib/components/board/MenuSelect.svelte';
	import Dialog from '$lib/ui/Dialog.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/platform/toasts.svelte';

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

	const personaOptions = $derived([
		{ value: '', label: 'None' },
		...personas.roster.map((r) => ({ value: r.slug, label: r.name, hint: r.role }))
	]);

	let title = $state('');
	let description = $state('');
	let kind = $state('task');
	let priority = $state('medium');
	let persona = $state('');
	let labels = $state('');
	let blockedBy = $state('');
	let creating = $state(false);

	/** The textarea is not a ds component, so its label is tied to it by hand. */
	const descriptionId = $props.id();

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
		persona = '';
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
				persona: persona || undefined,
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

		<Input
			label="Title"
			bind:value={title}
			data-testid="new-task-title"
			placeholder="What needs doing"
		/>

		<!-- The design system has no textarea of its own, so the shared one wears the
		     system's field classes rather than a second set of labels. -->
		<span class="dbm-field">
			<label class="dbm-field__label" for={descriptionId}>Description</label>
			<Textarea id={descriptionId} bind:value={description} rows={4} data-testid="new-task-description" />
		</span>

		<div class="pair">
			<MenuSelect label="Kind" value={kind} options={KIND_MENU} testId="new-task-kind" onchange={(k) => (kind = k)} />
			<MenuSelect
				label="Priority"
				value={priority}
				options={PRIORITY_MENU}
				testId="new-task-priority"
				onchange={(p) => (priority = p)}
			/>
			<MenuSelect
				label="Persona"
				value={persona}
				options={personaOptions}
				searchable
				testId="new-task-persona"
				onchange={(v) => (persona = v)}
			/>
		</div>

		<Input label="Labels" bind:value={labels} data-testid="new-task-labels" placeholder="api, ui" />

		<Input
			label="Blocked by"
			hint="Task keys, separated by commas."
			bind:value={blockedBy}
			data-testid="new-task-blockers"
			placeholder="ATL-3, ATL-7"
		/>
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

	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: var(--space-3);
	}

	.target {
		margin: 0;
		color: var(--text-secondary);
		font-size: var(--text-sm);
	}
</style>
