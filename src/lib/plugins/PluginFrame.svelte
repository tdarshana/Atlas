<script lang="ts">
	// One plugin, running in a sandboxed iframe.
	//
	// `sandbox="allow-scripts"` without `allow-same-origin` puts the frame in an opaque
	// origin: it can run its own script and nothing else, and it cannot reach into the app
	// or read anything the app stores. Everything it is allowed to do goes through the
	// bridge's postMessage protocol, which checks the manifest's permissions per call.
	//
	// The document itself comes from the `atlas-plugin` scheme rather than a `srcdoc`,
	// because a `srcdoc` inherits the app's CSP and that CSP blocks the plugin's script.
	import { onMount } from 'svelte';
	import type { Platform } from '$lib/ds';
	import { resolvePlatform } from '$lib/shell/platform';
	import { themeTokenAllowlist } from '$lib/shell/theme-pack';
	import { push } from '$lib/ui/toasts.svelte';
	import { createBridge, type Bridge, type ThemeTokens } from './bridge';
	import { frameUrl } from './frame-url';
	import { pluginBackend } from './plugin-api';
	import type { PluginInfo, Slot } from './types';

	interface Props {
		plugin: PluginInfo;
		/** Which of the plugin's views to render. */
		view: string;
		/** The slot, when this frame is a contributed component rather than a section. */
		slot?: Slot | null;
		/** A fixed height. Without one the frame follows the plugin's own `atlas.resize`. */
		height?: number;
		/** Fills its container instead, for a section that owns the whole content panel. */
		fill?: boolean;
	}

	let { plugin, view, slot = null, height, fill = false }: Props = $props();

	/** What a frame is given before it has asked for a height of its own. */
	const DEFAULT_HEIGHT = 120;

	/** Not colour tokens, so not on the theme pack's allow list, but the frame document's
	 * own stylesheet names one of them and a plugin will want both. */
	const FONT_TOKENS = ['--font-ui', '--font-mono'];

	/** The frame document also names `--bg-base`, which the allow list does carry, and
	 * `--color-scheme`, which is not a token at all: it is the app's own light or dark
	 * setting, sent so the frame's form controls and scrollbars match the app's. */
	const EXTRA_TOKENS = ['--bg-base'];

	let platform = $state<Platform | null>(null);
	let frame = $state<HTMLIFrameElement>();
	let reported = $state(DEFAULT_HEIGHT);
	let bridge: Bridge | null = null;

	const src = $derived(platform ? frameUrl(plugin.id, platform) : undefined);
	const title = $derived(plugin.manifest?.name ?? plugin.id);
	const frameStyle = $derived(fill ? 'height:100%' : `height:${height ?? reported}px`);

	/** Every token the app is drawing itself with right now, resolved to a literal value
	 * so the frame can use `var(--accent)` without the app's stylesheet. */
	function themeTokens(): ThemeTokens {
		const computed = getComputedStyle(document.documentElement);
		const tokens: ThemeTokens = {};
		for (const name of [...themeTokenAllowlist(), ...FONT_TOKENS, ...EXTRA_TOKENS]) {
			const value = computed.getPropertyValue(name).trim();
			if (value) tokens[name] = value;
		}
		tokens['--color-scheme'] = document.documentElement.dataset.theme === 'light' ? 'light' : 'dark';
		return tokens;
	}

	/**
	 * The frame speaks first. Waiting for the iframe's own `load` event raced the plugin:
	 * the plugin's script had already run and painted by the time `load` fired, so the
	 * first frame was drawn before any theme token arrived. The client posts `atlas:hello`
	 * the moment it runs, and buffers its own calls until init comes back.
	 */
	function onHello(source: MessageEventSource): void {
		// Only the window this iframe is showing right now. Any other window saying hello,
		// including one from a page the frame navigated itself to, is not this plugin.
		if (!frame || source !== frame.contentWindow) return;
		bridge?.dispose();
		bridge = createBridge({
			plugin,
			view,
			slot,
			target: source as unknown as { postMessage(message: unknown, targetOrigin: string): void },
			source,
			api: pluginBackend(plugin.id),
			actor: `plugin/${plugin.id}`,
			onResize: (h) => (reported = h),
			onNotify: (kind, text) => push(kind, `${title}: ${text}`)
		});
		bridge.sendInit(themeTokens());
	}

	onMount(() => {
		void resolvePlatform().then((p) => (platform = p));

		const onMessage = (event: MessageEvent) => {
			const data = event.data as { type?: unknown } | null;
			if (data && typeof data === 'object' && data.type === 'atlas:hello') {
				if (event.source) onHello(event.source);
				return;
			}
			bridge?.handle(event);
		};
		window.addEventListener('message', onMessage);

		// The theme lives on the root element: `data-theme` for the light/dark switch and
		// `style` for an imported theme pack's token overrides.
		const observer = new MutationObserver(() => bridge?.sendTheme(themeTokens()));
		observer.observe(document.documentElement, {
			attributes: true,
			attributeFilter: ['data-theme', 'style']
		});

		return () => {
			observer.disconnect();
			window.removeEventListener('message', onMessage);
			bridge?.dispose();
			bridge = null;
		};
	});
</script>

{#if src}
	<iframe
		bind:this={frame}
		{title}
		{src}
		sandbox="allow-scripts"
		style={frameStyle}
		data-testid="plugin-frame-{plugin.id}"
	></iframe>
{/if}

<style>
	iframe {
		display: block;
		width: 100%;
		border: 0;
		background: transparent;
	}
</style>
