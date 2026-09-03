<script module lang="ts">
	/**
	 * What removing a project actually does, in one sentence used by every screen that
	 * offers it. `ProjectRepo::delete` keeps the project's memories deliberately, since
	 * nothing is ever hard-deleted from `memories`, so the copy says so. This deviates
	 * from frame 02.7, which predates that decision.
	 */
	export const REMOVE_PROJECT_COPY =
		'Removing the project forgets its root and its tasks. Memories and files on disk are not touched.';
</script>

<script lang="ts">
	// The one Remove project confirm, shared by the Profile tab's header button and the
	// Settings tab's Danger zone, so the two cannot disagree about the blast radius.
	import { Button } from '$lib/ds';
	import { project } from '$lib/stores/project.svelte';
	import Dialog from '$lib/ui/Dialog.svelte';

	interface Props {
		open: boolean;
		/** The project's name, said back to the user before they confirm. */
		name: string;
		/** The confirm button's `data-testid`; the two tabs are told apart by it. */
		testid: string;
		onclose: () => void;
		onconfirm: () => void;
	}

	let { open, name, testid, onclose, onconfirm }: Props = $props();
</script>

<Dialog {open} title="Remove this project?" {onclose}>
	<p class="prose">
		Atlas forgets <strong>{name}</strong> and stops offering it as a scope.
		{REMOVE_PROJECT_COPY} Connecting the same root again re-adds it.
	</p>
	{#snippet footer()}
		<Button onclick={onclose}>Cancel</Button>
		<Button variant="danger" data-testid={testid} disabled={project.removing} onclick={onconfirm}>
			{project.removing ? 'Removing…' : 'Remove'}
		</Button>
	{/snippet}
</Dialog>

<style>
	.prose {
		margin: 0;
		max-width: 80ch;
	}
</style>
