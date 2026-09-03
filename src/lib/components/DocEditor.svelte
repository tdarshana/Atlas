<script lang="ts">
	// The practice and workflow editor (frame 07). Both kinds are the same shape behind
	// the same routes, so one dialog serves both; the caller names the kind. The name is
	// the identity the API saves under, so it is fixed once a doc exists.
	import { Button, Input, Select } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { nameError, parseList } from '$lib/stores/agents.svelte';
	import type { Doc, NewDoc, Project } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	interface Props {
		open: boolean;
		/** The doc to edit, or null to create one. */
		editing: Doc | null;
		/** Singular noun for the title and the buttons, e.g. `practice`. */
		noun: string;
		projects: Project[];
		onclose: () => void;
		onsave: (doc: NewDoc) => Promise<Doc>;
		ondelete: (name: string) => Promise<void>;
	}

	let { open, editing, noun, projects, onclose, onsave, ondelete }: Props = $props();

	/** `Select` binds a string, so the global scope is the empty value. */
	const GLOBAL = '';

	let name = $state('');
	let body = $state('');
	let tags = $state('');
	let projectId = $state(GLOBAL);
	let saving = $state(false);
	let error = $state<string | null>(null);
	let confirming = $state(false);

	const invalid = $derived(nameError(name));

	const projectOptions = $derived([
		{ value: GLOBAL, label: 'Global (no project)' },
		...projects.map((p) => ({ value: p.id, label: p.name }))
	]);

	/**
	 * Opening seeds the form. It keys on `editing` as well as `open` so picking a second
	 * doc while the dialog is up refills it rather than showing the first one's body.
	 */
	$effect(() => {
		if (!open) return;
		const doc = editing;
		name = doc?.name ?? '';
		body = doc?.body ?? '';
		tags = doc?.tags.join(', ') ?? '';
		projectId = doc?.project_id ?? GLOBAL;
		error = null;
		confirming = false;
	});

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
				project_id: projectId === GLOBAL ? null : projectId
			});
			push('success', `Saved ${name}`);
			onclose();
		} catch (e) {
			// A 400 carries the daemon's own message; show it as it came.
			error = errorMessage(e);
			push('error', error);
		} finally {
			saving = false;
		}
	}

	async function remove(): Promise<void> {
		const doc = editing;
		confirming = false;
		if (!doc) return;
		try {
			await ondelete(doc.name);
			push('success', `Deleted ${doc.name}`);
			onclose();
		} catch (e) {
			push('error', errorMessage(e));
		}
	}
</script>

<Dialog {open} title={editing ? `Edit ${editing.name}` : `New ${noun}`} onclose={cancel}>
	<div class="form">
		<label class="field">
			<span>Name</span>
			<Input
				bind:value={name}
				mono
				disabled={!!editing}
				placeholder="commit-style"
				data-testid="doc-name"
			/>
			{#if !editing && name !== '' && invalid}
				<span class="bad">{invalid}</span>
			{/if}
		</label>

		<label class="field">
			<span>Body (Markdown)</span>
			<Textarea bind:value={body} mono rows={12} data-testid="doc-body" />
		</label>

		<div class="pair">
			<label class="field">
				<span>Tags</span>
				<Input bind:value={tags} placeholder="git, review" />
			</label>
			<label class="field">
				<span>Project</span>
				<Select bind:value={projectId} options={projectOptions} data-testid="doc-project" />
			</label>
		</div>

		{#if error}
			<p class="bad" role="alert" data-testid="doc-error">{error}</p>
		{/if}
	</div>

	{#snippet footer()}
		{#if editing}
			<Button variant="danger" data-testid="doc-delete" onclick={() => (confirming = true)}>
				Delete
			</Button>
		{/if}
		<span class="spacer"></span>
		<Button onclick={cancel}>Cancel</Button>
		<Button variant="primary" data-testid="doc-save" disabled={saving} onclick={save}>
			{saving ? 'Saving…' : 'Save'}
		</Button>
	{/snippet}
</Dialog>

<Dialog open={confirming} title={`Delete ${noun}`} onclose={() => (confirming = false)}>
	<p>Delete <strong>{editing?.name}</strong>?</p>
	{#snippet footer()}
		<Button onclick={() => (confirming = false)}>Cancel</Button>
		<Button variant="danger" data-testid="doc-delete-confirm" onclick={remove}>Delete</Button>
	{/snippet}
</Dialog>

<style>
	.form {
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
		color: var(--text-secondary);
	}

	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 12px;
	}

	.spacer {
		flex: 1;
	}

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}
</style>
