use super::*;
use iced::widget::{column, row};

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
                let divider = drag::divider(path, *axis, *ratio);
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
        let tabs = iced::widget::scrollable(tabs).direction(
            iced::widget::scrollable::Direction::Horizontal(
                iced::widget::scrollable::Scrollbar::default(),
            ),
        );
        let pane_action = |label, command| {
            button(text(label).size(13))
                .padding([3, 6])
                .style(button::text)
                .on_press(Message::PaneCommand {
                    pane: pane.id,
                    command,
                })
        };
        let strip = row![
            container(tabs).width(Fill),
            pane_action("⇥", CommandId("workspace.split-right")),
            pane_action("⇩", CommandId("workspace.split-down")),
            pane_action("×", CommandId("workspace.close-pane")),
        ]
        .spacing(2)
        .padding(2)
        .align_y(iced::Alignment::Center);

        let content: Element<'_, Message> = match pane.active_tab() {
            None => {
                let mut welcome = column![
                    text("MD Editor").size(24).color(colors::heading()),
                    text("Open a note or browse the vault to begin.")
                        .size(14)
                        .color(colors::marker())
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
                                text(chord).size(12).color(colors::marker())
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
                            let toolbar = row![
                                button(text("B").font(BOLD).size(12))
                                    .padding([3, 8])
                                    .style(button::text)
                                    .on_press(Message::RunCommand(CommandId("editor.toggle-bold"))),
                                button(text("I").size(12))
                                    .padding([3, 8])
                                    .style(button::text)
                                    .on_press(Message::RunCommand(CommandId(
                                        "editor.toggle-italic"
                                    ))),
                                button(text("Code").size(12))
                                    .padding([3, 8])
                                    .style(button::text)
                                    .on_press(Message::RunCommand(CommandId("editor.toggle-code"))),
                                button(text("H").font(BOLD).size(12))
                                    .padding([3, 8])
                                    .style(button::text)
                                    .on_press(Message::RunCommand(CommandId(
                                        "editor.heading-cycle"
                                    ))),
                                button(text("List").size(12))
                                    .padding([3, 8])
                                    .style(button::text)
                                    .on_press(Message::RunCommand(CommandId(
                                        "editor.toggle-bullet"
                                    ))),
                                button(text("Todo").size(12))
                                    .padding([3, 8])
                                    .style(button::text)
                                    .on_press(Message::RunCommand(CommandId(
                                        "editor.toggle-checkbox"
                                    ))),
                                button(text("Link").size(12))
                                    .padding([3, 8])
                                    .style(button::text)
                                    .on_press(Message::RunCommand(CommandId(
                                        "editor.toggle-wikilink"
                                    ))),
                            ]
                            .spacing(2)
                            .padding(2)
                            .align_y(iced::Alignment::Center);

                            let toolbar_container =
                                container(toolbar).width(Fill).style(|_| container::Style {
                                    background: Some(iced::Background::Color(
                                        tokens::dark().bg_secondary,
                                    )),
                                    border: iced::Border {
                                        color: tokens::dark().border_subtle,
                                        width: 1.0,
                                        radius: 0.0.into(),
                                    },
                                    ..container::Style::default()
                                });

                            let editor = canvas(EditorCanvas {
                                tab: tab.id,
                                session,
                                focused,
                            })
                            .width(Fill)
                            .height(Fill);

                            let mut view_col = column![toolbar_container];
                            if session.find_open {
                                view_col =
                                    view_col.push(self.view_md_find_replace_bar(session, tab.id));
                            }
                            view_col.push(editor).into()
                        }
                        None => missing_session(),
                    },
                    EditorKind::Pdf => match self.sessions.pdf.get(&tab.document) {
                        Some(session) => pdf_view::view(session, tab.id),
                        None => missing_session(),
                    },
                    _ => container(text("unsupported editor kind").color(colors::marker()))
                        .center(Fill)
                        .into(),
                }
            }
        };

        let border_color = if pane_focused {
            tokens::dark().accent
        } else {
            tokens::dark().border
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

pub(super) fn missing_session<'a>() -> Element<'a, Message> {
    container(text("document failed to load").color(colors::marker()))
        .center(Fill)
        .into()
}
