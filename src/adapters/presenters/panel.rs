//! The panel: the last bubble expanded under the prompt, one section per
//! action word. A view model — keys in, effects out — that ratatui draws
//! into whatever frame it is given, so the tests draw into a buffer and
//! the gateway draws on the terminal.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style as Paint};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::entities::{Action, Danger, Duration, FailureCase, Fix, FixSource};
use crate::use_cases::{IgnoreRequest, ScopeChoice};

use super::Style;

const GOLD: Color = Color::Indexed(179);
const MIN_HEIGHT: u16 = 6;
const MAX_HEIGHT: u16 = 14;
/// Header, tabs and footer.
const CHROME: u16 = 3;
const SEAM_WIDTH: u16 = 2;
/// Room kept for an answer that is still being asked for.
const ASKING_ROOM: u16 = 6;
const PAGE: u16 = 5;
const HOUR: Duration = Duration::from_millis(3_600_000);

/// What the user pressed, freed from the terminal's encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Section(Action),
    Next,
    Prev,
    Up,
    Down,
    PageUp,
    PageDown,
    Enter,
    Copy,
    Close,
    Help,
    Click { column: u16, row: u16 },
}

/// What the panel wants done outside itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Nothing,
    Ask(Ask),
    Insert(String),
    Copy(String),
    Ignore(IgnoreRequest),
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    Fix,
    Explain,
}

