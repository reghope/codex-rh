use crate::status::format_tokens_compact;
use codex_core::protocol::SubAgentActivityKind;
use codex_core::protocol::SubAgentStatus;
use codex_core::protocol::SubAgentUiItem;
use codex_core::protocol::SubAgentsUpdateEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::render::renderable::Renderable;

pub(crate) struct SubAgentsPane<'a> {
    pub(crate) update: &'a SubAgentsUpdateEvent,
    pub(crate) expanded: bool,
    pub(crate) background_mode: bool,
}

impl SubAgentsPane<'_> {
    fn lines(&self) -> Vec<Line<'static>> {
        if self.update.running_count == 0 {
            return Vec::new();
        }

        subagents_tree_lines(self.update, self.expanded, self.background_mode)
    }
}

impl Renderable for SubAgentsPane<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(Text::from(self.lines())).render(area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        self.lines().len().try_into().unwrap_or(u16::MAX)
    }
}

fn subagents_tree_lines(
    update: &SubAgentsUpdateEvent,
    show_transcripts: bool,
    background_mode: bool,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    let failed_count = update
        .agents
        .iter()
        .filter(|agent| agent.status == SubAgentStatus::Failed)
        .count();
    let finished_count = update.created_count.saturating_sub(update.running_count);
    let any_transcripts = update
        .agents
        .iter()
        .any(|agent| !agent.transcript.is_empty() || agent.transcript_truncated);
    let bg_badge = if background_mode {
        "bg:on".cyan().bold()
    } else {
        "bg:off".dim()
    };

    let mut header = Line::from(vec![
        "Subagents ".bold(),
        format!("{}/{}", update.running_count, update.created_count).bold(),
        " running".into(),
    ]);
    if finished_count > 0 {
        header.push_span(" · ".dim());
        header.push_span(format!("{finished_count} finished").dim());
    }
    if failed_count > 0 {
        header.push_span(" · ".dim());
        header.push_span(format!("{failed_count} failed").red().bold());
    }

    header.push_span(" (".dim());
    if any_transcripts {
        header.push_span("ctrl+o".dim());
        header.push_span(if show_transcripts {
            " hide transcripts".dim()
        } else {
            " show transcripts".dim()
        });
        header.push_span(" · ".dim());
    }
    header.push_span("ctrl+b".dim());
    header.push_span(" ".dim());
    header.push_span(bg_badge);
    header.push_span(")".dim());

    lines.push(header);

    let mut agents: Vec<&SubAgentUiItem> = update.agents.iter().collect();
    agents.sort_by_key(|agent| {
        (
            status_sort_rank(agent.status),
            agent.title.as_str(),
            agent.id.as_str(),
        )
    });

    for (idx, agent) in agents.iter().enumerate() {
        let is_last = idx + 1 == agents.len();
        lines.extend(subagent_lines(agent, is_last, show_transcripts));
    }

    lines
}

fn subagent_lines(
    agent: &SubAgentUiItem,
    is_last: bool,
    show_transcripts: bool,
) -> Vec<Line<'static>> {
    let branch = if is_last { "└─ " } else { "├─ " };
    let title = Span::from(agent.title.clone()).dim();
    let title = match agent.status {
        SubAgentStatus::Running => Span::from(agent.title.clone()),
        SubAgentStatus::Failed => Span::from(agent.title.clone()).red().bold(),
        SubAgentStatus::Completed | SubAgentStatus::Canceled => title,
    };

    let mut header = Line::from(vec![
        branch.dim(),
        status_badge(agent.status),
        " ".dim(),
        title,
        format!(" ({})", agent.template).dim(),
    ]);
    header.push_span(" · ".dim());
    header.push_span(format!("{} tools", agent.tool_uses).dim());
    header.push_span(" · ".dim());
    let total_tokens = agent
        .total_tokens
        .map_or_else(|| "?".to_string(), format_tokens_compact);
    header.push_span(format!("{total_tokens} tokens").dim());

    let pipe = if is_last { "   " } else { "│  " };
    let (kind_style, label) = if let Some(activity) = agent.last_activity.as_ref() {
        let kind_style = match activity.kind {
            SubAgentActivityKind::Bash => "Bash".magenta().bold(),
            SubAgentActivityKind::Read => "Read".cyan(),
            SubAgentActivityKind::Mcp => "MCP".green(),
            SubAgentActivityKind::WebSearch => "WebSearch".cyan(),
            SubAgentActivityKind::ApplyPatch => "ApplyPatch".green(),
            SubAgentActivityKind::Other => "Activity".dim(),
        };
        (kind_style, activity.label.clone())
    } else {
        let label = match agent.status {
            SubAgentStatus::Running => "Starting…",
            SubAgentStatus::Completed => "Completed",
            SubAgentStatus::Failed => "Failed",
            SubAgentStatus::Canceled => "Canceled",
        };
        ("Activity".dim(), label.to_string())
    };

    let mut lines = vec![
        header,
        Line::from(vec![
            pipe.dim(),
            "⎿  ".dim(),
            kind_style,
            ": ".dim(),
            label.into(),
        ]),
    ];

    if show_transcripts && (!agent.transcript.is_empty() || agent.transcript_truncated) {
        for line in &agent.transcript {
            lines.push(Line::from(vec![
                pipe.dim(),
                "   ".dim(),
                line.clone().dim(),
            ]));
        }
        if agent.transcript_truncated {
            lines.push(Line::from(vec![
                pipe.dim(),
                "   ".dim(),
                "(older transcript truncated)".dim(),
            ]));
        }
    }

    lines
}

fn status_badge(status: SubAgentStatus) -> Span<'static> {
    match status {
        SubAgentStatus::Running => "RUN".cyan().bold(),
        SubAgentStatus::Completed => "OK".dim(),
        SubAgentStatus::Failed => "FAIL".red().bold(),
        SubAgentStatus::Canceled => "CXL".dim(),
    }
}

fn status_sort_rank(status: SubAgentStatus) -> u8 {
    match status {
        SubAgentStatus::Running => 0,
        SubAgentStatus::Failed => 1,
        SubAgentStatus::Canceled => 2,
        SubAgentStatus::Completed => 3,
    }
}
