use promkit::crossterm;

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    KeyBuffer(Vec<char>),
    VerticalCursorBuffer(usize, usize),   // (up, down)
    HorizontalCursorBuffer(usize, usize), // (left, right)
    LastResize(u16, u16),                 // (width, height)
    Others(crossterm::event::Event, usize),
}
