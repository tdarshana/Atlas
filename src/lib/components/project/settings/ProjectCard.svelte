<script lang="ts">
	// The Project card of frame 02.7: what the project is called, the prefix its task keys
	// carry, where it lives, and where it is pushed.
	import { Input } from '$lib/ds';
	import { KEY_PREFIX_RE } from './settings';

	interface Props {
		name: string;
		boardKey: string;
		remote: string;
		/** Read only: the root is set when the project is connected. */
		root: string;
	}

	let {
		name = $bindable(),
		boardKey = $bindable(),
		remote = $bindable(),
		root
	}: Props = $props();

	// The daemon applies the same rule, but saying so before the request is what lets the
	// field explain itself rather than the toast.
	const keyError = $derived(
		boardKey && !KEY_PREFIX_RE.test(boardKey)
			? '2 to 6 characters, starting with a letter, letters and digits only.'
			: undefined
	);
</script>

<section class="card" data-testid="settings-project">
	<header><span class="title">Project</span></header>
	<div class="body">
		<div class="row">
			<Input label="Name" bind:value={name} />
			<!-- A key prefix is always upper case, so it is folded as it is typed rather than
			     refused after the fact. -->
			<Input
				label="Key prefix"
				mono
				value={boardKey}
				error={keyError}
				oninput={(e) => (boardKey = e.currentTarget.value.toUpperCase())}
			/>
		</div>
		<Input
			label="Root"
			mono
			readonly
			value={root}
			hint="Set when the project was connected. Reconnect to change it."
		/>
		<Input label="Remote" mono placeholder="git@github.com:org/repo.git" bind:value={remote} />
	</div>
</section>

<style>
	.card {
		display: flex;
		flex-direction: column;
		overflow: hidden;
	}

	header {
		display: flex;
		align-items: center;
		height: 32px;
		flex: 0 0 32px;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.title {
		font-weight: 600;
	}

	.body {
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 12px;
	}

	.row {
		display: grid;
		grid-template-columns: 1fr 120px;
		gap: 12px;
	}
</style>