/// What comes back from outside: which model is being asked, its answer,
/// a confirmation.
#[derive(Debug, Clone, PartialEq)]
pub enum Arrival {
    Asking(Ask, String),
    Fix(Result<Option<Fix>, String>),
    Explanation(Result<(String, String), String>),
    Ignored(String),
    Note(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Loading<T> {
    Idle,
    Asking(String),
    Ready(T),
    Failed(String),
}

pub struct Panel {
    command: String,
    status: String,
    duration: Option<String>,
    fix: Loading<Option<Fix>>,
    can_ask_fix: bool,
    explanation: Loading<(String, String)>,
    agents: Vec<String>,
    agent_focus: usize,
    ignore_choices: Vec<(String, IgnoreRequest)>,
    ignore_focus: usize,
    ignored: Option<String>,
    privacy: String,
    section: Action,
    scroll: u16,
    note: Option<String>,
    help: bool,
    tab_areas: Vec<(Rect, Action)>,
    row_areas: Vec<(Rect, usize)>,
}

impl Panel {
    /// `fix` is what rules or a stored proposal already know; `can_ask_fix`
    /// says a quick-fix model is routed; `agents` are the configured CLI
    /// agents, `default_agent` the one routed for investigations.
    pub fn new(
        case: &FailureCase,
        fix: Option<Fix>,
        can_ask_fix: bool,
        agents: Vec<String>,
        default_agent: Option<&str>,
        privacy: String,
    ) -> Self {
        let outcome = case.outcome();
        let program = outcome.command().program().to_string();
        let mut ignore_choices = vec![(
            "this exact command line".to_string(),
            IgnoreRequest::Last {
                program: None,
                scope: ScopeChoice::Command,
            },
        )];
        if let Some(cwd) = case.cwd() {
            ignore_choices.push((
                format!("{program} in this directory"),
                IgnoreRequest::Last {
                    program: Some(program.clone()),
                    scope: ScopeChoice::Directory(cwd.to_string()),
                },
            ));
        }
        ignore_choices.push((
            format!("{program} in this shell"),
            IgnoreRequest::Last {
                program: Some(program.clone()),
                scope: ScopeChoice::Session,
            },
        ));
        ignore_choices.push((
            format!("{program} everywhere"),
            IgnoreRequest::Last {
                program: Some(program),
                scope: ScopeChoice::Always,
            },
        ));
        ignore_choices.push((
            "everything for an hour".to_string(),
            IgnoreRequest::Mute(HOUR),
        ));
        let agent_focus = default_agent
            .and_then(|d| agents.iter().position(|a| a == d))
            .unwrap_or(0);
        let section = if fix.is_some() {
            Action::Fix
        } else {
            Action::Why
        };
        Self {
            command: outcome.command().as_str().to_string(),
            status: format!("exit {}", outcome.status()),
            duration: outcome
                .duration()
                .filter(|d| d.as_millis() >= 1_000)
                .map(|d| d.to_string()),
            fix: match fix {
                Some(fix) => Loading::Ready(Some(fix)),
                None => Loading::Idle,
            },
            can_ask_fix,
            explanation: match case.explanation() {
                Some(e) => Loading::Ready((e.model().to_string(), e.text().to_string())),
                None => Loading::Idle,
            },
            agents,
            agent_focus,
            ignore_choices,
            ignore_focus: 0,
            ignored: None,
            privacy,
            section,
            scroll: 0,
            note: None,
            help: false,
            tab_areas: Vec::new(),
            row_areas: Vec::new(),
        }
    }

    /// The first thing to do once open: the section's own question.
    pub fn open(&mut self) -> Effect {
        self.enter(self.section)
    }

    #[cfg(test)]
    pub fn section(&self) -> Action {
        self.section
    }

    pub fn press(&mut self, key: Key) -> Effect {
        match key {
            Key::Section(action) => self.enter(action),
            Key::Next => self.enter(self.neighbour(1)),
            Key::Prev => self.enter(self.neighbour(-1)),
            Key::Up => self.move_focus(-1),
            Key::Down => self.move_focus(1),
            Key::PageUp => {
                self.scroll = self.scroll.saturating_sub(PAGE);
                Effect::Nothing
            }
            Key::PageDown => {
                self.scroll = self.scroll.saturating_add(PAGE);
                Effect::Nothing
            }
            Key::Enter => self.act(),
            Key::Copy => match self.known_fix() {
                Some(fix) => Effect::Copy(fix.command().as_str().to_string()),
                None => {
                    self.note = Some("nothing to copy yet".into());
                    Effect::Nothing
                }
            },
            Key::Close => Effect::Close,
            Key::Help => {
                self.help = !self.help;
                Effect::Nothing
            }
            Key::Click { column, row } => self.click(column, row),
        }
    }

    pub fn receive(&mut self, arrival: Arrival) {
        match arrival {
            Arrival::Asking(Ask::Fix, model) => self.fix = Loading::Asking(model),
            Arrival::Asking(Ask::Explain, model) => self.explanation = Loading::Asking(model),
            Arrival::Fix(Ok(fix)) => self.fix = Loading::Ready(fix),
            Arrival::Fix(Err(why)) => self.fix = Loading::Failed(why),
            Arrival::Explanation(Ok(answer)) => self.explanation = Loading::Ready(answer),
            Arrival::Explanation(Err(why)) => self.explanation = Loading::Failed(why),
            Arrival::Ignored(text) => self.ignored = Some(text),
            Arrival::Note(text) => self.note = Some(text),
        }
    }

    /// Rows to draw, between six and fourteen: room for what is shown, or
    /// for what is being asked.
    pub fn height(&self, width: u16, style: &Style) -> u16 {
        let (lines, _) = self.content(style);
        let shown = wrapped_rows(&lines, width.saturating_sub(SEAM_WIDTH).max(1));
        let asking = match self.section {
            Action::Why => matches!(self.explanation, Loading::Asking(_) | Loading::Idle),
            Action::Fix => {
                matches!(self.fix, Loading::Asking(_))
                    || (matches!(self.fix, Loading::Idle) && self.can_ask_fix)
            }
            _ => false,
        };
        let needed = shown.max(if asking { ASKING_ROOM } else { 3 });
        (CHROME + needed).clamp(MIN_HEIGHT, MAX_HEIGHT)
    }

    pub fn render(&mut self, frame: &mut Frame, style: &Style) {
        let area = frame.area();
        if area.height < CHROME || area.width < SEAM_WIDTH + 10 {
            return;
        }
        let seam_glyph = if style.ascii { "|" } else { "▎" };
        let seam: Vec<Line> = (0..area.height)
            .map(|_| {
                Line::from(Span::styled(
                    seam_glyph,
                    paint(style, Paint::new().fg(GOLD)),
                ))
            })
            .collect();
        frame.render_widget(
            Paragraph::new(seam),
            Rect::new(area.x, area.y, SEAM_WIDTH, area.height),
        );
        let body = Rect::new(
            area.x + SEAM_WIDTH,
            area.y,
            area.width - SEAM_WIDTH,
            area.height,
        );
        frame.render_widget(
            Paragraph::new(self.header(style, body.width)),
            Rect::new(body.x, body.y, body.width, 1),
        );
        frame.render_widget(
            Paragraph::new(self.tabs(style, body)),
            Rect::new(body.x, body.y + 1, body.width, 1),
        );
        let content_area = Rect::new(body.x, body.y + 2, body.width, body.height - CHROME);
        let (lines, list_start) = self.content(style);
        let total = wrapped_rows(&lines, content_area.width);
        self.scroll = self.scroll.min(total.saturating_sub(content_area.height));
        self.row_areas.clear();
        if let Some(start) = list_start {
            let rows = match self.section {
                Action::Agent => self.agents.len(),
                Action::Ignore => self.ignore_choices.len(),
                _ => 0,
            };
            for index in 0..rows {
                let line = (start + index) as u16;
                if line >= self.scroll && line - self.scroll < content_area.height {
                    let y = content_area.y + line - self.scroll;
                    self.row_areas
                        .push((Rect::new(content_area.x, y, content_area.width, 1), index));
                }
            }
        }
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0)),
            content_area,
        );
        frame.render_widget(
            Paragraph::new(self.footer(style, body.width)),
            Rect::new(body.x, body.y + body.height - 1, body.width, 1),
        );
    }

    fn enter(&mut self, section: Action) -> Effect {
        self.section = section;
        self.scroll = 0;
        self.note = None;
        match section {
            Action::Why if matches!(self.explanation, Loading::Idle) => Effect::Ask(Ask::Explain),
            Action::Fix if matches!(self.fix, Loading::Idle) => {
                if self.can_ask_fix {
                    Effect::Ask(Ask::Fix)
                } else {
                    self.fix = Loading::Failed(
                        "No rule knows this one, and no model is routed for quick fixes.".into(),
                    );
                    Effect::Nothing
                }
            }
            _ => Effect::Nothing,
        }
    }

    fn neighbour(&self, delta: isize) -> Action {
        let all = Action::ALL;
        let index = all.iter().position(|a| *a == self.section).unwrap_or(0) as isize;
        all[(index + delta).rem_euclid(all.len() as isize) as usize]
    }

    fn move_focus(&mut self, delta: isize) -> Effect {
        match self.section {
            Action::Agent if !self.agents.is_empty() => {
                self.agent_focus = step(self.agent_focus, delta, self.agents.len());
            }
            Action::Ignore if self.ignored.is_none() => {
                self.ignore_focus = step(self.ignore_focus, delta, self.ignore_choices.len());
            }
            _ => {
                self.scroll = if delta < 0 {
                    self.scroll.saturating_sub(1)
                } else {
                    self.scroll.saturating_add(1)
                };
            }
        }
        Effect::Nothing
    }

    fn act(&mut self) -> Effect {
        match self.section {
            Action::Agent => match self.agents.get(self.agent_focus) {
                Some(name) if self.agents.len() > 1 => {
                    Effect::Insert(format!("kintsu agent --with {name}"))
                }
                Some(_) => Effect::Insert("kintsu agent".into()),
                None => Effect::Nothing,
            },
            Action::Ignore if self.ignored.is_some() => Effect::Close,
            Action::Ignore => self
                .ignore_choices
                .get(self.ignore_focus)
                .map(|(_, request)| Effect::Ignore(request.clone()))
                .unwrap_or(Effect::Nothing),
            _ => match self.known_fix() {
                Some(fix) => Effect::Insert(fix.command().as_str().to_string()),
                None => Effect::Nothing,
            },
        }
    }

    fn click(&mut self, column: u16, row: u16) -> Effect {
        let hit = |area: &Rect| {
            column >= area.x
                && column < area.x + area.width
                && row >= area.y
                && row < area.y + area.height
        };
        if let Some((_, action)) = self.tab_areas.iter().find(|(area, _)| hit(area)) {
            return self.enter(*action);
        }
        if let Some((_, index)) = self.row_areas.iter().find(|(area, _)| hit(area)) {
            match self.section {
                Action::Agent => self.agent_focus = *index,
                Action::Ignore => self.ignore_focus = *index,
                _ => {}
            }
        }
        Effect::Nothing
    }

    fn known_fix(&self) -> Option<&Fix> {
        match &self.fix {
            Loading::Ready(Some(fix)) => Some(fix),
            _ => None,
        }
    }

    fn header(&self, style: &Style, width: u16) -> Line<'static> {
        let dot = style.dot();
        let mut headline = format!("{}{dot}{}", self.command, self.status);
        if let Some(duration) = &self.duration {
            headline.push_str(dot);
            headline.push_str(duration);
        }
        let close = "esc";
        let room = (width as usize).saturating_sub(close.len() + 1);
        let headline = style.abbreviate(&headline, room);
        let padding = room.saturating_sub(headline.chars().count()) + 1;
        Line::from(vec![
            Span::styled(
                headline,
                paint(style, Paint::new().add_modifier(Modifier::BOLD)),
            ),
            Span::raw(" ".repeat(padding)),
            Span::styled(
                close,
                paint(style, Paint::new().add_modifier(Modifier::DIM)),
            ),
        ])
    }

    fn tabs(&mut self, style: &Style, body: Rect) -> Line<'static> {
        self.tab_areas.clear();
        let mut spans = Vec::new();
        let mut x = body.x;
        for action in Action::ALL {
            let label = capitalised(action.name());
            let key = &action.name()[..1];
            let width = (label.len() + 1 + key.len()) as u16;
            self.tab_areas
                .push((Rect::new(x, body.y + 1, width, 1), action));
            let active = action == self.section;
            let label_paint = if active {
                Paint::new()
                    .fg(GOLD)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Paint::new()
            };
            spans.push(Span::styled(label, paint(style, label_paint)));
            spans.push(Span::styled(
                format!(" {key}"),
                paint(style, Paint::new().add_modifier(Modifier::DIM)),
            ));
            spans.push(Span::raw("   "));
            x += width + 3;
        }
        Line::from(spans)
    }

    /// The section's lines, and where a list starts in them.
    fn content(&self, style: &Style) -> (Vec<Line<'static>>, Option<usize>) {
        let dim = paint(style, Paint::new().add_modifier(Modifier::DIM));
        let bold = paint(style, Paint::new().add_modifier(Modifier::BOLD));
        let asking = |model: &str| {
            vec![Line::from(Span::styled(
                format!("asking {model}{}", style.ellipsis()),
                dim,
            ))]
        };
        let pointer = if style.ascii { ">" } else { "▸" };
        let rows = |items: Vec<String>, focus: usize| -> Vec<Line<'static>> {
            items
                .into_iter()
                .enumerate()
                .map(|(i, item)| {
                    if i == focus {
                        Line::from(vec![
                            Span::styled(
                                format!("{pointer} "),
                                paint(style, Paint::new().fg(GOLD)),
                            ),
                            Span::styled(item, bold),
                        ])
                    } else {
                        Line::from(format!("  {item}"))
                    }
                })
                .collect()
        };
        match self.section {
            Action::Why => match &self.explanation {
                Loading::Idle => (Vec::new(), None),
                Loading::Asking(model) => (asking(model), None),
                Loading::Ready((model, text)) => {
                    let mut lines: Vec<Line<'static>> =
                        text.lines().map(|l| Line::from(l.to_string())).collect();
                    lines.push(Line::from(Span::styled(format!("— {model}"), dim)));
                    (lines, None)
                }
                Loading::Failed(why) => (vec![Line::from(why.clone())], None),
            },
            Action::Fix => match &self.fix {
                Loading::Idle => (Vec::new(), None),
                Loading::Asking(model) => (asking(model), None),
                Loading::Ready(Some(fix)) => {
                    let mut first = vec![Span::styled(fix.command().as_str().to_string(), bold)];
                    match fix.danger() {
                        Danger::None => {}
                        Danger::NeedsPrivilege => {
                            first.push(Span::styled(" (runs as root)", dim));
                        }
                        Danger::Destructive(why) => first.push(Span::styled(
                            format!(" {} {why}", style.warning_sign()),
                            paint(style, Paint::new().fg(Color::Red)),
                        )),
                    }
                    let who = match fix.source() {
                        FixSource::Rule(name) => format!("rule {name}"),
                        FixSource::Model(name) => format!("{name}{}not verified", style.dot()),
                    };
                    let confidence = (fix.confidence().value() * 100.0).round() as u32;
                    let mut lines = vec![
                        Line::from(first),
                        Line::from(Span::styled(
                            format!("{who}{}{confidence}% confident", style.dot()),
                            dim,
                        )),
                    ];
                    if !fix.rationale().trim().is_empty() {
                        lines.push(Line::from(""));
                        lines.extend(fix.rationale().lines().map(|l| Line::from(l.to_string())));
                    }
                    (lines, None)
                }
                Loading::Ready(None) => (
                    vec![
                        Line::from("No fix known for this one."),
                        Line::from(Span::styled("w explains it, a hands it to an agent.", dim)),
                    ],
                    None,
                ),
                Loading::Failed(why) => (
                    vec![
                        Line::from(why.clone()),
                        Line::from(Span::styled("kintsu setup adds a model.", dim)),
                    ],
                    None,
                ),
            },
            Action::Agent => {
                if self.agents.is_empty() {
                    (
                        vec![
                            Line::from("No agent configured."),
                            Line::from(Span::styled(
                                "kintsu setup adds Claude Code, Codex, OpenCode, aider, Gemini CLI or Copilot CLI.",
                                dim,
                            )),
                        ],
                        None,
                    )
                } else {
                    let mut lines = vec![Line::from(Span::styled(
                        "Hands the case to the agent, in this shell: command, output, cwd, git state.",
                        dim,
                    ))];
                    lines.extend(rows(self.agents.clone(), self.agent_focus));
                    (lines, Some(1))
                }
            }
            Action::Ignore => match &self.ignored {
                Some(text) => (vec![Line::from(text.clone())], None),
                None => (
                    rows(
                        self.ignore_choices
                            .iter()
                            .map(|(label, _)| label.clone())
                            .collect(),
                        self.ignore_focus,
                    ),
                    Some(0),
                ),
            },
            Action::Privacy => (
                self.privacy
                    .lines()
                    .map(|l| Line::from(l.to_string()))
                    .collect(),
                None,
            ),
        }
    }

    fn footer(&self, style: &Style, width: u16) -> Line<'static> {
        let dim = paint(style, Paint::new().add_modifier(Modifier::DIM));
        let dot = style.dot();
        if let Some(note) = &self.note {
            return Line::from(Span::styled(note.clone(), dim));
        }
        if self.help {
            return Line::from(Span::styled(
                format!(
                    "w f a i p sections{dot}Tab next{dot}{} insert{dot}c copy{dot}↑↓ scroll{dot}esc close",
                    style.enter()
                ),
                dim,
            ));
        }
        match self.section {
            Action::Agent => Line::from(Span::styled(
                match self.agents.get(self.agent_focus) {
                    Some(name) => format!(
                        "{} inserts kintsu agent --with {name}; Enter then starts it",
                        style.enter()
                    ),
                    None => format!("kintsu setup{dot}esc closes"),
                },
                dim,
            )),
            Action::Ignore => Line::from(Span::styled(
                if self.ignored.is_some() {
                    "esc closes".to_string()
                } else {
                    format!("{} makes it quiet", style.enter())
                },
                dim,
            )),
            _ => match self.known_fix() {
                Some(fix) => {
                    let pointer = if style.ascii { ">" } else { "▸" };
                    let keys = format!("Insert {}{dot}Copy c", style.enter());
                    let room = (width as usize).saturating_sub(keys.len() + 3);
                    let command = style.abbreviate(fix.command().as_str(), room);
                    let padding = room.saturating_sub(command.chars().count()) + 1;
                    Line::from(vec![
                        Span::styled(format!("{pointer} "), paint(style, Paint::new().fg(GOLD))),
                        Span::styled(
                            command,
                            paint(style, Paint::new().add_modifier(Modifier::BOLD)),
                        ),
                        Span::raw(" ".repeat(padding)),
                        Span::styled(keys, dim),
                    ])
                }
                None => Line::from(Span::styled(
                    format!("f for a fix{dot}a for an agent{dot}? keys"),
                    dim,
                )),
            },
        }
    }
}

