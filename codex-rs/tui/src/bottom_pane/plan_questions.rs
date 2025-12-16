use std::cell::RefCell;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::scroll_state::ScrollState;
use crate::bottom_pane::textarea::TextArea;
use crate::bottom_pane::textarea::TextAreaState;
use crate::render::renderable::Renderable;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::StatefulWidgetRef;
use ratatui::widgets::Widget;

use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;

use super::CancellationEvent;
use super::bottom_pane_view::BottomPaneView;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QuestionKind {
    SingleSelect,
    MultiSelect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct QuestionOption {
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) is_free_text: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlanQuestion {
    pub(crate) label: String,
    pub(crate) prompt: String,
    pub(crate) kind: QuestionKind,
    pub(crate) options: Vec<QuestionOption>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlanQuestionRound {
    pub(crate) questions: Vec<PlanQuestion>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct QuestionAnswer {
    selected_option_indices: Vec<usize>,
    free_text: Option<String>,
}

pub(crate) struct PlanQuestionsView {
    app_event_tx: AppEventSender,
    round: PlanQuestionRound,
    active_tab: usize,
    state: ScrollState,
    answers: Vec<QuestionAnswer>,
    complete: bool,
    error: Option<String>,
    free_text_editor: TextArea,
    free_text_state: RefCell<TextAreaState>,
    is_editing_free_text: bool,
}

impl PlanQuestionsView {
    pub(crate) fn new(round: PlanQuestionRound, app_event_tx: AppEventSender) -> Self {
        let mut state = ScrollState::new();
        let initial_len = round
            .questions
            .first()
            .map(|q| q.options.len())
            .unwrap_or(0);
        state.clamp_selection(initial_len);
        Self {
            app_event_tx,
            answers: vec![QuestionAnswer::default(); round.questions.len()],
            round,
            active_tab: 0,
            state,
            complete: false,
            error: None,
            free_text_editor: TextArea::new(),
            free_text_state: RefCell::new(TextAreaState::default()),
            is_editing_free_text: false,
        }
    }

    fn active_question(&self) -> Option<&PlanQuestion> {
        self.round.questions.get(self.active_tab)
    }

    fn active_answer(&self) -> Option<&QuestionAnswer> {
        self.answers.get(self.active_tab)
    }

    fn is_submit_tab(&self) -> bool {
        self.active_tab >= self.round.questions.len()
    }

    fn move_tab_left(&mut self) {
        if self.active_tab > 0 {
            self.save_active_free_text();
            self.active_tab -= 1;
            self.reset_option_cursor();
        }
    }

    fn move_tab_right(&mut self) {
        let max = self.round.questions.len();
        if self.active_tab < max {
            self.save_active_free_text();
            self.active_tab += 1;
            if self.active_tab == max && self.answers.iter().all(answer_is_nonempty) {
                self.submit();
                return;
            }
            self.reset_option_cursor();
        }
    }

    fn reset_option_cursor(&mut self) {
        self.is_editing_free_text = false;
        let len = self.active_question().map(|q| q.options.len()).unwrap_or(0);
        self.state.reset();
        self.state.clamp_selection(len);
    }

    fn move_up(&mut self) {
        if self.is_editing_free_text {
            return;
        }
        let Some(question) = self.active_question() else {
            return;
        };
        self.state.move_up_wrap(question.options.len());
    }

    fn move_down(&mut self) {
        if self.is_editing_free_text {
            return;
        }
        let Some(question) = self.active_question() else {
            return;
        };
        self.state.move_down_wrap(question.options.len());
    }

    fn toggle_selected(&mut self) {
        self.error = None;

        if self.is_submit_tab() {
            self.submit();
            return;
        }

        let active_tab = self.active_tab;
        let Some(idx) = self.state.selected_idx else {
            return;
        };
        let Some(question) = self.round.questions.get(active_tab) else {
            return;
        };
        let Some(option) = question.options.get(idx) else {
            return;
        };

        let kind = question.kind.clone();
        let option_is_free_text = option.is_free_text;

        if option_is_free_text {
            let existing = if let Some(answer) = self.answers.get_mut(active_tab) {
                answer.selected_option_indices.clear();
                answer.free_text.take().unwrap_or_default()
            } else {
                String::new()
            };

            self.free_text_editor.set_text(existing.as_str());
            self.free_text_editor
                .set_cursor(self.free_text_editor.text().len());
            *self.free_text_state.borrow_mut() = TextAreaState::default();

            if let Some(answer) = self.answers.get_mut(active_tab) {
                answer.free_text = Some(existing);
            }
            self.is_editing_free_text = true;
            return;
        }

        self.is_editing_free_text = false;
        match kind {
            QuestionKind::SingleSelect => {
                if let Some(answer) = self.answers.get_mut(active_tab) {
                    answer.free_text = None;
                    answer.selected_option_indices = vec![idx];
                }
                self.move_tab_right();
            }
            QuestionKind::MultiSelect => {
                if let Some(answer) = self.answers.get_mut(active_tab) {
                    answer.free_text = None;
                    if let Some(pos) = answer
                        .selected_option_indices
                        .iter()
                        .position(|selected| *selected == idx)
                    {
                        answer.selected_option_indices.remove(pos);
                    } else {
                        answer.selected_option_indices.push(idx);
                        answer.selected_option_indices.sort_unstable();
                    }
                }
            }
        }
    }

    fn submit(&mut self) {
        self.save_active_free_text();

        let all_answered = self.answers.iter().all(answer_is_nonempty);
        if !all_answered {
            self.error = Some("Answer all questions to submit.".to_string());
            self.active_tab = 0;
            self.reset_option_cursor();
            return;
        }

        let formatted = format_answers(&self.round, &self.answers);
        self.app_event_tx.send(AppEvent::CodexOp(Op::UserInput {
            items: vec![UserInput::Text { text: formatted }],
        }));
        self.complete = true;
    }

    fn save_active_free_text(&mut self) {
        if !self.is_editing_free_text {
            return;
        }

        let active_tab = self.active_tab;
        let normalized = normalize_free_text(self.free_text_editor.text());
        if let Some(answer) = self.answers.get_mut(active_tab)
            && answer.free_text.is_some()
        {
            answer.free_text = Some(normalized.clone());
        }

        self.free_text_editor.set_text(normalized.as_str());
        self.free_text_editor
            .set_cursor(self.free_text_editor.text().len());
        *self.free_text_state.borrow_mut() = TextAreaState::default();
        self.is_editing_free_text = false;
    }
}

impl BottomPaneView for PlanQuestionsView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind != KeyEventKind::Press {
            return;
        }

        if self.is_editing_free_text {
            self.handle_free_text_key_event(key_event);
            return;
        }

        match key_event {
            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                self.complete = true;
            }
            KeyEvent {
                code: KeyCode::Left,
                ..
            } => self.move_tab_left(),
            KeyEvent {
                code: KeyCode::Right,
                ..
            }
            | KeyEvent {
                code: KeyCode::Tab,
                modifiers: KeyModifiers::NONE,
                ..
            } => self.move_tab_right(),
            KeyEvent {
                code: KeyCode::Up, ..
            } => self.move_up(),
            KeyEvent {
                code: KeyCode::Down,
                ..
            } => self.move_down(),
            KeyEvent {
                code: KeyCode::Enter,
                ..
            } => self.toggle_selected(),
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                if let Some(idx) = c
                    .to_digit(10)
                    .and_then(|n| n.checked_sub(1))
                    .map(|n| n as usize)
                {
                    let Some(question) = self.active_question() else {
                        return;
                    };
                    if idx < question.options.len() {
                        self.state.selected_idx = Some(idx);
                        self.toggle_selected();
                    }
                }
            }
            _ => {}
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.complete = true;
        CancellationEvent::Handled
    }

    fn handle_paste(&mut self, pasted: String) -> bool {
        if !self.is_editing_free_text || pasted.is_empty() {
            return false;
        }

        let normalized = normalize_free_text(pasted.as_str());
        self.free_text_editor.insert_str(normalized.as_str());
        let text = self.free_text_editor.text().to_string();
        if let Some(answer) = self.answers.get_mut(self.active_tab)
            && answer.free_text.is_some()
        {
            answer.free_text = Some(text);
        }
        true
    }
}

