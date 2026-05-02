pub mod export;
pub mod help;
pub mod theme;

#[derive(Debug, Clone)]
pub enum Modal {
    Help,
    ThemePicker,
    Export(export::ExportState),
}
