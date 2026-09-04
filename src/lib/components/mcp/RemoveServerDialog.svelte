<script lang="ts">
	// Removing a server rewrites one of the agent's own config files, so the confirm
	// names the file it is about to edit rather than just the server.
	import { Button } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import type { McpServerEntry } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';

	interface Props {
		/** The server to remove, or null when the dialog is closed. */
		server: McpServerEntry | null;
		onclose: () => void;
		onremove: (id: string) => Promise<unknown>;
	}

	let { server, onclose, onremove }: Props = $props();

	let removing = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (server) error = null;
	});

	async function confirm(): Promise<void> {
		if (!server) return;
		removing = true;
		try {
			await onremove(server.id);
			onclose();
		} catch (e) {
			error = errorMessage(e);
		} finally {
			removing = false;
		}
	}
</script>

<Dialog open={!!server} title="Remove MCP server" {onclose}>
	{#if server}
		<div class="body">
			<p>
				Remove <span class="mono">{server.name}</span> from
				<span class="mono">{server.file ?? 'its config file'}</span>?
			</p>
			<span class="hint" data-testid="remove-server-undo">
				Atlas copies the file into <span class="mono">~/.atlas/config-backups/</span> first, so
				the edit can be undone by hand.
			</span>
			{#if error}<p class="bad" role="alert" data-testid="remove-server-error">{error}</p>{/if}
		</div>
	{/if}

	{#snippet footer()}
		<span class="spacer"></span>
		<Button variant="ghost" size="sm" onclick={onclose}>Keep</Button>
		<Button
			variant="danger"
			size="sm"
			disabled={removing}
			data-testid="remove-server-confirm"
			onclick={confirm}
		>
			{removing ? 'Removing…' : 'Remove'}
		</Button>
	{/snippet}
</Dialog>

<style>
	.body {
		display: flex;
		flex-direction: column;
		gap: 8px;
		min-width: 420px;
	}

	p {
		margin: 0;
		word-break: break-all;
	}

	.spacer {
		flex: 1;
	}

	.mono {
		font-family: var(--font-mono);
		font-size: 12px;
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
