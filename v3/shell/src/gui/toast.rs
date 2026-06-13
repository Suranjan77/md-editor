use super::*;
use iced::widget::{button, column, container, row, text};

impl Shell {
    fn queue_toast(&mut self, message: String, kind: ToastKind) -> Task<Message> {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        self.toasts.push(Toast { id, kind, message });

        Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                id
            },
            Message::DismissToast,
        )
    }

    pub fn show_toast(&mut self, message: String, kind: ToastKind) -> Task<Message> {
        self.status = message.clone();
        self.queue_toast(message, kind)
    }

    pub(super) fn success_toast(&mut self, message: impl Into<String>) -> Task<Message> {
        self.queue_toast(message.into(), ToastKind::Success)
    }

    pub fn info(&mut self, message: impl Into<String>) -> Task<Message> {
        self.show_toast(message.into(), ToastKind::Info)
    }

    pub fn success(&mut self, message: impl Into<String>) -> Task<Message> {
        self.show_toast(message.into(), ToastKind::Success)
    }

    pub fn warning(&mut self, message: impl Into<String>) -> Task<Message> {
        self.show_toast(message.into(), ToastKind::Warning)
    }

    pub fn error(&mut self, message: impl Into<String>) -> Task<Message> {
        self.show_toast(message.into(), ToastKind::Error)
    }

    pub(super) fn view_toasts(&self) -> Element<'_, Message> {
        let mut col = column![].spacing(10).align_x(iced::Alignment::End);
        for toast in &self.toasts {
            let border_color = match toast.kind {
                ToastKind::Info => tokens::dark().accent,
                ToastKind::Success => tokens::dark().success,
                ToastKind::Warning => tokens::dark().warning,
                ToastKind::Error => tokens::dark().danger,
            };

            let bg_color = tokens::dark().bg_secondary;
            let text_color = tokens::dark().text_primary;
            let icon_color = border_color;

            let badge = match toast.kind {
                ToastKind::Info => "ℹ",
                ToastKind::Success => "✓",
                ToastKind::Warning => "⚠",
                ToastKind::Error => "✗",
            };

            let close_btn = button(text("×").size(14))
                .padding([2, 5])
                .style(button::text)
                .on_press(Message::CloseToastClicked(toast.id));

            let card = container(
                row![
                    text(badge).size(16).color(icon_color),
                    text(toast.message.clone())
                        .size(13)
                        .color(text_color)
                        .width(iced::Length::Fill),
                    close_btn
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center)
                .padding(12),
            )
            .width(320)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(bg_color)),
                border: iced::Border {
                    color: border_color,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..container::Style::default()
            });
            col = col.push(card);
        }

        container(iced::widget::scrollable(col))
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .align_x(iced::Alignment::End)
            .align_y(iced::Alignment::Start)
            .padding(16)
            .into()
    }
}
