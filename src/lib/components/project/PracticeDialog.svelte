<script lang="ts">
	// The Practices tab's "New practice" dialog (frame 02.3): the same doc editor the
	// global Practices screen uses, minus the project picker, since a doc created here is
	// always scoped to this project. Editing an existing doc keeps whatever scope it
	// already has; only creation is fixed.
	import { Button, Input } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { nameError, parseList } from '$lib/stores/agents.svelte';
	import type { Doc, NewDoc, Uuid } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	interface Props {
		open: boolean;
		/** The doc to edit, or null for a new project-scoped practice. */
		editing: Doc | null;
		projectId: Uuid;
		onclose: () => void;
		onsave: (doc: NewDoc) => Promise<Doc>;
	}

	let { open, editing, projectId, onclose, onsave }: Props = $props();

	let name = $state('');
	let body = $state('');
	let tags = $state('');
	let saving = $state(false);
	let error = $state<string | null>(null);

	// The dialog's own instance is reused across opens, so seed the fields whenever it is
	// asked to open rather than once at mount.
	$effect(() => {
		if (!open) return;
		name = editing?.name ?? '';
		body = editing?.body ?? '';
		tags = editing?.tags.join(', ') ?? '';
		error = null;
	});

	const invalid = $derived(nameError(name));

	function cancel(): void {
		onclose();
	}

	async function save(): Promise<void> {
		if (invalid) {
			error = invalid;
			return;
		}
		saving = true;
		error = null;
		try {
			await onsave({
				name,
				body,
				tags: parseList(tags),
				project_id: editing ? editing.project_id : projectId
			});
			push('success', `Saved ${name}`);
			onclose();
		} catch (e) {
			error = errorMessage(e);
			push('error', error);
		} finally {
			saving = false;
		}
	}
</script>

<Dialog {open} title={editing ? `Edit ${editing.name}` : 'New practice'} onclose={cancel}>
	<div class="form">
		<label class="field">
			<span>Name</span>
			<Input
				bind:value={name}
				disabled={!!editing}
				placeholder="commit-style"
				data-testid="practice-name"
			/>
			{#if !editing && name !== '' && invalid}
				<span class="bad">{invalid}</span>
			{/if}
		</label>

		<label class="field">
			<span>Body (Markdown)</span>
			<Textarea bind:value={body} mono rows={12} data-testid="practice-body" />
		</label>

		<label class="field">
			<span>Tags</span>
			<Input bind:value={tags} data-testid="practice-tags" placeholder="git, review" />
		</label>

		{#if error}
			<p class="bad" role="alert" data-testid="practice-error">{error}</p>
		{/if}
	</div>

	{#snippet footer()}
		<Button onclick={cancel}>Cancel</Button>
		<Button variant="primary" data-testid="practice-save" disabled={saving} onclick={save}>
			{saving ? 'Saving…' : 'Save'}
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

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}
</style>
