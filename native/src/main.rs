mod app;
mod editor;
mod messages;
mod search;
mod theme;
mod views;

fn main() -> iced::Result {
    let icon = iced::window::icon::from_file_data(
        include_bytes!("../../app-icon.png"),
        Some(image::ImageFormat::Png),
    )
    .ok();

    iced::application(
        app::MdEditor::new,
        app::MdEditor::update,
        app::MdEditor::view,
    )
    .title(app::MdEditor::title)
    .theme(|state: &app::MdEditor| state.theme())
    .subscription(app::MdEditor::subscription)
    .window(iced::window::Settings {
        size: iced::Size::new(1200.0, 800.0),
        icon,
        ..Default::default()
    })
    .run()
}
