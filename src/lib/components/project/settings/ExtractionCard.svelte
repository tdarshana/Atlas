<script lang="ts">
	// The Extraction card of frame 02.7. Ticked, the project follows the global settings and
	// stores nothing of its own; unticked, each field it fills in overrides the global one,
	// field by field, and each field left blank still inherits.
	import { Button, Checkbox, Input } from '$lib/ds';
	import { type ExtractionForm, keyWillDrop } from './settings';

	interface Props {
		form: ExtractionForm;
		testing: boolean;
		/** The last `Test connection` answer, or null before one has been asked for. */
		result: { ok: boolean; text: string } | null;
		ontest: () => void;
	}

	let { form = $bindable(), testing, result, ontest }: Props = $props();

	const dropping = $derived(keyWillDrop(form));

	const keyHint = $derived(
		dropping
			? 'The stored key belongs to the base URL this project was loaded with. Saving without a new key clears it.'
			: form.storedKey
				? 'Key stored.'
				: 'No key stored yet.'
	);

	/**
	 * The project's own switch. Untouched it stays null, which inherits the global one, so
	 * opening this card and setting a model does not start extraction nobody asked for.
	 */
	function setEnabled(on: boolean) {
		form = { ...form, enabled: on };
	}
</script>

<section class="card" data-testid="settings-extraction">
	<header>
		<span class="title">Extraction</span>
		<span class="spacer"></span>
		<Button
			variant="ghost"
			size="sm"
			data-testid="extraction-test"
			disabled={testing}
			onclick={ontest}
		>
			{testing ? 'Testing…' : 'Test connection'}
		</Button>
	</header>
	<div class="body">
		<Checkbox label="Use global extraction settings" bind:checked={form.useGlobal} />

		{#if form.useGlobal}
			<span class="hint">
				Inherited from Settings, Extraction. Untick to override for this project only.
			</span>
		{:else}
			<Checkbox
				label="Run extraction for this project"
				checked={form.enabled === true}
				indeterminate={form.enabled === null}
				onchange={(e) => setEnabled(e.currentTarget.checked)}
			/>
			{#if form.enabled === null}
				<span class="hint">Following the global switch until you set it here.</span>
			{/if}
			<div class="row">
				<Input
					label="Base URL"
					mono
					placeholder="https://api.deepseek.com"
					bind:value={form.baseUrl}
				/>
				<Input label="Model" mono placeholder="deepseek-chat" bind:value={form.model} />
				<Input
					label="Auto-accept threshold"
					mono
					type="number"
					min="0"
					max="1"
					step="0.05"
					bind:value={form.threshold}
				/>
			</div>
			<Input
				label="API key"
				type="password"
				autocomplete="off"
				placeholder="sk-…"
				hint={keyHint}
				bind:value={form.apiKey}
			/>
		{/if}

		{#if result}
			<span class="result" class:bad={!result.ok} role="status">{result.text}</span>
		{/if}
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

	.spacer {
		flex: 1;
	}

	.body {
		display: flex;
		flex-direction: column;
		gap: 10px;
		padding: 12px;
	}

	.row {
		display: grid;
		grid-template-columns: 1fr 1fr 200px;
		gap: 12px;
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.result {
		color: var(--success-text);
	}

	.result.bad {
		color: var(--danger-text);
	}
</style>
