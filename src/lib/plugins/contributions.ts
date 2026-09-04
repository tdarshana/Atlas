// What the installed plugins add to the app, gathered into one shape the shell can read.
//
// Only a plugin that is both enabled and compatible contributes anything: a disabled one
// is inert, and an incompatible one may not even have a manifest to read.

import type {
	CommandContribution,
	Component,
	PluginInfo,
	Section,
	Slot,
	ThemeContribution,
	ToolContribution
} from './types';

/** Every ref carries the plugin it came from, so the shell can route back to it. */
export type Ref<T> = T & { pluginId: string };

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

		for (const section of contributes.sections ?? []) out.sections.push({ ...section, pluginId });
		for (const command of contributes.commands ?? []) out.commands.push({ ...command, pluginId });
		for (const theme of contributes.themes ?? []) out.themes.push({ ...theme, pluginId });
		for (const tool of contributes.tools ?? []) out.tools.push({ ...tool, pluginId });
		for (const component of contributes.components ?? []) {
			const bucket = (out.components[component.slot] ??= []);
			bucket.push({ ...component, pluginId });
		}
	}

	return out;
}

/** The route a contributed section opens. */
export function sectionHref(section: SectionRef): string {
	return `/plugins/${encodeURIComponent(section.pluginId)}/${encodeURIComponent(section.view)}`;
}
