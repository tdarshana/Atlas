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
		margin-bottom: var(--space-4);
	}

	h1 {
		margin: var(--space-1) 0 0;
	}

	a {
		color: var(--muted);
		font-size: 13px;
	}

	.sync {
		margin-top: var(--space-5);
	}
</style>
