use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

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

/// Mensajes emitidos mientras corre un comando apt en modo streaming.
/// `Output` se manda por cada línea de stdout/stderr a medida que aparece,
/// `Done` se manda una sola vez al terminar el proceso.
pub enum AptLine {
    Output(String),
    Done(Result<(), String>),
}

/// Ejecuta un comando apt-get privilegiado (vía pkexec) transmitiendo su
/// salida línea por línea a través de `sender`, en lugar de esperar a que
/// termine todo el proceso. Pensado para alimentar una terminal en vivo en la UI.
///
/// Usa DEBIAN_FRONTEND=noninteractive + force-confdef/force-confold para
/// evitar que apt se quede esperando un prompt de conffiles durante un
/// full-upgrade masivo (una causa típica de "cuelgues" o códigos de salida
/// raros en actualizaciones con miles de paquetes).
pub fn run_privileged_apt_streaming(args: &[&str], sender: Sender<AptLine>) {
    let mut cmd = Command::new("pkexec");
    cmd.arg("env")
        .arg("DEBIAN_FRONTEND=noninteractive")
        .arg("apt-get")
        .arg("-o")
        .arg("Dpkg::Options::=--force-confdef")
        .arg("-o")
        .arg("Dpkg::Options::=--force-confold");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = sender.send(AptLine::Done(Err(format!(
                "Error al invocar pkexec: {}",
                e
            ))));
            return;
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Leemos stdout y stderr en hilos separados para no bloquear si uno de
    // los dos buffers se llena mientras el otro está inactivo.
    let sender_out = sender.clone();
    let stdout_handle = stdout.map(|out| {
        std::thread::spawn(move || {
            let reader = BufReader::new(out);
            for line in reader.lines().map_while(Result::ok) {
                let _ = sender_out.send(AptLine::Output(line));
            }
        })
    });

    let sender_err = sender.clone();
    let stderr_handle = stderr.map(|err| {
        std::thread::spawn(move || {
            let reader = BufReader::new(err);
            for line in reader.lines().map_while(Result::ok) {
                let _ = sender_err.send(AptLine::Output(format!("⚠ {}", line)));
            }
        })
    });

    if let Some(h) = stdout_handle {
        let _ = h.join();
    }
    if let Some(h) = stderr_handle {
        let _ = h.join();
    }

    match child.wait() {
        Ok(status) if status.success() => {
            let _ = sender.send(AptLine::Done(Ok(())));
        }
        Ok(status) => {
            let _ = sender.send(AptLine::Done(Err(format!(
                "apt-get terminó con código de salida {:?}. Revisá el output de arriba: \
                 puede ser un error real o solo un hook post-instalación no fatal.",
                status.code()
            ))));
        }
        Err(e) => {
            let _ = sender.send(AptLine::Done(Err(format!(
                "Error esperando el proceso: {}",
                e
            ))));
        }
    }
}

/// Pone o saca la marca de "retenido" (hold) sobre una lista de paquetes usando
/// `apt-mark`. Se usa para excluir puntualmente, de una corrida de `full-upgrade`,
/// los paquetes que el usuario decidió no actualizar (los que destildó en la lista).
/// `action` debe ser "hold" o "unhold".
///
/// Si `packages` está vacío, no hace nada (no hay nada que retener/liberar) y
/// devuelve éxito directamente, sin pedir contraseña de más.
pub fn run_apt_mark(action: &str, packages: &[String]) -> Result<(), String> {
    if packages.is_empty() {
        return Ok(());
    }

    let mut cmd = Command::new("pkexec");
    cmd.arg("apt-mark").arg(action);
    for pkg in packages {
        cmd.arg(pkg);
    }

    let status = cmd
        .status()
        .map_err(|e| format!("Error al invocar pkexec para apt-mark {}: {}", action, e))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "apt-mark {} falló o fue cancelado por el usuario.",
            action
        ))
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