impl Renderable for PlanQuestionsView {
    fn desired_height(&self, width: u16) -> u16 {
        let prompt_height = self
            .active_question()
            .map(|q| wrap_plain_lines(q.prompt.as_str(), width).len() as u16)
            .unwrap_or(1);

        let options_height = self
            .active_question()
            .map(|q| {
                measure_options_height(q, self.active_answer(), self.is_editing_free_text, width)
            })
            .unwrap_or(1);

        let free_text_height = self.free_text_editor_height(width);

        // Header + (optional error) + blank + prompt + blank + options + (optional free text) + blank + footer
        let error_height = self.error.as_ref().map(|_| 1u16).unwrap_or(0);
        1 + error_height + 1 + prompt_height + 1 + options_height + free_text_height + 1 + 1
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        // Layout: header, (optional error), blank, prompt, blank, options, (optional input), blank, footer.
        let header_height = 1u16;
        let error_height = self.error.as_ref().map(|_| 1u16).unwrap_or(0);
        let blank_height = 1u16;
        let prompt_height = self
            .active_question()
            .map(|q| wrap_plain_lines(q.prompt.as_str(), area.width).len() as u16)
            .unwrap_or(1);
        let options_height = self
            .active_question()
            .map(|q| {
                measure_options_height(
                    q,
                    self.active_answer(),
                    self.is_editing_free_text,
                    area.width,
                )
            })
            .unwrap_or(1);
        let free_text_height = self.free_text_editor_height(area.width);
        let footer_height = 1u16;

        let [
            header_rect,
            error_rect,
            _blank1,
            prompt_rect,
            _blank2,
            options_rect,
            free_text_rect,
            _blank3,
            footer_rect,
        ] = Layout::vertical([
            Constraint::Length(header_height),
            Constraint::Length(error_height),
            Constraint::Length(blank_height),
            Constraint::Length(prompt_height),
            Constraint::Length(blank_height),
            Constraint::Length(options_height),
            Constraint::Length(free_text_height),
            Constraint::Length(blank_height),
            Constraint::Length(footer_height),
        ])
        .areas(area);

        Paragraph::new(step_bar_line(self.active_tab, &self.round, &self.answers))
            .render(header_rect, buf);

        if let Some(error) = self.error.as_ref() {
            Paragraph::new(Line::from(error.as_str()).red()).render(error_rect, buf);
        }

        if let Some(question) = self.active_question() {
            let prompt_lines = wrap_plain_lines(question.prompt.as_str(), prompt_rect.width);
            Paragraph::new(prompt_lines).render(prompt_rect, buf);

            render_options_list(
                question,
                self.active_answer(),
                options_rect,
                buf,
                &self.state,
                self.is_editing_free_text,
            );
        } else {
            Paragraph::new(Line::from("No questions").dim()).render(prompt_rect, buf);
        }

        if free_text_height > 0 && !free_text_rect.is_empty() {
            let mut state = self.free_text_state.borrow_mut();
            StatefulWidgetRef::render_ref(
                &(&self.free_text_editor),
                free_text_rect,
                buf,
                &mut state,
            );
            if self.free_text_editor.text().is_empty() {
                Paragraph::new(Line::from("Type your answer…").dim()).render(free_text_rect, buf);
            }
        }

        Paragraph::new(
            Line::from("Enter to select · Tab/Arrow keys to navigate · Esc to cancel").dim(),
        )
        .render(footer_rect, buf);
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        if !self.is_editing_free_text {
            return None;
        }

        if area.height == 0 || area.width == 0 {
            return None;
        }

        let error_height = self.error.as_ref().map(|_| 1u16).unwrap_or(0);
        let prompt_height = self
            .active_question()
            .map(|q| wrap_plain_lines(q.prompt.as_str(), area.width).len() as u16)
            .unwrap_or(1);
        let options_height = self
            .active_question()
            .map(|q| {
                measure_options_height(
                    q,
                    self.active_answer(),
                    self.is_editing_free_text,
                    area.width,
                )
            })
            .unwrap_or(1);
        let free_text_height = self.free_text_editor_height(area.width);
        if free_text_height == 0 {
            return None;
        }

        let header_height = 1u16;
        let blank_height = 1u16;
        let y = area.y
            + header_height
            + error_height
            + blank_height
            + prompt_height
            + blank_height
            + options_height;
        let rect = Rect {
            x: area.x,
            y,
            width: area.width,
            height: free_text_height,
        };
        let state = *self.free_text_state.borrow();
        self.free_text_editor.cursor_pos_with_state(rect, state)
    }
}

