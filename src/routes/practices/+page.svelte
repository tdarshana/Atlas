<script lang="ts">
	import DocsPage from '$lib/components/DocsPage.svelte';
	import { practices } from '$lib/stores/docs.svelte';
</script>

<DocsPage
	store={practices}
	title="Practices"
	noun="practice"
	hint="A practice is a standing rule agents always follow, written in Markdown. Instructions agents load on demand are Skills."
/>
