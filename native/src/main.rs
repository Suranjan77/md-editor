mod app;
mod editor;
mod messages;
mod theme;
mod views;

fn main() -> iced::Result {
    iced::application(app::MdEditor::new, app::MdEditor::update, app::MdEditor::view)
        .title(app::MdEditor::title)
        .theme(|state: &app::MdEditor| state.theme())
        .subscription(app::MdEditor::subscription)
        .window_size(iced::Size::new(1200.0, 800.0))
        .run()
}
