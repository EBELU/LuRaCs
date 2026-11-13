#[derive(Debug, Clone)]
pub enum RoiEvent {
    /// "Right-clicked and chose Delete", or Delete in the editor modal.
    DeleteRequested(String),
    Selected(String),
    SettingsUpdated(String),
}