fn step_bar_line(
    active_tab: usize,
    round: &PlanQuestionRound,
    answers: &[QuestionAnswer],
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push("←  ".dim());

    for (idx, question) in round.questions.iter().enumerate() {
        let answered = answers.get(idx).is_some_and(answer_is_nonempty);
        let marker = if answered { "☑ " } else { "☐ " };
        let mut tab_line = Line::from(vec![marker.into(), question.label.clone().into()]);
        if idx == active_tab {
            tab_line = tab_line.cyan().bold();
        } else {
            tab_line = tab_line.dim();
        }
        spans.extend(tab_line.spans);
        spans.push("  ".into());
    }

    let submit_active = active_tab >= round.questions.len();
    let submit = if submit_active {
        "✔ Submit".cyan().bold()
    } else {
        "✔ Submit".dim()
    };
    spans.push(submit);
    spans.push("  →".dim());
    Line::from(spans)
}

fn wrap_plain_lines(text: &str, width: u16) -> Vec<Line<'static>> {
    let wrap_width = width.max(1) as usize;
    textwrap::wrap(text, wrap_width)
        .into_iter()
        .map(|cow| Line::from(cow.into_owned()))
        .collect()
}

fn render_options_list(
    question: &PlanQuestion,
    answer: Option<&QuestionAnswer>,
    area: Rect,
    buf: &mut Buffer,
    state: &ScrollState,
    is_editing_free_text: bool,
) {
    if area.height == 0 {
        return;
    }

    let selected: &[usize] = answer.map_or(&[], |a| a.selected_option_indices.as_slice());
    let mut lines: Vec<Line<'static>> = Vec::new();

    let start = state.scroll_top;
    let visible = question.options.len().saturating_sub(start);

    for (visible_idx, (idx, opt)) in question
        .options
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .enumerate()
    {
        let is_cursor = state.selected_idx == Some(start + visible_idx);
        let prefix = if is_cursor { "❯ " } else { "  " };

        let (checkbox, title, desc) = if opt.is_free_text {
            let free_text = answer
                .and_then(|a| a.free_text.as_deref())
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string);
            let desc = if is_editing_free_text {
                None
            } else {
                free_text.or_else(|| Some("Next".to_string()))
            };
            ("[ ]", "Type something".to_string(), desc)
        } else {
            let checked = selected.contains(&idx);
            let checkbox = if checked { "[x]" } else { "[ ]" };
            (checkbox, opt.title.clone(), opt.description.clone())
        };

        let line = Line::from(vec![
            prefix.into(),
            format!("{}. ", idx + 1).into(),
            format!("{checkbox} ").into(),
            title.into(),
        ]);
        lines.push(if is_cursor { line.cyan().bold() } else { line });

        if let Some(desc) = desc {
            let wrapped = word_wrap_lines(
                std::iter::once(desc),
                RtOptions::new(area.width as usize)
                    .initial_indent(Line::from("     "))
                    .subsequent_indent(Line::from("     ")),
            );
            lines.extend(wrapped.into_iter().map(ratatui::prelude::Stylize::dim));
        }
    }

    Paragraph::new(lines).render(area, buf);
}

