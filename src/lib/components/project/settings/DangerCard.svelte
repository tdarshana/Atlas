<script lang="ts">
	// The Danger zone card of frame 02.7. The confirm and the delete itself stay with the
	// page, which is the piece that has somewhere to navigate afterwards.
	import { Button } from '$lib/ds';

	interface Props {
		disabled: boolean;
		onremove: () => void;
	}

	let { disabled, onremove }: Props = $props();
</script>

<section class="card" data-testid="settings-danger">
	<header><span class="title">Danger zone</span></header>
	<div class="body">
		<span class="sentence">
			Removing the project forgets its root, tasks, and project-scoped memories. Files on disk
			are not touched.
		</span>
		<Button variant="danger" data-testid="settings-remove" {disabled} onclick={onremove}>
			Remove project…
		</Button>
	</div>
</section>

<style>
	.card {
		display: flex;
		flex-direction: column;
		overflow: hidden;
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
