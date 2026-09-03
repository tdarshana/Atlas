<script lang="ts">
	// The global Memories screen's Remember dialog, opened from the command palette with
	// `/memories?remember=1`. Scope is a field here rather than fixed, because this screen
	// spans every project; the project tab's own dialog knows its project already.
	import { Button, Input, Select } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { MEMORY_KINDS } from '$lib/stores/memories.svelte';
	import type { Memory, MemoryKind, Project } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	interface Props {
		open: boolean;
		projects: Project[];
		onclose: () => void;
		onsaved: (memory: Memory) => void;
	}

	let { open, projects, onclose, onsaved }: Props = $props();

	/** `Select` binds a string, so the global scope is the empty value. */
	const GLOBAL = '';

	const kindOptions = MEMORY_KINDS.map((k) => ({ value: k, label: k }));
	const scopeOptions = $derived([
		{ value: GLOBAL, label: 'Global' },
		...projects.map((p) => ({ value: p.id, label: p.name }))
	]);

	let text = $state('');
	// Plain strings, not the union types: `Select` binds a string, and the value can only
	// ever be one of the options anyway, so casting on save is safe.
	let kind = $state('fact');
	let tags = $state('');
	let scope = $state(GLOBAL);
	let saving = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (!open) return;
		text = '';
		kind = 'fact';
		tags = '';
		scope = GLOBAL;
		error = null;
	});

	function cancel(): void {
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
				scope: scope === GLOBAL ? 'global' : 'project',
				project_id: scope === GLOBAL ? null : scope,
				kind: kind as MemoryKind,
				text: body,
				tags: tags
					.split(',')
					.map((t) => t.trim())
					.filter((t) => t !== '')
			});
			push('success', 'Remembered');
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
			<Textarea
				bind:value={text}
				rows={5}
				data-testid="remember-text"
				placeholder="What should Atlas remember?"
			/>
		</label>

		<div class="pair">
			<label class="field">
				<span>Kind</span>
				<Select bind:value={kind} options={kindOptions} data-testid="remember-kind" />
			</label>
			<label class="field">
				<span>Scope</span>
				<Select bind:value={scope} options={scopeOptions} data-testid="remember-scope" />
			</label>
		</div>

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

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}
</style>
