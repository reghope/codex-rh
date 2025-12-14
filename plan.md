# Codex Plan Mode — Product + Execution Spec

## 1) What Plan Mode is

Plan Mode is an execution strategy:

- Codex drafts a plan.
- Codex asks a small set of structured questions to lock intent.
- Codex immediately begins executing the plan.
- During execution, Codex can ask follow-up questions only when it hits new ambiguity.
- Codex continues seamlessly.

## 2) UX Surface Area

### 2.1 Entry points into Plan Mode

Codex must support all entry points below.

#### A) Slash command

`/plan`

- If currently not in plan mode: enter Plan Mode (sticky).
- If already in Plan Mode: regenerate plan from current context (does not reset progress unless user asks).

#### B) Mode toggle under the input field

Under the input box, show a mode pill/toggle:

- `Plan mode` (toggle on/off)
- When ON, it is visually “active” and persists for the session (optionally persisted per-repo).
- Placement: directly underneath the input, aligned left, next to other small controls (e.g., “Auto-run”, “Verbose”, etc. if added later).
- TUI copy (when ON): `Plan mode on (shift+tab to toggle)`

#### C) Keyboard toggle

`Shift+Tab` cycles modes:

- `Normal → Plan → Auto → Normal` (or just `Normal ↔ Plan` if only shipping two initially).
- `Shift+Tab` is a mode toggle only; it must **not** submit the current input.
- This is critical for “Claude Code muscle memory”.

## 3) Mode Semantics

### 3.1 Normal mode

Responds and executes as usual.

### 3.2 Plan mode (this spec)

- Always starts with: `plan → questions → execute`.
- Allows runtime interrupts (follow-up questions) only when needed.

### 3.3 Auto mode (optional later)

- Fewer prompts, more autonomy, same underlying architecture.

## 4) Interaction Loop (Plan Mode)

### 4.1 State machine

1. **Context scan**
2. **Draft plan + identify decision points**
3. **Ask question batch #1** (1–5 questions; round 1 of 1–5)
4. **Patch plan + print decision ledger**
5. **Execute steps + checkpoints**
6. If ambiguity/failure introduces a branch: **ask batch N** (1–5 questions; up to 5 total rounds) **→ continue**
7. **Finish summary**

## 5) Plan Output Format

When starting Plan Mode (via `/plan`, or when the user sends their next message while Plan Mode is enabled), Codex prints:

- **Goal** (1–2 lines)
- **Plan** (numbered steps)
- **Decision points** (what it needs from you)
- **Checkpoints** (tests/build/lint, smoke checks)
- **Rollback** (how it reverts if a step fails)

The plan must be actionable, not a wall of text.

### 5.1 Suggested template (rendered in chat/terminal)

```text
Goal
<1–2 lines>

Plan
1. ...
2. ...
3. ...

Decision points
1) **Label** (single-select|multi-select): <question prompt>
   1. <option title>
      <option description (optional)>
   2. ...
   5. (None) Type your answer
      Next
2) ...

Checkpoints
- ...
- ...

Rollback
- ...
```

## 6) Questions UI + Behavior

### 6.1 Questions are decision points

Codex asks questions only when answers materially change:

- approach/architecture
- scope
- compatibility promises
- migration strategy
- safety tradeoffs

### 6.1.1 Rounds (required)

- Questions are asked in **rounds**.
- A single Plan Mode run uses **1–5 rounds total**.
- Each round contains **1–5 questions**.

### 6.2 Question types

- **Single-select** (mutually exclusive)
- **Multi-select** (additive)
- **Free text** (rare)

### 6.3 Answer UX (terminal + chat friendly)

To avoid inventing new keybindings, answers are typed:

- Single-select: `1`
- Multi-select: `1,3,4`
- Free text: normal input

### 6.3.1 “(None) Type your answer” option (required)

- For **single-select** and **multi-select** questions, the **last option** is always: `(None) Type your answer`.
- The option list is capped at **5 total options**, so there are at most **4 predefined** options plus the final `(None) Type your answer`.
- When choosing `(None) Type your answer`, the user answers with **free text** (i.e., normal input).

### 6.4 Decision ledger (required)

After answers, Codex prints:

- **Decisions**
  - `X: …`
  - `Y: …`
- **Plan updates**
  - `Step 2 changed: …`
  - `Added Step 4: …`

Then it immediately begins execution.

## 7) Execution Engine Requirements

### 7.1 Executable plan graph

Represent the plan as nodes with:

- `step_id`, `description`
- `actions` (edit/run tools)
- `preconditions`, `postconditions`
- `checkpoint` (validation)
- `on_fail` (fallback or ask question)

### 7.2 Seamless continuation

Execution runs in slices:

`do work → checkpoint → summarize → next step`

### 7.3 Runtime question injection (“interrupts”)

