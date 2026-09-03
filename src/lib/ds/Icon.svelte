<script lang="ts">
	import { resolveIcon } from './icons';

	interface Props {
		/** A name from the design file's Lucide vocabulary. Unknown names fall back to `circle`. */
		name: string;
		size?: number;
		color?: string;
		/** Set only when the glyph carries meaning. Decorative glyphs stay aria-hidden. */
		title?: string;
	}

	let { name, size = 14, color = 'currentColor', title }: Props = $props();

	const Glyph = $derived(resolveIcon(name));
</script>

<span
	role={title ? 'img' : undefined}
	aria-label={title}
	aria-hidden={title ? undefined : 'true'}
	style="display:inline-flex;width:{size}px;height:{size}px;flex:0 0 auto;color:{color}"
>
	<Glyph {size} style="display:block" />
</span>
