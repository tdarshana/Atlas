// What the installed plugins add to the app, gathered into one shape the shell can read.
//
// Only a plugin that is both enabled and compatible contributes anything: a disabled one
// is inert, and an incompatible one may not even have a manifest to read.

import type {
	CommandContribution,
	Component,
	Permission,
	PluginInfo,
	Section,
	Slot,
	ThemeContribution,
	ToolContribution
} from './types';

/** Every ref carries the plugin it came from, so the shell can route back to it. The
 * name rides along because a contribution is labelled with it wherever it surfaces (a
 * palette command, a theme option), and those labels are built from the refs alone. */
export type Ref<T> = T & { pluginId: string; pluginName: string };

export type SectionRef = Ref<Section>;
export type ComponentRef = Ref<Component>;
export type CommandRef = Ref<CommandContribution>;
export type ThemeRef = Ref<ThemeContribution>;
export type ToolRef = Ref<ToolContribution>;

export interface Contributions {
	sections: SectionRef[];
	/** Keyed by slot; a slot no plugin fills is absent. */
	components: Partial<Record<Slot, ComponentRef[]>>;
	commands: CommandRef[];
	themes: ThemeRef[];
	tools: ToolRef[];
}

/** The plugins whose contributions count. */
export function activePlugins(items: PluginInfo[]): PluginInfo[] {
	return items.filter((p) => p.enabled && p.compatible && p.manifest !== null);
}

export function collectContributions(items: PluginInfo[]): Contributions {
	const out: Contributions = { sections: [], components: {}, commands: [], themes: [], tools: [] };

	for (const plugin of activePlugins(items)) {
		const pluginId = plugin.id;
		const contributes = plugin.manifest?.contributes;
		if (!contributes) continue;
		const pluginName = plugin.manifest?.name ?? pluginId;
		const from = { pluginId, pluginName };
		// Three of the five contribution kinds need a permission the manifest declares, and
		// the user can take any of those back on the Permissions view. Revoking `ui.sections`
		// has to hide the section, not just uncheck a box, so the grants gate the collector
		// the same way they gate the bridge.
		const holds = (permission: Permission) => (plugin.granted ?? []).includes(permission);

		if (holds('ui.sections')) {
			for (const section of contributes.sections ?? []) out.sections.push({ ...section, ...from });
		}
		for (const command of contributes.commands ?? []) out.commands.push({ ...command, ...from });
		for (const theme of contributes.themes ?? []) out.themes.push({ ...theme, ...from });
		if (holds('mcp.tools')) {
			for (const tool of contributes.tools ?? []) out.tools.push({ ...tool, ...from });
		}
		if (holds('ui.components')) {
			for (const component of contributes.components ?? []) {
				const bucket = (out.components[component.slot] ??= []);
				bucket.push({ ...component, ...from });
			}
		}
	}

	return out;
}

/** The route a contributed section opens. */
export function sectionHref(section: SectionRef): string {
	return `/plugins/${encodeURIComponent(section.pluginId)}/${encodeURIComponent(section.view)}`;
}
