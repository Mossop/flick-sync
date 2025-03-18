#[derive(Clone)]
pub(super) enum Event {
    SyncStart,
    SyncEnd,
}

impl Event {
    pub(super) async fn to_string(&self) -> String {
        match self {
            Self::SyncStart => r#"event: sync-status
data: <sl-icon class="spin" name="arrow-repeat"></sl-icon> Syncing

"#
            .to_string(),
            Self::SyncEnd => r#"event: sync-status
data: <sl-icon name="arrow-repeat"></sl-icon> Syncs

"#
            .to_string(),
        }
    }
}
