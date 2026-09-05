<script lang="ts">
	// The open skill, docked at the right of the table the way the board's task detail is
	// docked beside its lanes. It shows what the daemon holds: the name, where the skill
	// came from, its `SKILL.md` rendered by the shared Markdown viewer, and the other
	// files in its folder. Editing swaps the viewer for a textarea; Save writes straight
	// through and takes the saved text back from the server.
	import { Badge, Button, IconButton } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { relativeAge } from '$lib/format';
	import { copyText } from '$lib/shell';
	import { DETAIL_MAX, DETAIL_MIN, sourceLabel, splitFrontmatter } from '$lib/skills';
	import type { Skill } from '$lib/types';
	import MarkdownView from '$lib/ui/MarkdownView.svelte';
	import ResizeBar from '$lib/ui/ResizeBar.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/platform/toasts.svelte';

	interface Props {
		skill: Skill | null;
		loading: boolean;
		error: string | null;
		width: number;
		onclose: () => void;
		onresize: (width: number) => void;
		/** Writes the body through the store; the caller reloads the list. */
		onsave: (id: string, body: string) => Promise<unknown>;
		/** Absent when deleting is not offered, as on a read-only list. */
		ondelete?: (id: string) => Promise<unknown>;
	}

	let { skill, loading, error, width, onclose, onresize, onsave, ondelete }: Props = $props();

	let node = $state<HTMLElement>();
	let editing = $state(false);
	let draft = $state('');
	let saving = $state(false);
	let confirming = $state(false);
	let copied = $state(false);
	let copyTimer: ReturnType<typeof setTimeout> | null = null;

	const canDelete = $derived(!!ondelete && !!skill && skill.source === 'native');

	// A second skill picked while the first is being edited refills the panel rather than
	// leaving the previous body in the textarea under the new name.
	$effect(() => {
		const id = skill?.id;
		if (id === undefined) return;
		editing = false;
		confirming = false;
		draft = '';
	});

	function startEdit(): void {
		if (!skill) return;
		draft = skill.body;
		editing = true;
	}

	function cancelEdit(): void {
		editing = false;
		draft = '';
	}

	async function save(): Promise<void> {
		if (!skill || saving) return;
		saving = true;
		try {
			await onsave(skill.id, draft);
			editing = false;
			push('success', 'Skill saved');
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			saving = false;
		}
	}

	async function remove(): Promise<void> {
		if (!skill || !ondelete) return;
		try {
			await ondelete(skill.id);
			push('success', 'Skill deleted');
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			confirming = false;
		}
	}

	async function copyPath(): Promise<void> {
		if (!skill?.path) return;
		try {
			await copyText(skill.path);
			copied = true;
			if (copyTimer !== null) clearTimeout(copyTimer);
			copyTimer = setTimeout(() => {
				copyTimer = null;
				copied = false;
			}, 1500);
		} catch (e) {
			push('error', `Could not copy: ${errorMessage(e)}`);
		}
	}

	$effect(() => () => {
		if (copyTimer !== null) clearTimeout(copyTimer);
	});
</script>

<aside
	bind:this={node}
	class="detail"
	style="--detail-w:{width}px"
	aria-label="Skill detail"
	data-testid="skill-detail"
