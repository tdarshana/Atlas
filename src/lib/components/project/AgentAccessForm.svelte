<script lang="ts">
	// The agent access rules for one project, moved off the Project settings tab and onto
	// the Permissions tab. Three rules, each saying whether the project sets it or inherits
	// the global default, and each able to go back to that default.
	import { Badge, Button, Checkbox, Input } from '$lib/ds';
	import { alwaysAllowed } from './settings/settings';
	import { ruleNote, reviewNote, toAccess, type AccessForm, type ListRule } from './access';
	import type { ProjectAccessReport } from '$lib/types';

	interface Props {
		form: AccessForm;
		/** The project's rules, the global defaults and what they resolve to. Null while the
		 * read is in flight or after it failed, which only costs the inherited notes. */
		report: ProjectAccessReport | null;
	}

	let { form = $bindable(), report }: Props = $props();

	let draft = $state('');

	const trimmed = $derived(draft.trim());
	const canAdd = $derived(!!trimmed && !form.actors.includes(trimmed));

	// The badge and the note describe the rule as it stands in the form, not as the daemon
	// last stored it. They sit right beside the controls that change it, so deriving them
	// from the saved report would leave `Use global default` clearing the ticks while the
	// line above still read `Set on this project: ...`. The defaults still come from the
	// report: those are the daemon's answer, not the user's.
	const edited = $derived(toAccess(form));
	const writers = $derived(report ? ruleNote(edited, report.defaults, 'memory_writers') : null);
	const movers = $derived(report ? ruleNote(edited, report.defaults, 'task_movers') : null);
	const review = $derived(report ? reviewNote(edited, report.defaults) : null);

	function tick(rule: ListRule, actor: string, on: boolean) {
		if (rule === 'memory_writers') form.memoryWriters = { ...form.memoryWriters, [actor]: on };
		else form.taskMovers = { ...form.taskMovers, [actor]: on };
	}

	/** A label Atlas has not seen write to this project yet, added by hand. Both rules take
	 * it: a label that is ticked nowhere would be added and then vanish on the next load. */
	function add() {
		if (!canAdd) return;
		form.actors = [...form.actors, trimmed].sort((a, b) => a.localeCompare(b));
		tick('memory_writers', trimmed, true);
		tick('task_movers', trimmed, true);
		draft = '';
	}
</script>

<section class="card" data-testid="agent-access-form">
	<header><span class="title">Agent access</span></header>
	<div class="body">
		<span class="sentence">
			Which agents may write memories and move tasks in this project. A rule left on the
			global default follows whatever Settings says.
		</span>

		{#each [{ rule: 'memory_writers', label: 'Memory writers', note: writers, inherit: form.inheritWriters }, { rule: 'task_movers', label: 'Task movers', note: movers, inherit: form.inheritMovers }] as const as row (row.rule)}
			<div class="rule" data-testid="rule-{row.rule}">
				<div class="rule-head">
					<span class="rule-title">{row.label}</span>
					<Badge tone={row.note?.inherited ? 'neutral' : 'accent'}>
						{row.note?.inherited ? 'Inherited' : 'Set here'}
					</Badge>
					<span class="spacer"></span>
					{#if !row.inherit}
						<Button
							size="sm"
							data-testid="use-default-{row.rule}"
							onclick={() => {
								if (row.rule === 'memory_writers') form.inheritWriters = true;
								else form.inheritMovers = true;
							}}
						>
							Use global default
						</Button>
					{/if}
				</div>
				{#if row.note}
					<span class="hint" data-testid="note-{row.rule}">{row.note.text}</span>
				{/if}
				{#if row.inherit}
					<Checkbox
						label="Set this rule on the project instead"
						checked={false}
						data-testid="set-here-{row.rule}"
						onchange={() => {
							if (row.rule === 'memory_writers') form.inheritWriters = false;
							else form.inheritMovers = false;
						}}
					/>
				{:else}
					<div class="ticks">
						{#each form.actors as actor (actor)}
							<div class="tick">
								<Checkbox
									label={actor}
									checked={(row.rule === 'memory_writers' ? form.memoryWriters : form.taskMovers)[actor] ?? false}
									onchange={(e) => tick(row.rule, actor, e.currentTarget.checked)}
								/>
								<!-- The daemon exempts the user's own hands from both lists, so a tick
								     against one of them changes nothing; say so rather than imply a rule. -->
								{#if alwaysAllowed(actor)}<span class="always">always allowed</span>{/if}
							</div>
						{/each}
					</div>
				{/if}
			</div>
		{/each}

		<div class="add">
			<Input
				placeholder="Add label"
				aria-label="Add label"
				mono
				bind:value={draft}
				onkeydown={(e) => {
					if (e.key === 'Enter') add();
				}}
			/>
			<Button size="sm" data-testid="access-add" disabled={!canAdd} onclick={add}>Add</Button>
		</div>

		<div class="rule" data-testid="rule-require_review">
			<div class="rule-head">
				<span class="rule-title">Require review</span>
				<Badge tone={review?.inherited ? 'neutral' : 'accent'}>
					{review?.inherited ? 'Inherited' : 'Set here'}
				</Badge>
				<span class="spacer"></span>
				{#if form.requireReview}
					<Button
						size="sm"
						data-testid="use-default-require_review"
						onclick={() => (form.requireReview = false)}
					>
						Use global default
					</Button>
				{/if}
			</div>
			{#if review}
				<span class="hint" data-testid="note-require_review">{review.text}</span>
			{/if}
			<Checkbox
				label="Require review for memories proposed by agents"
				bind:checked={form.requireReview}
			/>
		</div>
	</div>
</section>

<style>
	/* The pane scrolls, not the card: a card that shrank would clip the row the user
	   came here to change. */
	.card {
		display: flex;
		flex-direction: column;
		flex: 0 0 auto;
	}

	header {
		display: flex;
		align-items: center;
		height: 32px;
		flex: 0 0 32px;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.title {
		font-weight: 600;
	}

	.body {
		display: flex;
		flex-direction: column;
		gap: 12px;
		padding: 12px;
	}

	.sentence {
		color: var(--text-secondary);
		max-width: 80ch;
	}

	.rule {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.rule-head {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.rule-title {
		font-weight: 600;
	}

	.spacer {
		flex: 1;
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.ticks {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.tick {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.always {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	/* An input and a button, not a bar: the button is its own width. */
	.add {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.add :global(.dbm-input) {
		width: 220px;
	}
</style>
