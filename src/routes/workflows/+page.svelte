<script lang="ts">
	import DocsPage from '$lib/components/DocsPage.svelte';
	import { workflows } from '$lib/stores/docs.svelte';
</script>

<DocsPage
	store={workflows}
	title="Workflows"
	noun="workflow"
	hint="A workflow is a repeatable procedure agents can follow, written in Markdown."
/>
