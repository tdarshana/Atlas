<script lang="ts" module>
	export type Platform = 'mac' | 'windows' | 'linux';

	/* The modifier symbol is resolved at runtime, never hardcoded. */
	export function resolvePlatform(override?: Platform): Platform {
		if (override) return override;
		if (typeof navigator === 'undefined') return 'windows';
		const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
		const p = nav.userAgentData?.platform || nav.platform || '';
		if (/mac/i.test(p)) return 'mac';
		if (/linux/i.test(p)) return 'linux';
		return 'windows';
	}

	const ARROWS = { Up: '↑', Down: '↓', Left: '←', Right: '→' };

	const MAP: Record<Platform, Record<string, string>> = {
		mac: {
			Mod: '⌘',
			Shift: '⇧',
			Alt: '⌥',
			Ctrl: '⌃',
			Enter: '↩',
			Esc: '⎋',
			Escape: '⎋',
			Backspace: '⌫',
			...ARROWS
		},
		windows: {
			Mod: 'Ctrl',
			Shift: 'Shift',
			Alt: 'Alt',
			Ctrl: 'Ctrl',
			Enter: 'Enter',
			Esc: 'Esc',
			Escape: 'Esc',
			Backspace: 'Backspace',
			...ARROWS
		},
		linux: {
			Mod: 'Ctrl',
			Shift: 'Shift',
			Alt: 'Alt',
			Ctrl: 'Ctrl',
			Enter: 'Enter',
			Esc: 'Esc',
			Escape: 'Esc',
			Backspace: 'Backspace',
			...ARROWS
		}
	};

	/** Splits a logical combo such as `Mod+K` into the glyphs for one platform. */
	export function comboKeys(combo: string, platform: Platform): string[] {
		return String(combo)
			.split('+')
			.map((k) => MAP[platform][k.trim()] || k.trim());
	}
</script>

<script lang="ts">
	interface Props {
		/** A logical combo, written with `Mod` for the platform modifier. */
		combo: string;
		platform?: Platform;
		plain?: boolean;
		class?: string;
	}

	let { combo, platform, plain = false, class: className = '' }: Props = $props();

	const os = $derived(resolvePlatform(platform));
	const keys = $derived(comboKeys(combo, os));
	const sep = $derived(os === 'mac' ? '' : '+');

	const cls = $derived(
		['dbm-keyhint', plain && 'dbm-keyhint--plain', className].filter(Boolean).join(' ')
	);
</script>

<!-- Rendered on one line: stray text nodes between the keys would read as gaps in the combo. -->
<span class={cls}
	>{#each keys as key, i (i)}{#if i > 0 && sep}<span>{sep}</span
			>{/if}<kbd>{key}</kbd>{/each}</span
>
