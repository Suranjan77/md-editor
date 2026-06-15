use super::*;
use iced::widget::{column, row, stack};

pub(super) const BOLD: iced::Font = super::fonts::SANS_BOLD;

impl Shell {
    pub(super) fn layout_view<'a>(&'a self, node: &Layout<'a>) -> Element<'a, Message> {
        self.layout_view_at(node, Vec::new())
    }

    pub(super) fn layout_view_at<'a>(
        &'a self,
        node: &Layout<'a>,
        path: SplitPath,
    ) -> Element<'a, Message> {
        match node {
            Layout::Pane(pane) => self.pane_view(pane),
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let mut first_path = path.clone();
                first_path.push(false);
                let mut second_path = path.clone();
                second_path.push(true);
                let a = container(self.layout_view_at(first, first_path))
                    .width(Fill)
                    .height(Fill);
                let b = container(self.layout_view_at(second, second_path))
                    .width(Fill)
                    .height(Fill);
                let divider = drag::divider(path, *axis, *ratio, self.tokens());
                let (pa, pb) = (
                    ((ratio * 1000.0) as u16).max(1),
                    (((1.0 - ratio) * 1000.0) as u16).max(1),
                );
                match axis {
                    SplitAxis::Horizontal => row![
                        a.width(iced::Length::FillPortion(pa)),
                        divider,
                        b.width(iced::Length::FillPortion(pb))
                    ]
                    .spacing(0)
                    .into(),
                    SplitAxis::Vertical => column![
                        a.height(iced::Length::FillPortion(pa)),
                        divider,
                        b.height(iced::Length::FillPortion(pb))
                    ]
                    .spacing(0)
                    .into(),
                }
            }
        }
    }

    pub(super) fn pane_view<'a>(&'a self, pane: &Pane) -> Element<'a, Message> {
        let tokens = self.tokens();
        let focused_tab = self.ws.focused_tab();
        let pane_focused = self.ws.focused_pane() == Some(pane.id);

        let mut tabs = row![].spacing(2);
        for tab in pane.tabs() {
            let title = self
                .ws
                .docs
                .get(tab.document)
                .map(|d| {
                    let name = d.path.rsplit('/').next().unwrap_or(&d.path);
                    let dirty = self
                        .sessions
                        .md
                        .get(&tab.document)
                        .is_some_and(|s| s.doc.buffer().is_dirty());
                    if dirty {
                        format!("{name} ●")
                    } else {
                        name.to_string()
                    }
                })
                .unwrap_or_else(|| "?".to_string());
            let active =
                focused_tab == Some(tab.id) || pane.active_tab().map(|t| t.id) == Some(tab.id);
            let select = button(text(title).size(13))
                .padding([3, 8])
                .style(move |theme, status| {
                    if active && pane_focused {
                        button::primary(theme, status)
                    } else if active {
                        button::secondary(theme, status)
                    } else {
                        button::text(theme, status)
                    }
                })
                .on_press(Message::TabSelected(tab.id));
            let close = button(text("×").size(13))
                .padding([3, 6])
                .style(button::text)
                .on_press(Message::TabCloseClicked(tab.id));
            tabs = tabs.push(
                iced::widget::mouse_area(row![select, close].spacing(0))
                    .on_middle_press(Message::TabCloseClicked(tab.id)),
            );
        }
        tabs = tabs.push(
            button(text("+").size(15))
                .padding([2, 8])
                .style(button::text)
                .on_press(Message::RunCommand(CommandId("file.quick-open"))),
        );
        // Tabs scroll horizontally on overflow, but the default scrollbar is a
        // fat, distracting rail; use a thin one and fade the track so the strip
        // reads as tabs, not a scroll region.
        let tabs = iced::widget::scrollable(tabs)
            .direction(iced::widget::scrollable::Direction::Horizontal(
                iced::widget::scrollable::Scrollbar::new()
                    .width(3.0)
                    .scroller_width(3.0)
                    .margin(0.0),
            ))
            .style(|theme: &iced::Theme, status| {
                let mut style = iced::widget::scrollable::default(theme, status);
                style.horizontal_rail.background = None;
                style.horizontal_rail.border = iced::Border::default();
                style
            });
        let pane_action = |icon, command| {
            button(super::icons::view(icon, tokens.text_secondary, 15.0))
                .padding([4, 6])
                .style(button::text)
                .on_press(Message::PaneCommand {
                    pane: pane.id,
                    command,
                })
        };
        let strip = row![
            container(tabs).width(Fill),
            pane_action(
                super::icons::Icon::Split,
                CommandId("workspace.split-right")
            ),
            pane_action(
                super::icons::Icon::SplitDown,
                CommandId("workspace.split-down")
            ),
            pane_action(super::icons::Icon::Close, CommandId("workspace.close-pane")),
        ]
        .spacing(2)
        .padding(2)
        .align_y(iced::Alignment::Center);

        let content: Element<'_, Message> = match pane.active_tab() {
            None => {
                let mut welcome = column![
                    text("MD Editor").size(24).color(tokens.accent),
                    text("Open a note or browse the vault to begin.")
                        .size(14)
                        .color(tokens.text_muted)
                ]
                .spacing(10)
                .width(320);
                for item in welcome::welcome_rows(&self.registry) {
                    let chord = item.chord.unwrap_or_default();
                    welcome = welcome.push(
                        button(
                            row![
                                text(item.label).size(14),
                                iced::widget::Space::new().width(Fill),
                                text(chord).size(12).color(tokens.text_muted)
                            ]
                            .width(Fill),
                        )
                        .width(Fill)
                        .padding([8, 12])
                        .on_press(Message::RunCommand(item.command)),
                    );
                }
                container(welcome).center(Fill).into()
            }
            Some(tab) => {
                let focused = focused_tab == Some(tab.id);
                match tab.editor {
                    EditorKind::Markdown => match self.sessions.md.get(&tab.document) {
                        Some(session) => {
                            let editor = canvas(EditorCanvas {
                                tab: tab.id,
                                session,
                                tokens,
                                focused,
                                reduce_motion: self.reduce_motion,
                            })
                            .width(Fill)
                            .height(Fill);

                            // Formatting controls float at the bottom of the
                            // editor (mirrors the PDF reader bar) instead of a
                            // permanent top toolbar that crowds the page.
                            let editor_stack = stack![editor, floating_format_bar(tokens)]
                                .width(Fill)
                                .height(Fill);

                            let mut view_col = column![];
                            if session.find_open {
                                view_col =
                                    view_col.push(self.view_md_find_replace_bar(session, tab.id));
                            }
                            view_col.push(editor_stack).into()
                        }
                        None => missing_session(tokens),
                    },
                    EditorKind::Pdf => match self.sessions.pdf.get(&tab.document) {
                        Some(session) => pdf_view::view(session, tab.id, tokens),
                        None => missing_session(tokens),
                    },
                    _ => container(text("unsupported editor kind").color(tokens.text_muted))
                        .center(Fill)
                        .into(),
                }
            }
        };

        let border_color = if pane_focused {
            tokens.accent
        } else {
            tokens.border
        };
        container(column![strip, container(content).height(Fill)])
            .style(move |_| container::Style {
                border: iced::Border {
                    color: border_color,
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..container::Style::default()
            })
            .width(Fill)
            .height(Fill)
            .into()
    }
}

