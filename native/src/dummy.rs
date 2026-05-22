use iced::advanced::widget::Widget;
struct Dummy;
impl<Message, Theme, R> Widget<Message, Theme, R> for Dummy {
    fn update(&mut self) {}
}
