<script lang="ts">
	// One line, shown on the Plugins page and in the Settings Plugins card, when the channel
	// that carries MCP tool calls has been down for longer than its grace period. Without
	// it a refused socket leaves a plugin's tools quietly missing with nothing on screen.
	import { channelWarning } from './tool-channel.svelte';

	const warning = $derived(channelWarning());
</script>

{#if warning}
	<p class="warn" role="alert" data-testid="plugin-channel-warning">
		{warning}
		<span class="hint">Agents cannot call this plugin's tools until it reconnects.</span>
	</p>
{/if}

<style>
	.warn {
		margin: 0;
		font-size: 12px;
		color: var(--warning-text);
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}
</style>
