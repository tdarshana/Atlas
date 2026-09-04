<script lang="ts">
	// The MCP tool half of the plugin host, mounted once in the app's layout.
	//
	// A plugin that contributes MCP tools has to be able to answer an agent at any moment,
	// not only while one of its views happens to be on screen. So each such plugin gets one
	// hidden frame here, running its own code with nothing painted, and the daemon's
	// forwarded calls are routed to that frame's bridge.
	//
	// Nothing renders for a plugin without tools, and nothing at all renders outside Tauri,
	// where there is no plugin store and no daemon socket to speak of.
	import { onMount } from 'svelte';
	import { push } from '$lib/ui/toasts.svelte';
	import PluginFrame from './PluginFrame.svelte';
	import { BACKGROUND_VIEW, callPluginTool, loadPlugins, plugins } from './host.svelte';
	import {
		reportChannelError,
		setChannelConnected,
		setChannelWanted
	} from './tool-channel.svelte';
	import {
		channelUrl,
		createToolChannel,
		deletePluginTools,
		putPluginTools,
		toolPlugins,
		toolRegistry,
		type ToolChannel,
		type ToolSocket
	} from './tools';

	const targets = $derived(plugins.available ? toolPlugins(plugins.items) : []);

	let channel: ToolChannel | null = null;

	function openChannel(): ToolChannel {
		setChannelWanted(true);
		return createToolChannel({
			url: channelUrl(),
			socketFactory: (url) => new WebSocket(url) as unknown as ToolSocket,
			registry: () => toolRegistry(plugins.items),
			register: putPluginTools,
			unregister: deletePluginTools,
			onCall: callPluginTool,
			onStatus: setChannelConnected,
			// A refused registration is the plugin's own problem, not the transport's: the
			// plugin is installed and running and its tools will stay missing until its
			// author fixes the manifest, so it is worth a toast rather than a console line.
			onRegisterError: (pluginId, message) => {
				const text = `Plugin ${pluginId}: the daemon refused its MCP tools. ${message}`;
				reportChannelError(text);
				push('error', text);
			},
			onError: (message) => {
				reportChannelError(message);
				console.warn(message);
			}
		});
	}

	function closeChannel(): void {
		channel?.dispose();
		channel = null;
		setChannelWanted(false);
	}

	onMount(() => {
		// A plugin's tools have to reach agents whether or not anyone opened the Plugins
		// page this session, so this is the one surface that reads the list unprompted.
		if (plugins.available && !plugins.loaded && !plugins.loading) void loadPlugins();
		return closeChannel;
	});

	// One socket while any plugin contributes a tool, none otherwise. The plugin list is
	// read through `targets` so this reruns on every install, enable, disable and
	// uninstall; `sync` then sends a `PUT` for each plugin that has tools and a `DELETE`
	// for each that just stopped having them.
	$effect(() => {
		const list = targets;
		if (list.length === 0) {
			closeChannel();
			return;
		}
		channel ??= openChannel();
		void channel.sync();
	});
</script>

{#each targets as plugin (plugin.id)}
	<div class="background" data-testid="plugin-background-{plugin.id}">
		<PluginFrame {plugin} view={BACKGROUND_VIEW} slot="background" height={0} />
	</div>
{/each}

<style>
	/* Out of the layout and out of the accessibility tree, but still a rendered frame:
	   `display: none` would stop the plugin's script running at all. */
	.background {
		position: absolute;
		width: 0;
		height: 0;
		overflow: hidden;
		visibility: hidden;
		pointer-events: none;
	}
</style>
