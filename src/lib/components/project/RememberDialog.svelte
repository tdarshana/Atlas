<script lang="ts">
	// The Memories tab's "Remember…" dialog (frame 02.1): writes a project-scoped memory
	// through the same `remember` route every other screen uses.
	import { Button, Input, Select } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { MEMORY_KINDS } from '$lib/stores/memories.svelte';
	import type { Memory, MemoryKind, Uuid } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	interface Props {
		open: boolean;
		projectId: Uuid;
		onclose: () => void;
		onsaved: (memory: Memory) => void;
	}

	let { open, projectId, onclose, onsaved }: Props = $props();

	const kindOptions = MEMORY_KINDS.map((k) => ({ value: k, label: k }));

	let text = $state('');
	// Plain string, not `MemoryKind`: `Select`'s `bind:value` is a string, and the value can
	// only ever be one of `MEMORY_KINDS` anyway, so casting on save is safe.
	let kind = $state('fact');
	let tags = $state('');
	let saving = $state(false);
	let error = $state<string | null>(null);

	function reset(): void {
		text = '';
		kind = 'fact';
		tags = '';
		error = null;
	}

	function cancel(): void {
		reset();
		onclose();
	}

	async function save(): Promise<void> {
		const body = text.trim();
		if (!body) {
			error = 'Text is required';
			return;
		}
		saving = true;
		error = null;
		try {
			const memory = await api().remember({
				scope: 'project',
				project_id: projectId,
				kind: kind as MemoryKind,
				text: body,
				tags: tags
					.split(',')
					.map((t) => t.trim())
					.filter((t) => t !== '')
			});
			push('success', 'Remembered');
			reset();
			onclose();
			onsaved(memory);
		} catch (e) {
			error = errorMessage(e);
			push('error', error);
		} finally {
			saving = false;
		}
	}
</script>

<Dialog {open} title="Remember" onclose={cancel}>
	<div class="form">
		<label class="field">
			<span>Text</span>
			<Textarea bind:value={text} rows={5} data-testid="remember-text" placeholder="What should Atlas remember?" />
		</label>

		<label class="field">
			<span>Kind</span>
			<Select bind:value={kind} options={kindOptions} data-testid="remember-kind" />
		</label>

		<label class="field">
			<span>Tags</span>
			<Input bind:value={tags} data-testid="remember-tags" placeholder="api, ui" />
		</label>

		{#if error}
			<p class="bad" role="alert" data-testid="remember-error">{error}</p>
		{/if}
	</div>

	{#snippet footer()}
		<Button onclick={cancel}>Cancel</Button>
		<Button variant="primary" data-testid="remember-save" disabled={saving} onclick={save}>
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
