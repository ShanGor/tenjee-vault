//! Explain Linux single-instance forwarding before the plugin exits the new process.

fn existing_instance_pid(identifier: &str) -> Option<u32> {
    let connection = zbus::blocking::Connection::session().ok()?;
    let reply = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetConnectionUnixProcessID",
            &(format!("{identifier}.SingleInstance"),),
        )
        .ok()?;
    reply.body().deserialize().ok()
}

pub fn report_existing_instance(identifier: &str) {
    if let Some(pid) = existing_instance_pid(identifier) {
        eprintln!(
            "Tenjee Vault is already running (PID {pid}). This launch will focus that process \
             and exit; the newly built app will not start. Use the existing app's Quit action, \
             then launch this build again."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::existing_instance_pid;

    #[test]
    fn detects_only_the_registered_single_instance_owner() {
        let identifier = format!("com.sam.tenjee_vault.test_{}", std::process::id());
        assert_eq!(existing_instance_pid(&identifier), None);

        let connection = zbus::blocking::connection::Builder::session()
            .unwrap()
            .name(format!("{identifier}.SingleInstance"))
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(existing_instance_pid(&identifier), Some(std::process::id()));

        connection
            .release_name(format!("{identifier}.SingleInstance"))
            .unwrap();
        assert_eq!(existing_instance_pid(&identifier), None);
    }
}
