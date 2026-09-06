// Skills state, shared by the `/skills` route, the project Skills tab and the Skills
// side panel, so a filter set in the panel and the counts it shows come from the same
// list the table draws. The list is per scope: loading a project's skills replaces the
// global list, because the two views are never on screen at the same time.
//
// Practices ride in the same list as rows with source `practice` (2026-09-07): they are
// fetched beside the skills and folded in client-side, opened from the docs already
// loaded, and written through the practice routes. The daemon's skill list is untouched.

import { api } from '$lib/daemon.svelte';
import { errorMessage } from '$lib/errors';
import { persistSet } from '$lib/platform/persist';
import {
	clampDetail,
	DETAIL_DEFAULT,
	DETAIL_KEY,
	isPracticeId,
	practiceName,
	practiceRow,
	practiceSkill,
	type OpenSkill,
	type SkillRow,
	type SourceFilter
} from '$lib/skills';
import type { Doc, NewDoc, NewSkill, Skill, Uuid } from '$lib/types';

export const skills = $state({
	/** Skills first, then the practices as rows of their own. */
	items: [] as SkillRow[],
	/** The practices behind the practice rows, as the daemon answered them. */
	practices: [] as Doc[],
	warnings: [] as string[],
	loading: false,
	error: null as string | null,
	/** The project the list was loaded for, or null for the global list. */
	projectId: null as Uuid | null,
	/** The Source select, shared with the side panel's SOURCES rows. */
	source: 'all' as SourceFilter,
	/** The open skill in the detail panel, and its own load state. */
	open: null as OpenSkill | null,
	openLoading: false,
	openError: null as string | null,
	detailWidth: loadDetailWidth()
});

export async function loadSkills(projectId: Uuid | null = null): Promise<void> {
	skills.loading = true;
	skills.projectId = projectId;
	try {
		const [list, practices] = await Promise.all([
			api().listSkills(projectId),
			api().listDocs('practice', projectId)
		]);
		skills.practices = practices;
		skills.items = [...list.skills, ...practices.map(practiceRow)];
		skills.warnings = list.warnings;
		skills.error = null;
	} catch (e) {
		skills.items = [];
		skills.practices = [];
		skills.warnings = [];
		skills.error = errorMessage(e);
	} finally {
		skills.loading = false;
	}
}

/** The practice a row id names, from the last load. */
function practiceFor(id: string): Doc | undefined {
	return skills.practices.find((d) => d.name === practiceName(id));
}

/** Opens one skill in the detail panel, fetching its body and file list. */
export async function openSkill(id: string, projectId: Uuid | null = null): Promise<void> {
	if (isPracticeId(id)) {
		const doc = practiceFor(id);
		skills.open = doc ? practiceSkill(doc) : null;
		skills.openError = doc ? null : `No practice named ${practiceName(id)}`;
		skills.openLoading = false;
		return;
	}
	skills.openLoading = true;
	skills.openError = null;
	try {
		skills.open = await api().getSkill(id, projectId);
	} catch (e) {
		skills.open = null;
		skills.openError = errorMessage(e);
	} finally {
		skills.openLoading = false;
	}
}

export function closeSkill(): void {
	skills.open = null;
	skills.openError = null;
}

/**
 * Edit in place. The daemon answers with the saved skill, so the panel and the row both
 * take their new text from the server rather than from the textarea. The project the
 * list was loaded for goes with the write: a project-scoped discovered skill's id is
 * resolved against that project's roots, so without it the daemon cannot find the file.
 */
export async function saveBody(id: string, body: string): Promise<OpenSkill> {
	if (isPracticeId(id)) {
		const doc = practiceFor(id);
		if (!doc) throw new Error(`No practice named ${practiceName(id)}`);
		const saved = await api().saveDoc('practice', { name: doc.name, body, tags: doc.tags, project_id: doc.project_id });
		skills.open = practiceSkill(saved);
		await loadSkills(skills.projectId);
		return skills.open;
	}
	const saved = await api().updateSkillBody(id, body, skills.projectId);
	skills.open = saved;
	await loadSkills(skills.projectId);
	return saved;
}

export async function createSkill(input: NewSkill): Promise<Skill> {
	const created = await api().createSkill(input);
	await loadSkills(skills.projectId);
	return created;
}

/** Creates (or replaces) a practice and reloads, so it appears as a row. */
export async function createPractice(input: NewDoc): Promise<Doc> {
	const created = await api().saveDoc('practice', input);
	await loadSkills(skills.projectId);
	return created;
}

export async function deleteSkill(id: string): Promise<void> {
	if (isPracticeId(id)) await api().deleteDoc('practice', practiceName(id));
	else await api().deleteSkill(id);
	if (skills.open?.id === id) closeSkill();
	await loadSkills(skills.projectId);
}

/** Writes the project's whole disabled list, then reloads so `enabled_here` is the
 * daemon's answer rather than an optimistic guess. */
export async function setDisabled(projectId: Uuid, ids: string[]): Promise<void> {
	await api().setProjectSkills(projectId, ids);
	await loadSkills(projectId);
}

/** The ids the project has turned off right now, read off the loaded skill rows. */
export function disabledIds(): string[] {
	return skills.items.filter((s) => s.source !== 'practice' && s.enabled_here === false).map((s) => s.id);
}

export function setSource(source: SourceFilter): void {
	skills.source = source;
}

export function setDetailWidth(width: number): void {
	skills.detailWidth = clampDetail(width);
	saveDetailWidth(skills.detailWidth);
}

function loadDetailWidth(): number {
	try {
		if (typeof localStorage === 'undefined') return DETAIL_DEFAULT;
		const raw = localStorage.getItem(DETAIL_KEY);
		if (raw === null) return DETAIL_DEFAULT;
		return clampDetail(Number(raw));
	} catch {
		return DETAIL_DEFAULT;
	}
}

function saveDetailWidth(width: number): void {
	try {
		if (typeof localStorage !== 'undefined') localStorage.setItem(DETAIL_KEY, String(width));
	} catch {
		/* a webview with storage denied still resizes, it just forgets */
	}
	void persistSet(DETAIL_KEY, width);
}
