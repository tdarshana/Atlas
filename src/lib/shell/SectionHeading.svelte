<script lang="ts">
	// The uppercase group label: PROJECTS, KINDS, BACKLOG.
	import type { Snippet } from 'svelte';

	let { children }: { children: Snippet } = $props();
</script>

<span class="group-heading">{@render children()}</span>
