pub mod help;
pub mod theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modal {
    Help,
    ThemePicker,
}
