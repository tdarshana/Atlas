<script lang="ts">
	// An on/off switch in the design system's colours: a pill that slides its knob and
	// fills with the accent when on. Rest props land on the hidden checkbox, so a
	// caller's `onchange`, `data-testid` and `aria-*` attributes work as on Checkbox.
	import type { HTMLInputAttributes } from 'svelte/elements';

	interface Props extends Omit<HTMLInputAttributes, 'type' | 'checked'> {
		checked?: boolean;
		label?: string;
	}

	let { checked = $bindable(false), label, disabled = false, ...rest }: Props = $props();

	function onchange(event: Event & { currentTarget: EventTarget & HTMLInputElement }) {
		checked = event.currentTarget.checked;
		rest.onchange?.(event);
	}
</script>

<label class="switch" class:on={checked} class:disabled>
	<input {...rest} type="checkbox" role="switch" aria-checked={checked} {checked} {disabled} {onchange} />
	<span class="track" aria-hidden="true"><span class="knob"></span></span>
	{#if label}<span class="text">{label}</span>{/if}
</label>

<style>
	label.switch {
		display: inline-flex;
		align-items: center;
		gap: var(--space-2);
		cursor: pointer;
		user-select: none;
		font-family: var(--font-ui);
		font-size: var(--text-sm);
		color: var(--text-primary);
	}

	label.switch.disabled {
		opacity: 0.5;
		pointer-events: none;
	}

	label.switch input {
		position: absolute;
		width: 1px;
		height: 1px;
		margin: 0;
		opacity: 0;
		pointer-events: none;
	}

	span.track {
		position: relative;
		display: inline-block;
		width: 28px;
		height: 16px;
		flex: none;
		border-radius: 8px;
		background: var(--border-strong);
		transition: background-color var(--dur-hover) var(--ease-hover);
	}

	span.knob {
		position: absolute;
		top: 2px;
		left: 2px;
		width: 12px;
		height: 12px;
		border-radius: 50%;
		background: var(--bg-raised);
		box-shadow: var(--shadow-sm);
		transition: transform var(--dur-hover) var(--ease-hover);
	}

	label.switch.on span.track {
		background: var(--accent);
	}

	label.switch.on span.knob {
		transform: translateX(12px);
		background: var(--text-on-accent);
	}

	label.switch input:focus-visible + span.track {
		outline: var(--focus-ring-width) solid var(--focus-ring);
		outline-offset: var(--focus-ring-offset);
	}
</style>
