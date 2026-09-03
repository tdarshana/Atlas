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
</style>
