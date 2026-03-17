# Spec Refinement Workflow — Design Notes

This document captures the full reasoning and decisions from the design discussion between the engineering director and the AI architect. It is intended to be read by the next worker who will implement this as the first multi-iter Cursus pipeline.

> **Note:** The orchestration layer was originally called "Sequentia" during the design discussion. It was renamed to **Cursus** (Latin: "a running, course, path"). Stages were renamed to **iters** (Latin: "journey, passage"). See the `cursus` fm spec for the full orchestration spec.

## Problem Statement

Specs are the source of truth for the entire project, but the current spec creation process (`spec.md`) is a single-pass flow: one conversation, one shot at writing the spec, then straight to issue creation. This produces specs with gaps, inconsistencies, lack of cohesion, and poor integrity. There is no self-review, no structured quality gate, and no separation between drafting and approval.

## Design Constraints

1. **Don't waste resources on wrong directions.** The agent should not invest heavily in a full spec draft until alignment is confirmed. The interactive discussion phase is the primary protection against wasted work.
2. **Progressive investment.** Start cheap (conversation), invest more only after shared understanding is established.
3. **Spec quality is paramount.** Specs are THE source of truth. They must be internally consistent, complete, testable, and implementable.
4. **Director review model.** The user is an engineering director. The agent team should bring polished, complete work for review — not rough drafts. But the agent should also not go off and do a bunch of work without first confirming direction.
5. **Issue creation is a separate process.** The spec workflow is purely about getting the spec right. Issue decomposition happens after approval, as its own process.
6. **Cursus is the orchestration layer.** This workflow will be implemented as the first multi-iter Cursus pipeline. Cursus defines iters, prompts, modes, and transitions in TOML (`.sgf/cursus/*.toml`).

## The Workflow: State Machine

```
DISCUSS → DRAFT → REVIEW ⇄ REVISE → APPROVED
```

### DISCUSS (interactive, human-in-the-loop)

**Purpose:** Establish alignment on intent, scope, and key constraints so the agent won't go off the rails during autonomous drafting.

**What happens:**
- The agent interviews the user about what they want to build or change.
- The agent actively probes for the things that typically become gaps: edge cases, error handling, interactions with existing specs, failure modes, scope boundaries.
- The agent reads pertinent existing specs as they come up in conversation.
- Both parties reach shared understanding of what's being built and why.

**Transition:** The user says "go draft it" or the agent proposes that it has enough to work with and the user confirms.

**Key decision:** An outline stage (bullet points before full prose) was considered and rejected. The real cost in spec drafting is the thinking (reading existing specs, cross-referencing, identifying gaps), not the prose writing. An outline does almost as much thinking as a full draft, so the savings from catching a wrong direction at outline stage vs draft stage are minimal. The alignment conversation is sufficient protection against wrong directions.

### DRAFT (autonomous, ralph loop)

**Purpose:** Produce a complete, polished spec that passes internal quality gates.

**What happens:**
- The agent creates or updates the `fm` spec in `draft` status.
- The agent writes full section content — not placeholders, not skeletons.
- The agent self-critiques against the quality checklist (see below) across multiple ralph loop iterations.
- Any open risks or assumptions identified during drafting should be resolved by the agent during this phase — by reading existing code/specs, reasoning through the design, or making and documenting design decisions. These should NOT be deferred to the user.
- The agent iterates internally until the quality checklist passes.
- The agent prepares a presentation artifact (see Presentation Format below).

**Transition:** Quality checklist passes internally → `.ralph-complete`. The next stage presents to the user.

**`fm` status during this phase:** `draft`

### REVIEW (interactive, human-in-the-loop)

**Purpose:** The user reviews the spec and either approves or provides feedback.

**What happens:**
- The agent presents the spec using the Presentation Format (see below).
- The user reviews, asks questions, pokes holes, identifies issues.
- This is a conversation, not a checklist.

**Transition:**
- User approves → APPROVED
- User gives feedback → REVISE

### REVISE (autonomous or interactive, depending on feedback scope)

**Purpose:** Address the user's feedback from review.

**What happens:**
- For small, targeted feedback: the agent may revise live in conversation (interactive).
- For large structural feedback: another autonomous ralph loop.
- After revision, the agent re-runs the quality checklist internally.
- The agent prepares an updated presentation.

**Transition:** Returns to REVIEW.

### APPROVED

**Purpose:** Finalize the spec.

**What happens:**
- Spec status moves from `draft` to `stable` in `fm`.
- The spec is now the source of truth and ready for issue creation (which is a separate process/pipeline).

## Quality Checklist (Internal — Not a Deliverable)

The agent must verify all of these during the DRAFT and REVISE phases. This checklist is internal discipline — the user does not see a checklist report. The quality of the spec itself is the evidence that the checklist was followed.