fn format_answers(round: &PlanQuestionRound, answers: &[QuestionAnswer]) -> String {
    let mut out = String::new();
    for (idx, (question, answer)) in round.questions.iter().zip(answers.iter()).enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        if let Some(text) = answer
            .free_text
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            out.push_str(text);
            continue;
        }

        match question.kind {
            QuestionKind::SingleSelect => {
                if let Some(sel) = answer.selected_option_indices.first() {
                    out.push_str(&(sel + 1).to_string());
                }
            }
            QuestionKind::MultiSelect => {
                let list = answer
                    .selected_option_indices
                    .iter()
                    .map(|sel| (sel + 1).to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                out.push_str(&list);
            }
        }
    }
    out
}

fn answer_is_nonempty(answer: &QuestionAnswer) -> bool {
    answer
        .free_text
        .as_ref()
        .is_some_and(|text| !text.trim().is_empty())
        || !answer.selected_option_indices.is_empty()
}

fn normalize_free_text(text: &str) -> String {
    let mut out = String::new();
    for part in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(part);
    }
    out
}

fn measure_options_height(
    question: &PlanQuestion,
    answer: Option<&QuestionAnswer>,
    is_editing_free_text: bool,
    width: u16,
) -> u16 {
    let options_width = width.max(1) as usize;
    let mut height = 0u16;
    for option in &question.options {
        // Option line itself.
        height = height.saturating_add(1);

        let desc = if option.is_free_text {
            let free_text = answer
                .and_then(|a| a.free_text.as_deref())
                .map(str::trim)
                .filter(|t| !t.is_empty());
            if is_editing_free_text {
                None
            } else if let Some(text) = free_text {
                Some(text.to_string())
            } else {
                Some("Next".to_string())
            }
        } else {
            option.description.as_ref().cloned()
        };

        if let Some(desc) = desc {
            let wrapped = word_wrap_lines(
                std::iter::once(desc),
                RtOptions::new(options_width)
                    .initial_indent(Line::from("     "))
                    .subsequent_indent(Line::from("     ")),
            );
            height = height.saturating_add(wrapped.len() as u16);
        }
    }
    height.max(1)
}

