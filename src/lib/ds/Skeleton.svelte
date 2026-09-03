<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';

	interface Props extends Omit<HTMLAttributes<HTMLElement>, 'class'> {
		width?: string | number;
		height?: string | number;
		/** When set, renders a block of placeholder rows instead of a single bar. */
		rows?: number;
		columns?: number;
	}

	let { width = '100%', height = 10, rows, columns = 4, ...rest }: Props = $props();

	const WIDTHS = [56, 140, 92, 72, 110];

	const size = (v: string | number) => (typeof v === 'number' ? `${v}px` : v);
</script>

{#if rows}
	<div {...rest} class="dbm-skeleton-rows" role="status" aria-label="Loading rows">
		{#each { length: rows } as _, r (r)}
			<div>
				{#each { length: columns } as _, c (c)}
					<span
						class="dbm-skeleton"
						style="width:{WIDTHS[c % WIDTHS.length]}px;opacity:{1 - r * 0.06}"
					></span>
				{/each}
			</div>
		{/each}
	</div>
{:else}
	<span
		{...rest}
		class="dbm-skeleton"
		style="width:{size(width)};height:{size(height)}"
		role="status"
		aria-label="Loading"
	></span>
{/if}
