<script lang="ts">
	// Asks for a workflow's name before anything is saved, so backing out leaves no
	// untitled-N behind. The suggested name is the next free untitled-N, editable.
	import { Button, Input } from '$lib/ds';
	import Dialog from '$lib/ui/Dialog.svelte';
	import { untitledName } from '$lib/stores/workflows.svelte';

	interface Props {
		open: boolean;
		onclose: () => void;
		oncreate: (name: string) => void | Promise<void>;
	}

	let { open, onclose, oncreate }: Props = $props();

	let name = $state('');
	let creating = $state(false);

	// A fresh suggestion each time the dialog opens.
	$effect(() => {
		if (open) name = untitledName();
	});

	async function create() {
		const trimmed = name.trim();
		if (!trimmed || creating) return;
		creating = true;
		try {
			await oncreate(trimmed);
			onclose();
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} title="New workflow" {onclose}>
	<Input
		label="Name"
		bind:value={name}
		data-testid="new-workflow-name"
		placeholder="nightly-summary"
		onkeydown={(e) => {
			if (e.key === 'Enter') {
				e.preventDefault();
				void create();
			}
		}}
	/>

	{#snippet footer()}
		<Button onclick={onclose}>Cancel</Button>
		<Button
			variant="primary"
			data-testid="new-workflow-create"
			disabled={creating || name.trim() === ''}
			onclick={create}
		>
			{creating ? 'Creating…' : 'Create'}
		</Button>
	{/snippet}
</Dialog>