impl PlanQuestionsView {
    fn handle_free_text_key_event(&mut self, key_event: KeyEvent) {
        match key_event {
            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                self.complete = true;
            }
            KeyEvent {
                code: KeyCode::Tab,
                modifiers: KeyModifiers::NONE,
                ..
            }
            | KeyEvent {
                code: KeyCode::Right,
                ..
            } => {
                self.save_active_free_text();
                self.move_tab_right();
            }
            KeyEvent {
                code: KeyCode::Enter,
                ..
            } => {
                self.save_active_free_text();
                self.move_tab_right();
            }
            other => {
                self.free_text_editor.input(other);
                let text = self.free_text_editor.text().to_string();
                if let Some(answer) = self.answers.get_mut(self.active_tab)
                    && answer.free_text.is_some()
                {
                    answer.free_text = Some(text);
                }
            }
        }
    }

    fn free_text_editor_height(&self, width: u16) -> u16 {
        if !self.is_editing_free_text {
            return 0;
        }

        let usable_width = width.max(1);
        self.free_text_editor
            .desired_height(usable_width)
            .clamp(1, 3)
    }
}

pub(crate) fn parse_plan_question_round(text: &str) -> Option<PlanQuestionRound> {
    let mut lines = text.lines();
    for line in lines.by_ref() {
        if is_decision_points_header(line) {
            break;
        }
    }

    let mut questions: Vec<PlanQuestion> = Vec::new();
    let mut current: Option<PlanQuestion> = None;
    let mut current_option: Option<QuestionOption> = None;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("checkpoints")
            || trimmed.eq_ignore_ascii_case("rollback")
            || trimmed.eq_ignore_ascii_case("plan")
            || trimmed.eq_ignore_ascii_case("goal")
        {
            break;
        }

        if let Some((num, rest)) = parse_numbered_line(line) {
            let rest = rest.trim().to_string();
            let is_question = looks_like_question(rest.as_str());

            if is_question {
                if let (Some(question), Some(option)) = (current.as_mut(), current_option.take()) {
                    question.options.push(option);
                }
                if let Some(prev) = current.take() {
                    questions.push(prev);
                }
                current = Some(parse_question(num, rest.as_str()));
                continue;
            }

            if let Some(question) = current.as_mut() {
                if let Some(option) = current_option.take() {
                    question.options.push(option);
                }
                current_option = Some(parse_option(rest.as_str()));
            }
            continue;
        }

        if let Some(rest) = parse_bullet_line(line) {
            let rest = rest.trim().to_string();
            if let Some(question) = current.as_mut() {
                if let Some(option) = current_option.take() {
                    question.options.push(option);
                }
                current_option = Some(parse_option(rest.as_str()));
            }
            continue;
        }

        if let Some(option) = current_option.as_mut() {
            if trimmed.is_empty() {
                continue;
            }
            append_option_description(option, trimmed);
        }
    }

    if let Some(question) = current.as_mut()
        && let Some(option) = current_option.take()
    {
        question.options.push(option);
    }
    if let Some(prev) = current.take() {
        questions.push(prev);
    }

    if questions.is_empty() {
        return None;
    }

    // UI limit: 1–5 questions per round.
    questions.truncate(5);

    for q in &mut questions {
        // Cap options to the UI maximum.
        if q.options.len() > 5 {
            q.options.truncate(5);
        }

        // Ensure there is a single free-text option and it is last.
        if let Some(pos) = q.options.iter().position(|o| o.is_free_text) {
            let opt = q.options.remove(pos);
            q.options.push(opt);
        } else if q.options.len() < 5 {
            q.options.push(QuestionOption {
                title: "(None) Type your answer".to_string(),
                description: None,
                is_free_text: true,
            });
        } else if let Some(last) = q.options.last_mut() {
            last.title = "(None) Type your answer".to_string();
            last.description = None;
            last.is_free_text = true;
        }
    }

    Some(PlanQuestionRound { questions })
}

