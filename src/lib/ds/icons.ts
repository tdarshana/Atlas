/* The Lucide vocabulary Atlas uses, mapped from the design file's icon names to bundled
   components. Nothing is fetched at runtime: the desktop webview runs under a
   `default-src 'self'` CSP, so every glyph is imported and bundled here. */

import type { Component } from 'svelte';

import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
import ArrowDown from '@lucide/svelte/icons/arrow-down';
import ArrowLeft from '@lucide/svelte/icons/arrow-left';
import ArrowRight from '@lucide/svelte/icons/arrow-right';
import ArrowUp from '@lucide/svelte/icons/arrow-up';
import BookOpen from '@lucide/svelte/icons/book-open';
import Bot from '@lucide/svelte/icons/bot';
import Braces from '@lucide/svelte/icons/braces';
import Check from '@lucide/svelte/icons/check';
import ChevronDown from '@lucide/svelte/icons/chevron-down';
import ChevronLeft from '@lucide/svelte/icons/chevron-left';
import ChevronRight from '@lucide/svelte/icons/chevron-right';
import ChevronUp from '@lucide/svelte/icons/chevron-up';
import ChevronsLeft from '@lucide/svelte/icons/chevrons-left';
import ChevronsRight from '@lucide/svelte/icons/chevrons-right';
import Circle from '@lucide/svelte/icons/circle';
import CircleCheck from '@lucide/svelte/icons/circle-check';
import CircleDot from '@lucide/svelte/icons/circle-dot';
import CircleX from '@lucide/svelte/icons/circle-x';
import Clock from '@lucide/svelte/icons/clock';
import Columns3 from '@lucide/svelte/icons/columns-3';
import Copy from '@lucide/svelte/icons/copy';
import Cpu from '@lucide/svelte/icons/cpu';
import Database from '@lucide/svelte/icons/database';
import Ellipsis from '@lucide/svelte/icons/ellipsis';
import ExternalLink from '@lucide/svelte/icons/external-link';
import File from '@lucide/svelte/icons/file';
import Filter from '@lucide/svelte/icons/filter';
import FlaskConical from '@lucide/svelte/icons/flask-conical';
import Folder from '@lucide/svelte/icons/folder';
import Gauge from '@lucide/svelte/icons/gauge';
import GitBranch from '@lucide/svelte/icons/git-branch';
import GitCommitHorizontal from '@lucide/svelte/icons/git-commit-horizontal';
import History from '@lucide/svelte/icons/history';
import Info from '@lucide/svelte/icons/info';
import Layers from '@lucide/svelte/icons/layers';
import ListChecks from '@lucide/svelte/icons/list-checks';
import Loader from '@lucide/svelte/icons/loader';
import Maximize from '@lucide/svelte/icons/maximize';
import Minus from '@lucide/svelte/icons/minus';
import PanelLeft from '@lucide/svelte/icons/panel-left';
import PanelRight from '@lucide/svelte/icons/panel-right';
import Pencil from '@lucide/svelte/icons/pencil';
import Play from '@lucide/svelte/icons/play';
import Plug from '@lucide/svelte/icons/plug';
import Plus from '@lucide/svelte/icons/plus';
import RefreshCw from '@lucide/svelte/icons/refresh-cw';
import Search from '@lucide/svelte/icons/search';
import Settings from '@lucide/svelte/icons/settings';
import Square from '@lucide/svelte/icons/square';
import Tag from '@lucide/svelte/icons/tag';
import Terminal from '@lucide/svelte/icons/terminal';
import Trash2 from '@lucide/svelte/icons/trash-2';
import WandSparkles from '@lucide/svelte/icons/wand-sparkles';
import X from '@lucide/svelte/icons/x';
import Zap from '@lucide/svelte/icons/zap';

/* eslint-disable @typescript-eslint/no-explicit-any */
export const icons = {
	gauge: Gauge,
	folder: Folder,
	'columns-3': Columns3,
	database: Database,
	braces: Braces,
	bot: Bot,
	'book-open': BookOpen,
	'git-branch': GitBranch,
	history: History,
	'list-checks': ListChecks,
	settings: Settings,
	search: Search,
	zap: Zap,
	play: Play,
	plug: Plug,
	cpu: Cpu,
	terminal: Terminal,
	'git-commit-horizontal': GitCommitHorizontal,
	'chevron-down': ChevronDown,
	'chevron-right': ChevronRight,
	'chevron-left': ChevronLeft,
	'chevron-up': ChevronUp,
	'chevrons-left': ChevronsLeft,
	'chevrons-right': ChevronsRight,
	circle: Circle,
	'circle-dot': CircleDot,
	'circle-check': CircleCheck,
	'circle-x': CircleX,
	'flask-conical': FlaskConical,
	'arrow-down': ArrowDown,
	'arrow-up': ArrowUp,
	'arrow-left': ArrowLeft,
	'arrow-right': ArrowRight,
	x: X,
	plus: Plus,
	minus: Minus,
	maximize: Maximize,
	ellipsis: Ellipsis,
	tag: Tag,
	'wand-sparkles': WandSparkles,
	info: Info,
	layers: Layers,
	clock: Clock,
	'panel-left': PanelLeft,
	'panel-right': PanelRight,
	square: Square,
	file: File,
	check: Check,
	copy: Copy,
	'refresh-cw': RefreshCw,
	'trash-2': Trash2,
	pencil: Pencil,
	'external-link': ExternalLink,
	filter: Filter,
	'alert-triangle': AlertTriangle,
	loader: Loader
} satisfies Record<string, Component<any>>;

export type IconName = keyof typeof icons;

const warned = new Set<string>();

/** Resolves an icon name, falling back to `circle` and warning once per unknown name. */
export function resolveIcon(name: string): Component<any> {
	const found = (icons as Record<string, Component<any>>)[name];
	if (found) return found;
	if (!warned.has(name)) {
		warned.add(name);
		console.warn(`[ds] unknown icon name: ${name}`);
	}
	return icons.circle;
}
