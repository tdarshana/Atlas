// Pure helpers for the project Agents tab (frame 02.4).
//
// `Agent` carries no `project_id` (unlike `Doc`), so there is no real way to scope an
// agent to a project. The convention here, matching the tag the design already uses to
// mark a memory as this project's own, is a tag equal to the project's name: "project
// agents" are every agent carrying that tag. This is a documented gap, not a daemon
// feature; see the task report.

import type { Agent } from '$lib/types';

export function projectAgents(all: Agent[], projectName: string): Agent[] {
	return all.filter((a) => a.tags.includes(projectName));
}

/** Adds the project's tag if it is not already there, without duplicating it. */
export function withProjectTag(tags: string[], projectName: string): string[] {
	return tags.includes(projectName) ? tags : [...tags, projectName];
}