1. **Structural completeness**: All required sections have substantive content. No section is boilerplate, placeholder, or a single vague sentence.
2. **Internal consistency**: No section contradicts another. Terminology is used consistently throughout. If a concept is named in one section, the same name is used everywhere.
3. **Testability**: The feature can be end-to-end tested from the CLI. The testing approach is clear and concrete.
4. **Cross-spec coherence**: The spec does not conflict with or duplicate existing specs. Cross-references (`fm ref`) are correct and complete. Interactions with other specs are accounted for.
5. **Edge cases and error handling**: Failure modes are identified. Error handling behavior is specified, not left to the implementer's imagination.
6. **Dependency clarity**: External dependencies are named. Integration points are defined. Message formats, API contracts, configuration are specified where relevant.
7. **Scope boundaries**: Clear what is in scope and out of scope. No ambiguous "maybe" features. An implementer reading this knows exactly what to build and what not to build.
8. **Implementability**: A build agent could pick this spec up with no additional context and produce the correct implementation. No section requires reading the original author's mind.

## Presentation Format (Deliverable to User)

When the spec is ready for review, the agent presents it using this structure:

1. **What we're building**: 2-3 sentence summary of the feature/change.
2. **Key decisions**: The non-obvious choices that were made and why. Design trade-offs and their rationale.
3. **How it fits in**: Which existing specs and crates are affected. How this interacts with the rest of the system.
4. **How it gets tested**: The end-to-end verification story. What CLI commands validate this works.
5. **Out of scope**: Explicitly states what this does NOT cover, confirming boundaries.
6. **What changed**: (Only for revisions to existing specs, not new specs.) Delta from previous spec state.

The goal of the presentation is for the user to quickly confirm: "yes, this is what I wanted." It is an executive briefing, not a spec dump.

## Relationship to Existing System

### What changes from the current `spec.md` flow:
- The single-pass "discuss then write" flow becomes a multi-stage state machine.
- Spec writing and issue creation are decoupled into separate processes.
- The agent self-critiques before presenting to the user.
- The user sees a structured presentation, not raw spec output.
- `fm` `draft` status is actively used as a staging ground.

### What stays the same:
- `fm` remains the spec management tool. All spec mutations go through `fm`.
- `pn` remains the issue tracker. Issue creation still follows the Issue Create Workflow.
- The alignment conversation is still the starting point.
- Specs must still be designed so results can be end-to-end tested from the CLI.
- `sgf <command>` now resolves to cursus definitions (`.sgf/cursus/<command>.toml`). Single-iter cursus definitions replace the old `config.toml` entries. Multi-iter definitions enable the spec refinement workflow.

### `fm` statuses used:
- `draft`: Spec is being worked on (DRAFT, REVISE phases).
- `stable`: Spec has been approved by the user (APPROVED phase).
- `proven`: (Existing status, unchanged.) Spec has been verified against implementation.

## Resolved Design Questions

These were open when the document was first written. All have been resolved in the Cursus spec.

- **Context passing between iters:** Each iter can `produces` a summary file (written by the agent). Subsequent iters `consumes` those files — the cursus runner injects them into the next iter's system prompt via `--append-system-prompt`. Files live at `.sgf/run/<run-id>/context/<key>.md`. No new tooling needed.
- **REVIEW → REVISE → REVIEW loop:** Handled via sentinel-based transitions. The REVIEW iter defines `on_reject = "draft"` and `on_revise = "revise"` in its `[iters.transitions]` table. The REVISE iter defines `next = "review"` to loop back after completion.
- **REVISE mode:** Defined as AFK (`mode = "afk"`) in the cursus TOML. The user can override to interactive via `-i` flag if they want a live conversation instead. This is simpler than conditional mode selection.
- **DISCUSS → DRAFT transition:** Interactive iters with `iterations = 1` are treated as complete when the `cl` session ends (unless a rejection sentinel is present). The DISCUSS iter's prompt instructs the agent to produce a summary file before ending. When the user is done discussing, the session ends, and cursus advances to DRAFT.

## Cursus TOML for This Workflow

The spec refinement pipeline is defined in `.sgf/cursus/spec.toml`:

```toml
description = "Spec creation and refinement"
alias = "s"

[[iters]]
name = "discuss"
prompt = "spec-discuss.md"
mode = "interactive"
produces = "discuss-summary"

[[iters]]
name = "draft"
prompt = "spec-draft.md"
mode = "afk"
iterations = 10
consumes = ["discuss-summary"]
produces = "draft-presentation"
auto_push = true

[[iters]]
name = "review"
prompt = "spec-review.md"
mode = "interactive"
consumes = ["discuss-summary", "draft-presentation"]

  [iters.transitions]
  on_reject = "draft"
  on_revise = "revise"

[[iters]]
name = "revise"
prompt = "spec-revise.md"
mode = "afk"
iterations = 5
consumes = ["discuss-summary", "draft-presentation"]
produces = "draft-presentation"
next = "review"

[[iters]]
name = "approve"
prompt = "spec-approve.md"
mode = "interactive"
consumes = ["draft-presentation"]
```
