<script lang="ts">
	// The agent editor (frame 06). A dialog rather than a page of its own: the Agents
	// table is the only way in, and the frame keeps the list on screen behind it. The
	// name is the identity the API saves under, so it is fixed once an agent exists.
	import { Button, Input } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { deleteAgent, nameError, parseList, saveAgent } from '$lib/stores/agents.svelte';
	import Dialog from '$lib/ui/Dialog.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	interface Props {
		open: boolean;
		/** The agent to edit, or null to create one. */
		editing: string | null;
		onclose: () => void;
		/** Fired after a successful save or delete, so the list can refresh. */
		onchanged: () => void | Promise<void>;
	}

	let { open, editing, onclose, onchanged }: Props = $props();

	let name = $state('');
	let description = $state('');
	let instructions = $state('');
	let modelHint = $state('');
	let tools = $state('');
	let tags = $state('');

	let loading = $state(false);
	let saving = $state(false);
	let error = $state<string | null>(null);
	let confirming = $state(false);

	const invalid = $derived(nameError(name));

	/**
	 * Opening seeds the form: blank for a new agent, the stored agent for an edit. It
	 * keys on `editing` as well as `open` so picking a second agent while the dialog is
	 * up refills it rather than showing the first one's instructions.
	 */
	$effect(() => {
		if (!open) return;
		const target = editing;
		name = target ?? '';
		description = '';
		instructions = '';
		modelHint = '';
		tools = '';
		tags = '';
		error = null;
		confirming = false;
		if (!target) return;
		loading = true;
		api()
			.getAgent(target)
			.then((agent) => {
				name = agent.name;
				description = agent.description;
				instructions = agent.instructions;
				modelHint = agent.model_hint ?? '';
				tools = agent.tools.join(', ');
				tags = agent.tags.join(', ');
			})
			.catch((e) => (error = errorMessage(e)))
			.finally(() => (loading = false));
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
			await saveAgent({
				name,
				description,
				instructions,
				model_hint: modelHint === '' ? null : modelHint,
				tools: parseList(tools),
				tags: parseList(tags)
			});
			push('success', `Saved ${name}`);
			onclose();
			await onchanged();
		} catch (e) {
			// The daemon's `error` string is the useful message on a 400.
			error = errorMessage(e);
			push('error', error);
		} finally {
			saving = false;
		}
	}

	async function remove(): Promise<void> {
		if (!editing) return;
		confirming = false;
		try {
			await deleteAgent(editing);
			push('success', `Deleted ${editing}`);
			onclose();
			await onchanged();
		} catch (e) {
			push('error', errorMessage(e));
		}
	}
</script>

<Dialog {open} title={editing ? `Edit ${editing}` : 'New agent'} onclose={cancel}>
	<div class="form">
		<label class="field">
			<span>Name</span>
			<Input
				bind:value={name}
				mono
				disabled={!!editing}
				placeholder="code-reviewer"
				data-testid="agent-name"
			/>
			{#if !editing && name !== '' && invalid}
				<span class="bad">{invalid}</span>
			{/if}
		</label>

		<label class="field">
			<span>Description</span>
			<Input bind:value={description} placeholder="Reviews diffs for correctness" />
		</label>

		<label class="field">
			<span>Instructions</span>
			<Textarea bind:value={instructions} mono rows={12} data-testid="agent-instructions" />
		</label>

		<div class="pair">
			<label class="field">
				<span>Model hint</span>
				<Input bind:value={modelHint} mono placeholder="claude-sonnet" />
			</label>
			<label class="field">
				<span>Tools</span>
				<Input bind:value={tools} mono placeholder="read, grep, bash" />
			</label>
		</div>

		<label class="field">
			<span>Tags</span>
			<Input bind:value={tags} placeholder="review, rust" />
		</label>

		{#if error}
			<p class="bad" role="alert" data-testid="agent-error">{error}</p>
		{/if}
	</div>

	{#snippet footer()}
		{#if editing}
			<Button variant="danger" data-testid="agent-delete" onclick={() => (confirming = true)}>
				Delete
			</Button>
		{/if}
		<span class="spacer"></span>
		<Button onclick={cancel}>Cancel</Button>
		<Button
			variant="primary"
			data-testid="agent-save"
			disabled={saving || loading}
			onclick={save}
		>
			{saving ? 'Saving…' : 'Save'}
		</Button>
	{/snippet}
</Dialog>

<Dialog open={confirming} title="Delete agent" onclose={() => (confirming = false)}>
	<p>Delete <strong>{editing}</strong>? Files already synced to disk stay where they are.</p>
	{#snippet footer()}
		<Button onclick={() => (confirming = false)}>Cancel</Button>
		<Button variant="danger" data-testid="agent-delete-confirm" onclick={remove}>Delete</Button>
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
