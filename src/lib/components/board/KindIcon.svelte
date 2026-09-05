<script lang="ts">
	// A task kind's mark: a small rounded square in the kind's colour with its white
	// glyph, in the style of an issue tracker. `subtask` draws the subtask mark instead.
	import { Icon } from '$lib/ds';
	import { kindMeta, SUBTASK_META } from './kind';

	interface Props {
		kind?: string;
		subtask?: boolean;
		size?: number;
	}

	let { kind = 'task', subtask = false, size = 14 }: Props = $props();

	const meta = $derived(subtask ? SUBTASK_META : kindMeta(kind));
</script>

<span
	class="kind-icon"
	style="width:{size}px;height:{size}px;background:{meta.color}"
	title={meta.label}
	role="img"
	aria-label={meta.label}
	data-testid="kind-icon-{subtask ? 'subtask' : kind}"
>
	<Icon name={meta.icon} size={Math.round(size * 0.7)} color="#fff" />
</span>

<style>
	.kind-icon {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		flex: 0 0 auto;
		border-radius: 3px;
	}
</style>
