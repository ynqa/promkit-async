use promkit::crossterm::event::Event;

#[derive(Clone, Debug, PartialEq)]
pub enum EventGroup {
    KeyBuffer(Vec<char>),
    VerticalCursorBuffer(usize, usize),   // (up, down)
    HorizontalCursorBuffer(usize, usize), // (left, right)
    LastResize(u16, u16),                 // (width, height)
    Others(Event, usize),
}