pub(super) fn missing_session<'a>(tokens: &'static tokens::Tokens) -> Element<'a, Message> {
    container(text("document failed to load").color(tokens.text_muted))
        .center(Fill)
        .into()
}

/// The bottom-floating Markdown formatting bar — same rounded, centered chrome
/// as the PDF reader's control bar, overlaid on the editor canvas.
fn floating_format_bar<'a>(tokens: &'static tokens::Tokens) -> Element<'a, Message> {
    let action = |label: &'a str, bold: bool, command: &'static str| {
        let mut t = text(label).size(13);
        if bold {
            t = t.font(BOLD);
        }
        button(t)
            .padding([5, 9])
            .style(button::text)
            .on_press(Message::RunCommand(CommandId(command)))
    };
    let bar = container(
        row![
            action("B", true, "editor.toggle-bold"),
            action("I", false, "editor.toggle-italic"),
            action("Code", false, "editor.toggle-code"),
            action("H", true, "editor.heading-cycle"),
            action("List", false, "editor.toggle-bullet"),
            action("Todo", false, "editor.toggle-checkbox"),
            action("Link", false, "editor.toggle-wikilink"),
        ]
        .spacing(2)
        .align_y(iced::Alignment::Center),
    )
    .padding(4)
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(tokens.bg_secondary)),
        border: iced::Border {
            color: tokens.border,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    });
    container(bar)
        .width(Fill)
        .height(Fill)
        .padding(iced::Padding {
            top: 0.0,
            right: 0.0,
            bottom: 14.0,
            left: 0.0,
        })
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Bottom)
        .into()
}
