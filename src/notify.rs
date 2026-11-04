//! Desktop notifications over D-Bus (GNOME, KDE, dunst, …).

pub fn send(summary: &str, body: &str) -> Result<(), String> {
    notify_rust::Notification::new()
        .appname("bus")
        .summary(summary)
        .body(body)
        .show()
        .map(|_| ())
        .map_err(|e| e.to_string())
}
