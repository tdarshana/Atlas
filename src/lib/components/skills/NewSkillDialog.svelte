<script lang="ts">
	// Creating an Atlas-native skill, the same shape as the practice and workflow editor:
	// a name the daemon saves under, a one-line description, and the Markdown body with a
	// Preview toggle. Discovered skills are never created here; they come from folders on
	// disk.
	import { Button, Input } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import type { NewSkill, Uuid } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import MarkdownView from '$lib/ui/MarkdownView.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';

	interface Props {
		open: boolean;
		/** Null creates a global skill; an id scopes the new skill to that project. */
		projectId: Uuid | null;
		onclose: () => void;
		oncreate: (input: NewSkill) => Promise<unknown>;
	}

	let { open, projectId, onclose, oncreate }: Props = $props();

	let name = $state('');
	let description = $state('');
	let body = $state('');
	let saving = $state(false);
	let error = $state<string | null>(null);
	let previewing = $state(false);

	/** The daemon's own rule, checked here so a typo is caught before the round trip. */
	const NAME_RE = /^[a-z0-9][a-z0-9-]{0,63}$/;
	const invalid = $derived(
		name.trim() === ''
			? 'A skill needs a name.'
			: NAME_RE.test(name.trim())
				? null
				: 'Lower case letters, digits and dashes, starting with a letter or digit.'
	);

	$effect(() => {
		if (!open) return;
		name = '';
		description = '';
		body = '';
		error = null;
		previewing = false;
	});

	async function create(): Promise<void> {
		if (invalid) {
			error = invalid;
			return;
		}
		saving = true;
		try {
			await oncreate({
				project_id: projectId,
				name: name.trim(),
				description: description.trim(),
				body
			});
			onclose();
		} catch (e) {
			error = errorMessage(e);
		} finally {
			saving = false;
		}
	}
</script>

<Dialog {open} title="New skill" {onclose}>
	<div class="form">
		<Input
			label="Name"
			mono
			placeholder="release-checklist"
			bind:value={name}
			data-testid="new-skill-name"
		/>
		<Input
			label="Description"
			placeholder="What this skill is for, in one line"
			bind:value={description}
			data-testid="new-skill-description"
		/>
		{#if previewing}
			<div class="preview">
				<MarkdownView source={body} showHeader={false} emptyText="Nothing to preview yet." />
			</div>
		{:else}
			<Textarea
				bind:value={body}
				mono
				rows={14}
				aria-label="Body"
				placeholder="# Release checklist"
				data-testid="new-skill-body"
			/>
		{/if}
		{#if error}<p class="bad" role="alert" data-testid="new-skill-error">{error}</p>{/if}
		<span class="hint">
			{projectId ? 'Saved for this project only.' : 'Saved globally, for every project.'}
		</span>
	</div>

	{#snippet footer()}
		<Button variant="ghost" size="sm" onclick={() => (previewing = !previewing)}>
			{previewing ? 'Edit' : 'Preview'}
		</Button>
		<span class="spacer"></span>
		<Button variant="ghost" size="sm" onclick={onclose}>Cancel</Button>
		<Button
			variant="primary"
			size="sm"
			disabled={saving || !!invalid}
			data-testid="new-skill-create"
			onclick={create}
		>
			{saving ? 'Creating…' : 'Create'}
		</Button>
	{/snippet}
</Dialog>

<style>
	.form {
		display: flex;
		flex-direction: column;
		gap: 10px;
		min-width: 520px;
	}

	.preview {
		border: 1px solid var(--border-subtle);
		border-radius: 3px;
		min-height: 200px;
		overflow: hidden;
	}

	.spacer {
		flex: 1;
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}
</style>
