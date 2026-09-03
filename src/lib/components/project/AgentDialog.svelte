<script lang="ts">
	// The Agents tab's "New agent" dialog (frame 02.4). `Agent` has no `project_id`
	// (see `agents.ts`'s `projectAgents`), so a project agent is tagged with the
	// project's own name; that tag is guaranteed on save regardless of what the user
	// typed in the Tags field.
	import { Button, Input } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { withProjectTag } from './agents';
	import { nameError, parseList } from '$lib/stores/agents.svelte';
	import type { Agent, NewAgent } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	interface Props {
		open: boolean;
		projectName: string;
		onclose: () => void;
		onsave: (agent: NewAgent) => Promise<Agent>;
	}

	let { open, projectName, onclose, onsave }: Props = $props();

	let name = $state('');
	let description = $state('');
	let instructions = $state('');
	let tags = $state('');
	let saving = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (!open) return;
		name = '';
		description = '';
		instructions = '';
		tags = '';
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
				description,
				instructions,
				tags: withProjectTag(parseList(tags), projectName)
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

<Dialog {open} title="New agent" onclose={cancel}>
	<div class="form">
		<label class="field">
			<span>Name</span>
			<Input bind:value={name} placeholder="code-reviewer" data-testid="project-agent-name" />
			{#if name !== '' && invalid}
				<span class="bad">{invalid}</span>
			{/if}
		</label>

		<label class="field">
			<span>Description</span>
			<Input bind:value={description} placeholder="Reviews diffs for correctness" />
		</label>

		<label class="field">
			<span>Instructions</span>
			<Textarea bind:value={instructions} mono rows={10} data-testid="project-agent-instructions" />
		</label>

		<label class="field">
			<span>Tags</span>
			<Input bind:value={tags} placeholder="review, rust" />
			<span class="hint">Exported into this root only; tagged {projectName} either way.</span>
		</label>

		{#if error}
			<p class="bad" role="alert" data-testid="project-agent-error">{error}</p>
		{/if}
	</div>

	{#snippet footer()}
		<Button onclick={cancel}>Cancel</Button>
		<Button variant="primary" data-testid="project-agent-save" disabled={saving} onclick={save}>
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

	.hint {
		font-size: 12px;
		color: var(--text-tertiary);
	}

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}
</style>