If Codex hits a fork mid-run (tests failing, missing env, ambiguous target), it:

1. Summarizes the situation in 1–3 lines.
2. Asks 1–5 high-leverage questions (counting as another round; up to 5 total rounds).
3. Updates the plan graph.
4. Continues from the correct step.

## 8) Controls and Discoverability

### 8.1 Inline mode indicator

When Plan Mode is enabled:

- Under input: `Plan mode on (shift+tab to toggle)`
- In the transcript header (optional): show a small badge `PLAN`

### 8.2 `/mode` command (optional but useful)

- `/mode plan`
- `/mode normal`
- `/mode auto`

### 8.3 `/plan` behavior details

If invoked while executing:

- Default: “re-plan from current state” (keep completed steps).
- Do not undo work unless user rewinds.

## 9) Minimum acceptance criteria (MVP)

Plan Mode is “done” when:

- User can enter it via:
  - `/plan`
  - the Plan mode toggle under input
  - `Shift+Tab` cycling
- When starting Plan Mode (`/plan`, or the first user message while Plan Mode is enabled), Codex:
  - prints a plan
  - asks ≤5 structured questions
  - starts execution automatically after answers
- If new ambiguity appears mid-run:
  - Codex can ask 1–5 follow-up rounds, each with 1–5 questions, and continues
- Plan Mode is always clearly visible (toggle + badge)

## 10) Engineering execution plan (how to build this)

This section is an implementation-oriented breakdown for the Codex CLI codebase.

### 10.1 Data model (core)

- Add an `InteractionMode` (or similar) enum: `Normal | Plan | Auto`.
- Track per-session mode as sticky state; optionally persist per-repo.
- Introduce an internal “plan graph” structure that can round-trip through:
  - UI rendering (human-readable plan),
  - `turn/plan/updated` (step + status),
  - execution bookkeeping (completed steps, checkpoints, failures).

### 10.2 Orchestration (core)

- **Normal mode**: current behavior unchanged.
- **Plan mode**:
  - Preflight: run Context scan, then ask the model to draft a plan and emit decision points.
  - Wait for user answers before executing.
  - Emit decision ledger + plan updates, then begin execution.
  - During execution, treat failures/ambiguity as “interrupts” that trigger another question round (1–5 questions; up to 5 total rounds), then resume.
- **Re-plan** (`/plan` while already in Plan mode, or during execution):
  - Keep completed steps as completed unless user asks to rewind.
  - Update plan graph + UI plan view; continue at the correct step.

### 10.3 UI (TUI)

- Add a mode pill under the input field:
  - Left-aligned, clearly indicates ON/OFF.
  - Persist across the session; reflect current mode immediately.
- Add `Shift+Tab` to cycle modes (`Normal → Plan → Auto → Normal` or `Normal ↔ Plan` for MVP).
- Add a transcript/header badge `PLAN` when Plan Mode is active.
- Add `/plan` command:
  - When idle: enter Plan Mode and trigger the initial plan → questions step.
  - When already active: re-plan from current state (do not reset progress).

### 10.4 Structured questions (render + parsing)

- Render questions as numbered items with explicit option lists (max 5 options; last option is always `(None) Type your answer`).
- Parse answers from user input (per question type):
  - Single-select: `1`
  - Multi-select: `1,3,4` (with whitespace tolerated)
  - `(None) Type your answer`: for select questions, any non-numeric input is treated as free text
  - Free text: pass through verbatim (including numeric)
- Validate and re-prompt on invalid answers (stay in Plan Mode, do not start execution).
- Emit the decision ledger as a stable, skimmable block.

### 10.5 Checkpoints

- Every plan step has a checkpoint definition:
  - tests/build/lint/smoke check, or “no-op” when not applicable.
- Default checkpointing can be conservative (run nothing) unless the plan step explicitly calls for it; the point is to make checkpoints visible and intentional.

### 10.6 Tests

- Unit tests for:
  - mode cycling (incl. `Shift+Tab`)
  - `/plan` behavior (enter vs re-plan)
  - answer parsing + validation for questions
- Snapshot tests for:
  - mode pill rendering
  - plan output formatting
  - decision ledger rendering

### 10.7 Docs

- Add `/plan` (and `/mode` if shipped) to `docs/slash_commands.md`.
- Document Plan Mode semantics and answer format in an appropriate docs page (new or existing).

## 11) Open questions (resolve during implementation)

- What should `/plan` do when there is no obvious “current goal” in context (fresh session / no pending task)?
  - Option A: require `/plan <goal…>` (argument form).
  - Option B: ask a single free-text goal question first, then continue with structured questions.
- Do we ship `Auto` mode in the first iteration, or only `Normal ↔ Plan`?
- Should Plan Mode persistence be:
  - session-only,
  - per-repo default in config,
  - or both (session overrides repo default)?
