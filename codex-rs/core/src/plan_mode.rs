use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::InteractionMode;

const PLAN_MODE_DEVELOPER_INSTRUCTIONS: &str = r#"Plan Mode is enabled.

Plan Mode loop:
- Always start with: Goal (1–2 lines), Plan (numbered steps), Decision points (question round 1 of up to 5 rounds; 1–5 questions), Checkpoints, Rollback.
- Do not call tools or start edits until the user answers the current question round.
- Decision points formatting must be parseable:
  - Use an exact section header line: "Decision points"
  - Each question uses: `N) **Label** (single-select|multi-select): Prompt`
  - Each option uses an indented numbered line: `  N. Option title`
  - If an option needs a description, put it on the next line indented by 5 spaces.
- Questions must be structured and numbered. Each question is single-select or multi-select, with 2–5 total options; the last option is always "(None) Type your answer".
- Answer format: for a round with K questions, the user replies with K lines (one per question, in order). Each line is either:
  - single-select: "1"
  - multi-select: "1,3,4"
  - free text: any non-numeric text (treat as choosing "(None) Type your answer")
- After receiving answers: print a Decision ledger with "Decisions" and "Plan updates", then immediately continue executing the plan.
- During execution: update progress via the update_plan tool; at checkpoints run the planned validations. If new ambiguity/failure requires a fork, ask another question round (still max 5 total rounds), update the plan, and continue.
"#;

pub(crate) fn inject_developer_message(
    mut input: Vec<ResponseItem>,
    mode: InteractionMode,
) -> Vec<ResponseItem> {
    if mode != InteractionMode::Plan {
        return input;
    }

    let insert_at = input
        .iter()
        .position(|item| !matches!(item, ResponseItem::Message { role, .. } if role == "developer"))
        .unwrap_or(input.len());

    input.insert(
        insert_at,
        ResponseItem::Message {
            id: None,
            role: "developer".to_string(),
            content: vec![ContentItem::InputText {
                text: PLAN_MODE_DEVELOPER_INSTRUCTIONS.to_string(),
            }],
        },
    );

    input
}
