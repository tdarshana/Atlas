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
	import { createBridge, type Bridge, type FrameContext, type ThemeTokens } from './bridge';
	import { frameUrl } from './frame-url';
	import { pluginById, registerFrame, unregisterFrame } from './host.svelte';
	import { pluginBackend } from './plugin-api';
	import type { FrameSlot, PluginInfo } from './types';

	interface Props {
		plugin: PluginInfo;
		/** Which of the plugin's views to render. */
		view: string;
		/** The slot, when this frame is a contributed component rather than a section, or
		 * `background` for the hidden frame that answers MCP tool calls. */
		slot?: FrameSlot | null;
		/** A fixed height. Without one the frame follows the plugin's own `atlas.resize`. */
		height?: number;
		/** A ceiling on what the frame may ask for, for a slot with a budget to keep. */
		maxHeight?: number;
		/** Fills its container instead, for a section that owns the whole content panel. */
		fill?: boolean;
		/** What the surface around this frame is showing, e.g. `{ taskKey }`. Sent with the
		 * handshake and again whenever it changes. */
		context?: FrameContext;
	}

	let { plugin, view, slot = null, height, maxHeight, fill = false, context }: Props = $props();

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
	/** The context as last sent, so a parent that hands over a fresh object holding the
	 * same values does not make the frame re-render for nothing. */
	let sentContext = '';

	const src = $derived(platform ? frameUrl(plugin.id, platform) : undefined);
	/**
	 * Which document this frame is showing, as the host asked for it. Any change recreates
	 * the iframe element, and with it the `Window` a bridge binds to.
	 *
	 * `view` is in the key as well as `src`, for two different reasons. `src` covers a
	 * change of plugin. `view` does not appear in the URL at all, since one document serves
	 * every view and the view travels in `atlas:init`, but `createBridge` fixes it at
	 * binding time, so a view change needs a new binding too.
	 *
	 * A page the plugin navigated itself to changes neither, which is exactly why keying on
	 * this keeps the rebind guard's security property: only the host moves this key.
	 */
	const frameKey = $derived(`${src ?? ''}|${view}`);
	const title = $derived(plugin.manifest?.name ?? plugin.id);
	const shownHeight = $derived(Math.min(height ?? reported, maxHeight ?? Number.POSITIVE_INFINITY));
	const frameStyle = $derived(fill ? 'height:100%' : `height:${shownHeight}px`);

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
		// Only the window this iframe is showing right now. A frame the app is not showing
		// is not this plugin.
		if (!frame || source !== frame.contentWindow) return;
		// A same-frame navigation keeps the same `Window` object, so the check above does
		// not exclude a page the frame navigated itself to. Once a bridge is live, a second
		// hello is refused: rebinding would hand the plugin's grants (`memories.remember`,
		// `tasks.move`, `settings.get`, all as `plugin/<id>`) to whatever page the frame
		// walked off to. Only the host resets a binding, by moving `frameKey`, which throws
		// the whole iframe element away and drops the bridge with it.
		if (bridge) {
			console.warn(
				`plugin ${plugin.id}: refused a second atlas:hello from an already-bound frame`
			);
			return;
		}
		bridge = createBridge({
			plugin,
			view,
			slot,
			target: source as unknown as { postMessage(message: unknown, targetOrigin: string): void },
			source,
			api: pluginBackend(plugin.id),
			// The live grants, so revoking one on the Permissions view reaches a frame that
			// is already mounted rather than waiting for it to be recreated.
			grants: () => pluginById(plugin.id)?.granted ?? plugin.granted ?? [],
			actor: `plugin/${plugin.id}`,
			onResize: (h) => (reported = h),
			onNotify: (kind, text) => push(kind, `${title}: ${text}`)
		});
		registerFrame(plugin.id, bridge, view);
		sentContext = JSON.stringify(context ?? {});
		bridge.sendInit(themeTokens(), context);
	}

	function detachBridge(): void {
		if (!bridge) return;
		unregisterFrame(plugin.id, bridge);
		bridge.dispose();
		bridge = null;
	}

	/** The key the live bridge was bound under. Plain, not `$state`: it is a record of what
	 * has happened, never something the template reads. */
	let boundKey = '';

	/**
	 * The host changed which document this frame shows. Drop the bridge in the same flush
	 * the keyed block destroys the iframe in, before the new document exists: `atlas:hello`
	 * arrives on a later task, so the new binding is never the one torn down here.
	 *
	 * Without this the frame is the one host-driven reload the app actually performs, and
	 * it would meet a live bridge: the new document's hello would be refused, nothing would
	 * be themed or initialised, and the previous plugin's bridge would stay attached to the
	 * next plugin's document, which accepts a message on window identity alone.
	 */
	$effect.pre(() => {
		const key = frameKey;
		if (key === boundKey) return;
		boundKey = key;
		detachBridge();
	});

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
			detachBridge();
		};
	});

	// The surface moved on to another subject: tell the frame, so a task detail panel
	// follows the selection instead of showing whichever task was open when it mounted.
	$effect(() => {
		const next = JSON.stringify(context ?? {});
		if (!bridge || next === sentContext) return;
		sentContext = next;
		bridge.sendContext(context ?? {});
	});
</script>

{#if src}
	{#key frameKey}
		<iframe
			bind:this={frame}
			{title}
			{src}
			sandbox="allow-scripts"
			style={frameStyle}
			data-testid="plugin-frame-{plugin.id}"
		></iframe>
	{/key}
{/if}

<style>
	iframe {
		display: block;
		width: 100%;
		border: 0;
		background: transparent;
	}
</style>