fn is_decision_points_header(line: &str) -> bool {
    let trimmed = line.trim();
    let rest = trimmed.trim_start_matches('#').trim_start();

    let lower = rest.to_ascii_lowercase();
    if !lower.starts_with("decision points") {
        return false;
    }

    let suffix = &rest["decision points".len()..];
    suffix.is_empty()
        || suffix
            .chars()
            .next()
            .is_some_and(|c| matches!(c, ':' | '-' | '—' | '<') || c.is_whitespace())
}

fn parse_numbered_line(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let (digits, rest) = split_digits_prefix(trimmed)?;

    let mut chars = rest.chars();
    match chars.next()? {
        ')' | '.' | ':' | '-' => {}
        _ => return None,
    }
    if !chars
        .as_str()
        .chars()
        .next()
        .is_some_and(char::is_whitespace)
    {
        return None;
    }

    let num: usize = digits.parse().ok()?;
    Some((num, chars.as_str().trim_start()))
}

fn parse_bullet_line(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    match chars.next()? {
        '-' | '*' => {}
        _ => return None,
    }
    if !chars
        .as_str()
        .chars()
        .next()
        .is_some_and(char::is_whitespace)
    {
        return None;
    }
    Some(chars.as_str().trim_start())
}

fn split_digits_prefix(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    Some((&s[..i], &s[i..]))
}

fn looks_like_question(rest: &str) -> bool {
    let lowered = rest.to_ascii_lowercase();
    rest.contains("**")
        || lowered.contains("single-select")
        || lowered.contains("single select")
        || lowered.contains("multi-select")
        || lowered.contains("multi select")
        || lowered.contains("select all")
}

fn parse_question(num: usize, rest: &str) -> PlanQuestion {
    let original = rest.trim().to_string();
    let (label_opt, mut prompt) = split_first_bold_block(original.as_str());
    let label = label_opt.unwrap_or_else(|| format!("Question {num}"));

    let lowered = original.to_ascii_lowercase();
    let kind = if lowered.contains("multi-select")
        || lowered.contains("multi select")
        || lowered.contains("select all")
    {
        QuestionKind::MultiSelect
    } else {
        QuestionKind::SingleSelect
    };

    prompt = prompt
        .replace("(single-select)", "")
        .replace("(multi-select)", "");
    let prompt = normalize_free_text(prompt.trim().trim_start_matches([':', '-', '—']).trim());
    let prompt = if prompt.is_empty() { original } else { prompt };

    PlanQuestion {
        label,
        prompt,
        kind,
        options: Vec::new(),
    }
}

fn parse_option(rest: &str) -> QuestionOption {
    let rest = rest.trim();
    let rest_lower = rest.to_ascii_lowercase();
    let is_free_text = rest_lower.contains("type your answer")
        || rest_lower.contains("type something")
        || rest_lower.contains("(none)");

    let (title, description) = if let Some((lhs, rhs)) = rest.split_once(" - ") {
        (lhs.trim().to_string(), Some(rhs.trim().to_string()))
    } else if let Some((lhs, rhs)) = rest.split_once(" — ") {
        (lhs.trim().to_string(), Some(rhs.trim().to_string()))
    } else {
        (rest.to_string(), None)
    };

    QuestionOption {
        title,
        description,
        is_free_text,
    }
}

fn append_option_description(option: &mut QuestionOption, line: &str) {
    let desc = option.description.get_or_insert_with(String::new);
    if !desc.is_empty() {
        desc.push(' ');
    }
    desc.push_str(line.trim());
}