>
	<header>
		<span class="name" data-testid="skill-detail-name">{skill?.name ?? 'Skill'}</span>
		{#if skill}
			<Badge variant="outline" title={skill.plugin ?? undefined} data-testid="skill-detail-source">
				{sourceLabel(skill.source)}
			</Badge>
		{/if}
		<span class="spacer"></span>
		<IconButton icon="x" label="Close skill detail" onclick={onclose} data-testid="skill-detail-close" />
	</header>

	<div class="body">
		{#if error}
			<p class="bad" role="alert" data-testid="skill-detail-error">{error}</p>
		{:else if loading && !skill}
			<span class="hint">Loading…</span>
		{:else if !skill}
			<span class="hint">Pick a skill to read it.</span>
		{:else}
			<p class="description">{skill.description}</p>

			{#if skill.path}
				<div class="path-row">
					<span class="mono path" title={skill.path}>{skill.path}</span>
					<Button size="sm" variant="ghost" data-testid="skill-detail-copy-path" onclick={copyPath}>
						{copied ? 'Copied' : 'Copy path'}
					</Button>
				</div>
			{/if}

			<div class="meta">
				<span class="hint">Scope {skill.scope}</span>
				{#if skill.plugin}<span class="hint">Plugin {skill.plugin}</span>{/if}
				{#if skill.updated_at}
					<span class="hint">Updated {relativeAge(skill.updated_at)} ago</span>
				{/if}
				{#if !skill.editable}<span class="hint">Read only</span>{/if}
			</div>

			<div class="actions">
				{#if skill.editable && !editing}
					<Button size="sm" data-testid="skill-edit" onclick={startEdit}>Edit</Button>
				{/if}
				{#if editing}
					<Button
						size="sm"
						variant="primary"
						data-testid="skill-save"
						disabled={saving}
						onclick={save}
					>
						{saving ? 'Saving…' : 'Save'}
					</Button>
					<Button size="sm" variant="ghost" data-testid="skill-cancel" onclick={cancelEdit}>
						Cancel
					</Button>
				{/if}
				<span class="spacer"></span>
				{#if canDelete}
					{#if confirming}
						<span class="hint">Delete this skill?</span>
						<Button size="sm" variant="danger" data-testid="skill-delete-confirm" onclick={remove}>
							Delete
						</Button>
						<Button size="sm" variant="ghost" onclick={() => (confirming = false)}>Keep</Button>
					{:else}
						<Button
							size="sm"
							variant="ghost"
							data-testid="skill-delete"
							onclick={() => (confirming = true)}
						>
							Delete…
						</Button>
					{/if}
				{/if}
			</div>

			{#if editing}
				<Textarea
					bind:value={draft}
					mono
					rows={20}
					aria-label="Skill body"
					data-testid="skill-body-editor"
				/>
			{:else}
				<div class="markdown">
					<!-- Preview skips the frontmatter: the name and the description it holds are
					     already in this panel's header, and rendering the block prints it as a
					     paragraph running into the first heading. Source still shows the file. -->
					<MarkdownView
						source={skill.body}
						preview={splitFrontmatter(skill.body).markdown}
						path={skill.path ?? undefined}
					/>
				</div>
			{/if}

			{#if skill.files.length > 0}
				<div class="files" data-testid="skill-files">
					<span class="group-heading">Files</span>
					{#each skill.files as file (file)}
						<span class="mono file">{file}</span>
					{/each}
				</div>
			{/if}
		{/if}
	</div>

	<ResizeBar
		label="Resize skill detail"
		value={width}
		min={DETAIL_MIN}
		max={DETAIL_MAX}
		side="left"
		onlive={(w) => node?.style.setProperty('--detail-w', `${w}px`)}
		{onresize}
		testid="skill-detail-resize"
	/>
</aside>

<style>
	.detail {
		position: relative;
		width: var(--detail-w, 380px);
		flex: 0 0 var(--detail-w, 380px);
		display: flex;
		flex-direction: column;
		min-height: 0;
		border: 1px solid var(--border-subtle);
		border-radius: 3px;
		background: var(--bg-raised);
	}

	header {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 32px;
		flex: 0 0 32px;
		padding: 0 8px 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.name {
		font-weight: 600;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.spacer {
		flex: 1;
	}

	.body {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 12px;
	}

	.description {
		margin: 0;
		color: var(--text-secondary);
	}

	.path-row {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.path {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--text-tertiary);
	}

	.meta {
		display: flex;
		flex-wrap: wrap;
		gap: 12px;
	}

	.actions {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	.markdown {
		flex: 0 0 auto;
		border: 1px solid var(--border-subtle);
		border-radius: 3px;
		overflow: hidden;
	}

	.files {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	.file {
		color: var(--text-tertiary);
	}

	.mono {
		font-family: var(--font-mono);
		font-size: 12px;
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}
</style>
