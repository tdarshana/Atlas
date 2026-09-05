<script lang="ts">
	// A tag editor in the Input's clothes: each tag is a badge with a remove button, and
	// the text box after them commits on Enter, on a comma, or when focus leaves. There
	// is no DBMan original, so it borrows the input's border and the accent badge.
	import Badge from './Badge.svelte';
	import Icon from './Icon.svelte';

	interface Props {
		value?: string[];
		label?: string;
		hint?: string;
		placeholder?: string;
		id?: string;
		disabled?: boolean;
		onchange?: (value: string[]) => void;
		/** The text box's test id; each tag's remove button gets `-remove-<tag>`. */
		testId?: string;
	}

	let {
		value = $bindable([]),
		label,
		hint,
		placeholder = 'Add a tag',
		id,
		disabled = false,
		onchange,
		testId
	}: Props = $props();

	const uid = $props.id();
	const fid = $derived(id ?? uid);

	let text = $state('');

	function set(next: string[]): void {
		value = next;
		onchange?.(next);
	}

	/** Adds what is typed, split on commas, skipping blanks and repeats. */
	function commit(): void {
		const added = text
			.split(',')
			.map((t) => t.trim())
			.filter((t) => t !== '' && !value.includes(t));
		text = '';
		if (added.length > 0) set([...value, ...added]);
	}

	function remove(tag: string): void {
		set(value.filter((t) => t !== tag));
	}

	function onkeydown(event: KeyboardEvent): void {
		if (event.key === 'Enter' || event.key === ',') {
			event.preventDefault();
			commit();
		} else if (event.key === 'Backspace' && text === '' && value.length > 0) {
			event.preventDefault();
			remove(value[value.length - 1]);
		}
	}
</script>

<span class="dbm-field">
	{#if label}<label class="dbm-field__label" for={fid}>{label}</label>{/if}
	<!-- A label, so a click anywhere in the box focuses the text field with no script. -->
	<label class="box" class:disabled for={fid}>
		{#each value as tag (tag)}
			<Badge tone="accent" mono class="tag">
				{tag}
				<button
					type="button"
					class="remove"
					aria-label="Remove {tag}"
					{disabled}
					data-testid={testId ? `${testId}-remove-${tag}` : undefined}
					onclick={() => remove(tag)}
				>
					<Icon name="x" size={10} />
				</button>
			</Badge>
		{/each}
		<input
			id={fid}
			type="text"
			{placeholder}
			{disabled}
			bind:value={text}
			data-testid={testId}
			{onkeydown}
			onblur={commit}
		/>
	</label>
	{#if hint}<span class="dbm-field__hint">{hint}</span>{/if}
</span>

<style>
	.box {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 4px;
		min-height: var(--h-control);
		padding: 3px var(--space-2);
		border: var(--border-width) solid var(--border-default);
		border-radius: var(--radius-sm);
		background: var(--bg-raised);
		transition: var(--transition-hover);
	}

	.box:hover {
		border-color: var(--border-strong);
	}

	.box:focus-within {
		border-color: var(--accent);
		outline: var(--focus-ring-width) solid var(--focus-ring);
		outline-offset: 0;
	}

	.box.disabled {
		opacity: 0.5;
	}

	.box :global(.tag) {
		display: inline-flex;
		align-items: center;
		gap: 3px;
		padding-right: 2px;
	}

	.remove {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 14px;
		height: 14px;
		padding: 0;
		border: none;
		border-radius: 50%;
		background: none;
		color: inherit;
		cursor: pointer;
		opacity: 0.7;
	}

	.remove:hover {
		opacity: 1;
		background: rgb(255 255 255 / 0.12);
	}

	input {
		/* A zero basis: the box sits on the badges' line whenever 80px remain, and grows
		   to fill it, rather than wrapping under them at its intrinsic width. */
		flex: 1 1 0;
		min-width: 48px;
		height: 20px;
		border: none;
		background: none;
		color: var(--text-primary);
		font-family: var(--font-ui);
		font-size: var(--text-sm);
		outline: none;
	}

	input::placeholder {
		color: var(--text-tertiary);
	}
</style>