fn split_first_bold_block(s: &str) -> (Option<String>, String) {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'*' {
            let start = i + 2;
            let mut j = start;
            while j + 1 < bytes.len() {
                if bytes[j] == b'*' && bytes[j + 1] == b'*' {
                    let inner = &s[start..j];
                    let label = inner.trim();
                    if label.is_empty() {
                        return (None, s.to_string());
                    }
                    let mut rest = String::new();
                    rest.push_str(&s[..i]);
                    rest.push_str(&s[j + 2..]);
                    return (Some(label.to_string()), rest);
                }
                j += 1;
            }
            return (None, s.to_string());
        }
        i += 1;
    }
    (None, s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use pretty_assertions::assert_eq;
    use tokio::sync::mpsc::unbounded_channel;

    fn single_select_round(num_questions: usize) -> PlanQuestionRound {
        PlanQuestionRound {
            questions: (0..num_questions)
                .map(|idx| PlanQuestion {
                    label: format!("Q{}", idx + 1),
                    prompt: "Pick one".to_string(),
                    kind: QuestionKind::SingleSelect,
                    options: vec![
                        QuestionOption {
                            title: "A".to_string(),
                            description: None,
                            is_free_text: false,
                        },
                        QuestionOption {
                            title: "(None) Type your answer".to_string(),
                            description: None,
                            is_free_text: true,
                        },
                    ],
                })
                .collect(),
        }
    }

    #[test]
    fn single_select_last_answer_auto_submits() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let round = single_select_round(2);
        let mut view = PlanQuestionsView::new(round, tx);

        view.handle_key_event(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
        assert!(!view.is_complete());

        view.handle_key_event(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
        assert!(view.is_complete());

        let mut submitted = None;
        while let Ok(ev) = rx.try_recv() {
            if let AppEvent::CodexOp(Op::UserInput { items }) = ev {
                submitted = items
                    .iter()
                    .filter_map(|item| match item {
                        UserInput::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                    .next();
                break;
            }
        }

        assert_eq!(submitted.as_deref(), Some("1\n1"));
    }

    #[test]
    fn parse_accepts_unindented_options() {
        let text = "\
Goal
X

Decision points
1) **Scope** (single-select): Choose one
1. Option A
2. Option B

Checkpoints
- None
";
        let round = parse_plan_question_round(text).expect("expected round");
        assert_eq!(round.questions.len(), 1);
        assert_eq!(round.questions[0].options[0].title, "Option A");
        assert_eq!(round.questions[0].options[1].title, "Option B");
        assert_eq!(
            round.questions[0]
                .options
                .last()
                .expect("expected free text option")
                .title,
            "(None) Type your answer"
        );
    }

    #[test]
    fn parse_accepts_colon_separators() {
        let text = "\
Decision points
1: **Scope** (single-select): Choose one
  1. Option A
  2. Option B
";
        let round = parse_plan_question_round(text).expect("expected round");
        assert_eq!(round.questions.len(), 1);
        assert_eq!(round.questions[0].label, "Scope");
    }

    #[test]
    fn parse_none_without_questions() {
        let text = "\
Decision points
- This is not formatted as questions
";
        assert!(parse_plan_question_round(text).is_none());
    }

    #[test]
    fn parse_accepts_bullet_options() {
        let text = "\
Decision points
1) **Scope** (single-select): Choose one
- Option A
- Option B
";
        let round = parse_plan_question_round(text).expect("expected round");
        assert_eq!(round.questions.len(), 1);
        assert_eq!(round.questions[0].options[0].title, "Option A");
        assert_eq!(round.questions[0].options[1].title, "Option B");
        assert_eq!(
            round.questions[0]
                .options
                .last()
                .expect("expected free text option")
                .title,
            "(None) Type your answer"
        );
    }

    #[test]
    fn parse_accumulates_multiline_option_description() {
        let text = "\
Decision points
1) **Scope** (single-select): Choose one
  1. Option A
     This is a longer description
     that spans multiple lines.
  2. Option B
";
        let round = parse_plan_question_round(text).expect("expected round");
        assert_eq!(round.questions.len(), 1);
        assert_eq!(round.questions[0].options[0].title, "Option A");
        assert_eq!(
            round.questions[0].options[0].description.as_deref(),
            Some("This is a longer description that spans multiple lines.")
        );
    }

    #[test]
    fn parse_forces_free_text_without_relabeling_real_option() {
        let text = "\
Decision points
1) **Scope** (single-select): Choose one
  1. Option A
  2. Option B
  3. Option C
  4. Option D
  5. Option E
";
        let round = parse_plan_question_round(text).expect("expected round");
        assert_eq!(round.questions.len(), 1);
        assert_eq!(round.questions[0].options.len(), 5);
        assert_eq!(round.questions[0].options[0].title, "Option A");
        assert_eq!(round.questions[0].options[3].title, "Option D");
        assert_eq!(
            round.questions[0]
                .options
                .last()
                .expect("expected free text option")
                .title,
            "(None) Type your answer"
        );
        assert!(
            round.questions[0]
                .options
                .last()
                .expect("expected free text option")
                .is_free_text
        );
    }
}
