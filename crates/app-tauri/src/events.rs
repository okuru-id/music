use application::{DomainEvent, EventPublisher};
use tauri::{AppHandle, Emitter};

/// Bridges domain events to the webview via Tauri's emit.
pub struct TauriEvents {
    app: AppHandle,
}

impl TauriEvents {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl EventPublisher for TauriEvents {
    fn publish(&self, e: DomainEvent) {
        // Frontend listens on "app://event"; payload is the tagged enum.
        let _ = self.app.emit("app://event", &e);
    }
}
