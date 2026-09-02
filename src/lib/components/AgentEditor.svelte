<script lang="ts">
	// The agent form, shared by /agents/new and /agents/edit/[name]. The name is the
	// identity the API saves under, so it is fixed once an agent exists.

	import { onMount, untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { deleteAgent, nameError, parseList, saveAgent } from '$lib/stores/agents.svelte';
	import Button from '$lib/ui/Button.svelte';
	import Card from '$lib/ui/Card.svelte';
	import Dialog from '$lib/ui/Dialog.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import Input from '$lib/ui/Input.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	/** The agent to edit; undefined creates a new one. */
	let { name: editing }: { name?: string } = $props();

	// The route keys the editor on the name, so a fresh instance is mounted for a
	// different agent and seeding state from the prop once is deliberate.
	const initial = untrack(() => editing);

	let name = $state(initial ?? '');
	let description = $state('');
	let instructions = $state('');
	let modelHint = $state('');
	let tools = $state('');
	let tags = $state('');

	let loading = $state(!!initial);
	let saving = $state(false);
	let loadError = $state<string | null>(null);
	let saveError = $state<string | null>(null);
	let confirming = $state(false);

	const invalid = $derived(nameError(name));

	onMount(async () => {
		if (!editing) return;
		try {
			const agent = await api().getAgent(editing);
			name = agent.name;
			description = agent.description;
			instructions = agent.instructions;
			modelHint = agent.model_hint ?? '';
			tools = agent.tools.join(', ');
			tags = agent.tags.join(', ');
		} catch (e) {
			loadError = errorMessage(e);
		} finally {
			loading = false;
		}
	});

	async function save(): Promise<void> {
		if (invalid) {
			saveError = invalid;
			return;
		}
		saving = true;
		saveError = null;
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
			if (!editing) await goto(`/agents/edit/${encodeURIComponent(name)}`);
		} catch (e) {
			// The daemon's `error` string is the useful message on a 400.
			saveError = errorMessage(e);
			push('error', saveError);
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
			await goto('/agents');
		} catch (e) {
			push('error', errorMessage(e));
		}
	}
</script>

{#if loadError}
	<ErrorState message={loadError}>
		<Button onclick={() => goto('/agents')}>Back to agents</Button>
	</ErrorState>
{:else}
	<Card title={editing ? `Edit ${editing}` : 'New agent'}>
		{#snippet actions()}
			<Button
				variant="primary"
				data-testid="agent-save"
				disabled={saving || loading}
				onclick={save}
			>
				Save
			</Button>
			{#if editing}
				<Button variant="danger" data-testid="agent-delete" onclick={() => (confirming = true)}>
					Delete
				</Button>
			{/if}
		{/snippet}

		<div class="form">
			<label class="field">
				<span>Name</span>
				<Input
					bind:value={name}
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
				<Textarea bind:value={instructions} mono rows={14} data-testid="agent-instructions" />
			</label>

			<div class="row">
				<label class="field">
					<span>Model hint</span>
					<Input bind:value={modelHint} placeholder="claude-sonnet" />
				</label>
				<label class="field">
					<span>Tools</span>
					<Input bind:value={tools} placeholder="read, grep, bash" />
				</label>
				<label class="field">
					<span>Tags</span>
					<Input bind:value={tags} placeholder="review, rust" />
				</label>
			</div>

			{#if saveError}
				<p class="bad" role="alert" data-testid="agent-error">{saveError}</p>
			{/if}
		</div>
	</Card>
{/if}

<Dialog
	open={confirming}
	title="Delete agent"
	onclose={() => (confirming = false)}
>
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
		gap: var(--space-4);
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

	.row {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-4);
	}

	/* Only fields laid out side by side share the row's width. Inside `.form`, which
	   stacks its children, the same basis is read as a height and stretches every
	   field to 200px. */
	.row > .field {
		flex: 1 1 200px;
	}

	.bad {
		margin: 0;
		color: var(--danger);
		font-size: 13px;
	}
</style>
