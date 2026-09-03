<script lang="ts">
	import { page } from '$app/state';
	import AgentEditor from '$lib/components/AgentEditor.svelte';
	import SyncPanel from '$lib/components/SyncPanel.svelte';

	const name = $derived(page.params.name ?? '');
</script>

<header class="head">
	<a href="/agents">← Agents</a>
	<h1>{name}</h1>
</header>

{#key name}
	<AgentEditor {name} />
{/key}

<div class="sync">
	<SyncPanel />
</div>

<style>
	.head {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		height: 28px;
		flex: 0 0 28px;
		margin-bottom: var(--space-4);
	}

	h1 {
		margin: 0;
		font-size: 15px;
		font-weight: 600;
	}

	a {
		color: var(--text-secondary);
		font-size: 13px;
	}

	.sync {
		margin-top: var(--space-5);
	}
</style>
