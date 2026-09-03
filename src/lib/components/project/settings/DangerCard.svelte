<script lang="ts">
	// The Danger zone card of frame 02.7. The confirm and the delete itself stay with the
	// page, which is the piece that has somewhere to navigate afterwards.
	import { Button } from '$lib/ds';
	import { REMOVE_PROJECT_COPY } from '../RemoveProjectDialog.svelte';

	interface Props {
		disabled: boolean;
		onremove: () => void;
	}

	let { disabled, onremove }: Props = $props();
</script>

<section class="card" data-testid="settings-danger">
	<header><span class="title">Danger zone</span></header>
	<div class="body">
		<span class="sentence">{REMOVE_PROJECT_COPY}</span>
		<Button variant="danger" data-testid="settings-remove" {disabled} onclick={onremove}>
			Remove project…
		</Button>
	</div>
</section>

<style>
	/* The pane scrolls, not the card: a card that shrank would clip its own fields. */
	.card {
		display: flex;
		flex-direction: column;
		flex: 0 0 auto;
	}

	header {
		display: flex;
		align-items: center;
		height: 32px;
		flex: 0 0 32px;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.title {
		font-weight: 600;
	}

	.body {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 12px;
	}

	.sentence {
		flex: 1;
		color: var(--text-secondary);
	}
</style>
