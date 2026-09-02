<script lang="ts">
	import AgentEditor from '$lib/components/AgentEditor.svelte';
</script>

<header class="head">
	<a href="/agents">← Agents</a>
	<h1>New agent</h1>
</header>

<AgentEditor />

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
</style>
