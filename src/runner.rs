use std::process::Command;

/// Runs `apt-get dist-upgrade --just-print` to simulate package upgrades.
pub fn run_apt_simulation() -> Result<String, String> {
    let output = Command::new("apt-get")
        .arg("dist-upgrade")
        .arg("--just-print")
        .output()
        .map_err(|e| format!("Failed to execute apt-get: {}", e))?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .map_err(|e| format!("Invalid UTF-8 output from apt-get: {}", e))
    } else {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        Err(format!("apt-get error: {}", err_msg))
    }
}

/// Runs an APT command with root privileges using pkexec.
pub fn run_privileged_apt(args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("pkexec");
    cmd.arg("apt-get");
    for arg in args {
        cmd.arg(arg);
    }

    let output = cmd.output()
        .map_err(|e| format!("Failed to execute pkexec: {}", e))?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .map_err(|e| format!("Invalid UTF-8 output: {}", e))
    } else {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        if err_msg.is_empty() {
            Err("Authentication cancelled by user or failed.".to_string())
        } else {
            Err(format!("Privileged command error: {}", err_msg))
        }
    }
}

/// Performs a full system audit: updates package lists via pkexec, then simulates upgrades.
pub fn run_full_audit() -> Result<String, String> {
    // 1. Actualizamos las listas de repositorios con privilegios de root (pedirá contraseña)
    let update_res = run_privileged_apt(&["update"]);
    if let Err(e) = update_res {
        return Err(format!("Falló la actualización de repositorios:\n{}", e));
    }

    // 2. Con las listas frescas, ejecutamos la simulación de dist-upgrade
    run_apt_simulation()
}
/// Ejecuta la actualización real de paquetes con privilegios de root usando pkexec.
pub fn run_apply_upgrades() -> Result<String, String> {
    run_privileged_apt(&["dist-upgrade", "-y"])
}