fn paint(style: &Style, wanted: Paint) -> Paint {
    if style.color { wanted } else { Paint::new() }
}

fn step(current: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (current as isize + delta).rem_euclid(len as isize) as usize
}

fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// How many rows `lines` take once wrapped at `width`.
fn wrapped_rows(lines: &[Line<'_>], width: u16) -> u16 {
    let width = width.max(1) as usize;
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width) as u16)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{
        CaseId, CommandLine, CommandOutcome, Confidence, ExitStatus, Explanation, FixSource,
        Timestamp, UiMode,
    };
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn case(text: &str, cwd: Option<&str>) -> FailureCase {
        let outcome = CommandOutcome::new(CommandLine::new(text).unwrap(), ExitStatus::new(1))
            .lasting(Duration::from_millis(12_000));
        FailureCase::new(
            CaseId::new("c"),
            Timestamp::from_millis(0),
            outcome,
            cwd.map(String::from),
        )
    }

    fn fix(text: &str) -> Fix {
        Fix::new(
            CommandLine::new(text).unwrap(),
            Confidence::new(0.9),
            FixSource::Rule("typo".into()),
            "the closest known command",
        )
    }

    fn rows(panel: &mut Panel, style: &Style, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
        terminal.draw(|frame| panel.render(frame, style)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    const COLOUR: Style = Style {
        color: true,
        ascii: false,
        mode: UiMode::Toast,
        links: false,
    };

    #[test]
    fn opens_on_a_known_fix_and_enter_inserts_it() {
        let mut panel = Panel::new(
            &case("gti status", None),
            Some(fix("git status")),
            false,
            vec![],
            None,
            String::new(),
        );
        assert_eq!(panel.open(), Effect::Nothing);
        assert_eq!(panel.section(), Action::Fix);
        let screen = rows(&mut panel, &Style::PLAIN, 8);
        assert_eq!(
            screen[0],
            "| gti status - exit 1 - 12 s                                                 esc"
        );
        assert_eq!(
            screen[1],
            "| Why w   Fix f   Agent a   Ignore i   Privacy p"
        );
        assert_eq!(screen[2], "| git status");
        assert_eq!(screen[3], "| rule typo - 90% confident");
        assert_eq!(screen[5], "| the closest known command");
        assert!(
            screen[7].starts_with("| > git status") && screen[7].ends_with("Insert Enter - Copy c"),
            "{}",
            screen[7]
        );
        assert_eq!(panel.press(Key::Enter), Effect::Insert("git status".into()));
        assert_eq!(panel.press(Key::Copy), Effect::Copy("git status".into()));
        assert_eq!(panel.press(Key::Close), Effect::Close);
    }

    #[test]
    fn without_a_fix_it_opens_on_why_and_asks_the_models_as_sections_are_entered() {
        let mut panel = Panel::new(
            &case("make test", None),
            None,
            true,
            vec![],
            None,
            String::new(),
        );
        assert_eq!(panel.open(), Effect::Ask(Ask::Explain));
        assert_eq!(
            panel.height(80, &Style::PLAIN),
            CHROME + ASKING_ROOM,
            "room for the answer"
        );
        panel.receive(Arrival::Asking(Ask::Explain, "local".into()));
        let screen = rows(&mut panel, &Style::PLAIN, 9);
        assert_eq!(screen[2], "| asking local...");
        assert_eq!(
            panel.press(Key::Enter),
            Effect::Nothing,
            "nothing to insert yet"
        );
        panel.receive(Arrival::Explanation(Ok((
            "local".into(),
            "Node is too old.\nUse 22.".into(),
        ))));
        let screen = rows(&mut panel, &Style::PLAIN, 9);
        assert_eq!(
            (screen[2].as_str(), screen[3].as_str(), screen[4].as_str()),
            ("| Node is too old.", "| Use 22.", "| — local")
        );
        assert_eq!(screen[8], "| f for a fix - a for an agent - ? keys");
        assert_eq!(
            panel.press(Key::Section(Action::Fix)),
            Effect::Ask(Ask::Fix)
        );
        panel.receive(Arrival::Asking(Ask::Fix, "local".into()));
        assert_eq!(rows(&mut panel, &Style::PLAIN, 9)[2], "| asking local...");
        let model_fix = Fix::new(
            CommandLine::new("nvm use 22").unwrap(),
            Confidence::new(0.6),
            FixSource::Model("local".into()),
            "",
        );
        panel.receive(Arrival::Fix(Ok(Some(model_fix))));
        let screen = rows(&mut panel, &Style::PLAIN, 9);
        assert_eq!(screen[3], "| local - not verified - 60% confident");
        assert_eq!(panel.press(Key::Enter), Effect::Insert("nvm use 22".into()));
        assert_eq!(
            panel.press(Key::Section(Action::Why)),
            Effect::Nothing,
            "asked once"
        );
        panel.receive(Arrival::Fix(Err("no model answered".into())));
        panel.press(Key::Section(Action::Fix));
        assert_eq!(rows(&mut panel, &Style::PLAIN, 9)[2], "| no model answered");
    }

    #[test]
    fn opens_on_the_explanation_the_case_already_holds_without_asking_again() {
        let explained = case("make test", None)
            .with_explanation(Explanation::new("local", "The target is missing."));
        let mut panel = Panel::new(&explained, None, true, vec![], None, String::new());
        assert_eq!(panel.open(), Effect::Nothing, "answered last time");
        assert_eq!(panel.section(), Action::Why);
        let screen = rows(&mut panel, &Style::PLAIN, 8);
        assert_eq!(
            (screen[2].as_str(), screen[3].as_str()),
            ("| The target is missing.", "| — local")
        );
    }

    #[test]
    fn without_a_quick_fix_model_the_fix_section_says_so_instead_of_asking() {
        let mut panel = Panel::new(
            &case("make test", None),
            None,
            false,
            vec![],
            None,
            String::new(),
        );
        panel.open();
        assert_eq!(panel.press(Key::Section(Action::Fix)), Effect::Nothing);
        let screen = rows(&mut panel, &Style::PLAIN, 9);
        assert_eq!(
            screen[2],
            "| No rule knows this one, and no model is routed for quick fixes."
        );
        assert_eq!(screen[3], "| kintsu setup adds a model.");
        panel.receive(Arrival::Fix(Ok(None)));
        assert_eq!(
            rows(&mut panel, &Style::PLAIN, 9)[2],
            "| No fix known for this one."
        );
    }

    #[test]
    fn sections_cycle_with_tab_and_the_letters_jump() {
        let mut panel = Panel::new(
            &case("make", None),
            Some(fix("make all")),
            false,
            vec![],
            None,
            "cmd: make".into(),
        );
        panel.open();
        assert_eq!(panel.section(), Action::Fix);
        panel.press(Key::Next);
        assert_eq!(panel.section(), Action::Agent);
        panel.press(Key::Prev);
        panel.press(Key::Prev);
        assert_eq!(panel.section(), Action::Why);
        panel.press(Key::Prev);
        assert_eq!(panel.section(), Action::Privacy, "wraps");
        assert_eq!(rows(&mut panel, &Style::PLAIN, 7)[2], "| cmd: make");
        assert_eq!(panel.press(Key::Help), Effect::Nothing);
        assert!(rows(&mut panel, &Style::PLAIN, 7)[6].contains("w f a i p sections"));
    }

    #[test]
    fn the_agent_section_lists_agents_and_enter_inserts_the_command() {
        let mut panel = Panel::new(
            &case("make", None),
            None,
            false,
            vec!["claude".into(), "codex".into()],
            Some("codex"),
            String::new(),
        );
        panel.open();
        panel.press(Key::Section(Action::Agent));
        let screen = rows(&mut panel, &Style::PLAIN, 8);
        assert_eq!(
            (screen[3].as_str(), screen[4].as_str()),
            ("|   claude", "| > codex")
        );
        assert!(
            screen[7].contains("inserts kintsu agent --with codex"),
            "{}",
            screen[7]
        );
        assert_eq!(
            panel.press(Key::Enter),
            Effect::Insert("kintsu agent --with codex".into())
        );
        panel.press(Key::Up);
        assert_eq!(
            panel.press(Key::Enter),
            Effect::Insert("kintsu agent --with claude".into())
        );
        panel.press(Key::Up);
        assert_eq!(
            panel.press(Key::Enter),
            Effect::Insert("kintsu agent --with codex".into()),
            "wraps"
        );

        let mut single = Panel::new(
            &case("make", None),
            None,
            false,
            vec!["claude".into()],
            None,
            String::new(),
        );
        single.press(Key::Section(Action::Agent));
        assert_eq!(
            single.press(Key::Enter),
            Effect::Insert("kintsu agent".into())
        );

        let mut none = Panel::new(
            &case("make", None),
            None,
            false,
            vec![],
            None,
            String::new(),
        );
        none.press(Key::Section(Action::Agent));
        assert_eq!(none.press(Key::Enter), Effect::Nothing);
        assert_eq!(
            rows(&mut none, &Style::PLAIN, 7)[2],
            "| No agent configured."
        );
    }

    #[test]
    fn the_ignore_section_offers_scopes_and_closes_once_something_is_quiet() {
        let mut panel = Panel::new(
            &case("make test", Some("/w/app")),
            None,
            false,
            vec![],
            None,
            String::new(),
        );
        panel.open();
        panel.press(Key::Section(Action::Ignore));
        let screen = rows(&mut panel, &Style::PLAIN, 9);
        assert_eq!(screen[2], "| > this exact command line");
        assert_eq!(screen[3], "|   make in this directory");
        assert_eq!(screen[6], "|   everything for an hour");
        assert_eq!(
            panel.press(Key::Enter),
            Effect::Ignore(IgnoreRequest::Last {
                program: None,
                scope: ScopeChoice::Command
            })
        );
        panel.press(Key::Down);
        assert_eq!(
            panel.press(Key::Enter),
            Effect::Ignore(IgnoreRequest::Last {
                program: Some("make".into()),
                scope: ScopeChoice::Directory("/w/app".into())
            })
        );
        for _ in 0..3 {
            panel.press(Key::Down);
        }
        assert_eq!(
            panel.press(Key::Enter),
            Effect::Ignore(IgnoreRequest::Mute(HOUR))
        );
        panel.receive(Arrival::Ignored("make test stays quiet everywhere.".into()));
        let screen = rows(&mut panel, &Style::PLAIN, 9);
        assert_eq!(screen[2], "| make test stays quiet everywhere.");
        assert_eq!(screen[8], "| esc closes");
        assert_eq!(panel.press(Key::Enter), Effect::Close);
    }

    #[test]
    fn copy_without_a_fix_leaves_a_note_and_notes_show_in_the_footer() {
        let mut panel = Panel::new(&case("make", None), None, true, vec![], None, String::new());
        panel.open();
        assert_eq!(panel.press(Key::Copy), Effect::Nothing);
        assert_eq!(
            rows(&mut panel, &Style::PLAIN, 7)[6],
            "| nothing to copy yet"
        );
        panel.receive(Arrival::Note("copied".into()));
        assert_eq!(rows(&mut panel, &Style::PLAIN, 7)[6], "| copied");
    }

    #[test]
    fn clicks_land_on_tabs_and_rows() {
        let mut panel = Panel::new(
            &case("make", Some("/w")),
            None,
            false,
            vec!["claude".into(), "codex".into()],
            None,
            String::new(),
        );
        panel.open();
        rows(&mut panel, &Style::PLAIN, 9);
        let (agent_tab, _) = panel
            .tab_areas
            .iter()
            .find(|(_, a)| *a == Action::Agent)
            .copied()
            .unwrap();
        assert_eq!(
            panel.press(Key::Click {
                column: agent_tab.x,
                row: agent_tab.y
            }),
            Effect::Nothing
        );
        assert_eq!(panel.section(), Action::Agent);
        rows(&mut panel, &Style::PLAIN, 9);
        let (second_row, _) = panel
            .row_areas
            .iter()
            .find(|(_, i)| *i == 1)
            .copied()
            .unwrap();
        panel.press(Key::Click {
            column: second_row.x + 3,
            row: second_row.y,
        });
        assert_eq!(
            panel.press(Key::Enter),
            Effect::Insert("kintsu agent --with codex".into())
        );
        assert_eq!(
            panel.press(Key::Click { column: 79, row: 8 }),
            Effect::Nothing,
            "empty space"
        );
    }

    #[test]
    fn long_content_scrolls_and_the_height_stays_within_bounds() {
        let privacy: String = (1..=40).map(|i| format!("line {i}\n")).collect();
        let mut panel = Panel::new(
            &case("make", None),
            Some(fix("make all")),
            false,
            vec![],
            None,
            privacy,
        );
        panel.open();
        assert_eq!(
            panel.height(80, &Style::PLAIN),
            7,
            "fix: three chrome rows plus the fix's four lines"
        );
        panel.press(Key::Section(Action::Privacy));
        assert_eq!(panel.height(80, &Style::PLAIN), MAX_HEIGHT);
        let screen = rows(&mut panel, &Style::PLAIN, 9);
        assert_eq!(screen[2], "| line 1");
        panel.press(Key::Down);
        panel.press(Key::PageDown);
        assert_eq!(rows(&mut panel, &Style::PLAIN, 9)[2], "| line 7");
        for _ in 0..20 {
            panel.press(Key::PageDown);
        }
        let screen = rows(&mut panel, &Style::PLAIN, 9);
        assert_eq!(screen[7], "| line 40", "scrolling stops at the end");
        panel.press(Key::PageUp);
        panel.press(Key::Up);
        assert_eq!(rows(&mut panel, &Style::PLAIN, 9)[2], "| line 29");
        assert_eq!(
            Panel::new(
                &case("make", None),
                None,
                false,
                vec![],
                None,
                String::new()
            )
            .height(80, &Style::PLAIN),
            CHROME + ASKING_ROOM
        );
    }

    #[test]
    fn colour_paints_the_seam_gold_and_plain_paints_nothing() {
        let mut panel = Panel::new(
            &case("make", None),
            Some(fix("make all")),
            false,
            vec![],
            None,
            String::new(),
        );
        panel.open();
        let mut terminal = Terminal::new(TestBackend::new(80, 7)).unwrap();
        terminal.draw(|frame| panel.render(frame, &COLOUR)).unwrap();
        let cell = &terminal.backend().buffer()[(0, 0)];
        assert_eq!((cell.symbol(), cell.fg), ("▎", GOLD));
        terminal
            .draw(|frame| panel.render(frame, &Style::PLAIN))
            .unwrap();
        let cell = &terminal.backend().buffer()[(0, 0)];
        assert_eq!((cell.symbol(), cell.fg), ("|", Color::Reset));
        let mut tiny = Terminal::new(TestBackend::new(8, 2)).unwrap();
        tiny.draw(|frame| panel.render(frame, &COLOUR)).unwrap();
    }
}
