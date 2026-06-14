use iced::widget::{button, column, container, mouse_area, row, stack, text};
use iced::{Background, Border, Element, Fill, Padding};
use md_kernel::input::EditorKind;
use md_kernel::{CommandId, CommandRegistry};

use super::{Message, tokens};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuId {
    File,
    Edit,
    View,
    Pdf,
    Help,
}

impl MenuId {
    pub const ALL: [MenuId; 5] = [
        MenuId::File,
        MenuId::Edit,
        MenuId::View,
        MenuId::Pdf,
        MenuId::Help,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            MenuId::File => "File",
            MenuId::Edit => "Edit",
            MenuId::View => "View",
            MenuId::Pdf => "PDF",
            MenuId::Help => "Help",
        }
    }

    fn left(self) -> f32 {
        match self {
            MenuId::File => 8.0,
            MenuId::Edit => 54.0,
            MenuId::View => 100.0,
            MenuId::Pdf => 150.0,
            MenuId::Help => 194.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub command: CommandId,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuGroup {
    pub id: MenuId,
    pub items: Vec<MenuItem>,
}

pub fn menu_model(
    registry: &CommandRegistry,
    focused_kind: Option<EditorKind>,
    has_tab: bool,
) -> Vec<MenuGroup> {
    let markdown = focused_kind == Some(EditorKind::Markdown);
    let pdf = focused_kind == Some(EditorKind::Pdf);
    let make = |id, commands: &[(CommandId, bool)]| MenuGroup {
        id,
        items: commands
            .iter()
            .filter(|(command, _)| registry.get(*command).is_some())
            .map(|(command, enabled)| MenuItem {
                command: *command,
                enabled: *enabled,
            })
            .collect(),
    };
    vec![
        make(
            MenuId::File,
            &[
                (CommandId("vault.open"), true),
                (CommandId("file.new-note"), true),
                (CommandId("file.new-folder"), true),
                (CommandId("file.rename"), has_tab),
                (CommandId("file.delete"), has_tab),
                (CommandId("file.quick-open"), true),
                (CommandId("editor.save"), markdown),
                (CommandId("workspace.close-tab"), has_tab),
                (CommandId("app.quit"), true),
            ],
        ),
        make(
            MenuId::Edit,
            &[
                (CommandId("editor.undo"), markdown),
                (CommandId("editor.redo"), markdown),
                (CommandId("editor.copy"), markdown),
                (CommandId("editor.cut"), markdown),
                (CommandId("editor.select-all"), markdown),
                (CommandId("editor.find"), markdown),
                (CommandId("editor.toggle-bold"), markdown),
                (CommandId("editor.toggle-italic"), markdown),
                (CommandId("editor.toggle-code"), markdown),
                (CommandId("editor.heading-cycle"), markdown),
                (CommandId("editor.toggle-bullet"), markdown),
                (CommandId("editor.toggle-checkbox"), markdown),
                (CommandId("editor.toggle-wikilink"), markdown),
                (CommandId("note.backlinks"), markdown),
            ],
        ),
        make(
            MenuId::View,
            &[
                (CommandId("workspace.toggle-files"), true),
                (CommandId("workspace.refresh-files"), true),
                (CommandId("workspace.collapse-files"), true),
                (CommandId("workspace.toggle-tracker"), true),
                (CommandId("workspace.split-right"), has_tab),
                (CommandId("workspace.split-down"), has_tab),
                (CommandId("workspace.close-pane"), has_tab),
                (CommandId("workspace.next-tab"), has_tab),
                (CommandId("note.outline-panel"), markdown),
                (CommandId("search.global"), true),
                (CommandId("palette.open"), true),
            ],
        ),
        make(
            MenuId::Pdf,
            &[
                (CommandId("pdf.toc"), pdf),
                (CommandId("pdf.find"), pdf),
                (CommandId("pdf.go-to-page"), pdf),
                (CommandId("pdf.previous-page"), pdf),
                (CommandId("pdf.next-page"), pdf),
                (CommandId("pdf.zoom-in"), pdf),
                (CommandId("pdf.zoom-out"), pdf),
                (CommandId("pdf.zoom-input"), pdf),
                (CommandId("pdf.fit-width"), pdf),
                (CommandId("pdf.fit-page"), pdf),
                (CommandId("pdf.toc-panel"), pdf),
                (CommandId("pdf.annotations-panel"), pdf),
                (CommandId("pdf.back"), pdf),
                (CommandId("pdf.forward"), pdf),
                (CommandId("pdf.copy-selection"), pdf),
                (CommandId("pdf.highlight"), pdf),
                (CommandId("pdf.annotation-note"), pdf),
                (CommandId("pdf.highlight-color"), pdf),
                (CommandId("pdf.highlight-and-note"), pdf),
                (CommandId("pdf.annotation-link-note"), pdf),
                (CommandId("pdf.annotations-export"), pdf),
                (CommandId("pdf.annotations-orphans"), true),
            ],
        ),
        make(MenuId::Help, &[(CommandId("help.shortcuts"), true)]),
    ]
}

pub fn bar(open: Option<MenuId>, tokens: &'static tokens::Tokens) -> Element<'static, Message> {
    let mut content = row![].spacing(1).padding([2, 6]);
    for id in MenuId::ALL {
        let active = open == Some(id);
        content = content.push(
            button(text(id.title()).size(13))
                .padding([4, 9])
                .style(move |theme, status| {
                    if active {
                        button::secondary(theme, status)
                    } else {
                        button::text(theme, status)
                    }
                })
                .on_press(Message::MenuToggled(id)),
        );
    }
    container(content)
        .width(Fill)
        .height(30)
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.bg_secondary)),
            border: Border {
                color: tokens.border_subtle,
                width: 0.0,
                radius: 0.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

pub fn popover<'a>(
    open: MenuId,
    model: Vec<MenuGroup>,
    registry: &'a CommandRegistry,
    tokens: &'static tokens::Tokens,
) -> Element<'a, Message> {
    let backdrop = container(
        mouse_area(
            container(iced::widget::Space::new())
                .width(Fill)
                .height(Fill),
        )
        .on_press(Message::MenuClosed),
    )
    .padding(Padding {
        top: 30.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    })
    .width(Fill)
    .height(Fill);
    let mut items = column![].spacing(1).padding(5);
    if let Some(group) = model.into_iter().find(|group| group.id == open) {
        for item in group.items {
            let Some(spec) = registry.get(item.command) else {
                continue;
            };
            let chord = spec
                .bindings
                .first()
                .map(|binding| binding.chord.to_string())
                .unwrap_or_default();
            let color = if item.enabled {
                tokens.text_primary
            } else {
                tokens.text_muted
            };
            let mut control = button(
                row![
                    text(spec.title).size(13).color(color),
                    iced::widget::Space::new().width(Fill),
                    text(chord).size(12).color(tokens.text_muted)
                ]
                .width(Fill),
            )
            .width(250)
            .padding([6, 9])
            .style(button::text);
            if item.enabled {
                control = control.on_press(Message::MenuCommand(item.command));
            }
            items = items.push(control);
        }
    }
    let card = container(items).style(move |_| container::Style {
        background: Some(Background::Color(tokens.bg_secondary)),
        border: Border {
            color: tokens.border,
            width: 1.0,
            radius: 5.0.into(),
        },
        ..container::Style::default()
    });
    let positioned = container(card).width(Fill).height(Fill).padding(Padding {
        top: 29.0,
        right: 0.0,
        bottom: 0.0,
        left: open.left(),
    });
    stack![backdrop, positioned].into()
}
