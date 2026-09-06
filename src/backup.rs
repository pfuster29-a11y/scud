use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Crea un respaldo de la selección actual de paquetes del sistema.
/// Retorna la ruta absoluta del archivo de respaldo generado.
pub fn create_package_backup() -> Result<String, String> {
    let home = std::env::var("HOME").map_err(|_| "No se pudo encontrar la variable HOME")?;
    let backup_dir = PathBuf::from(home).join(".local/share/scud/backups");

    fs::create_dir_all(&backup_dir)
        .map_err(|e| format!("No se pudo crear el directorio de respaldos: {}", e))?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("Error de tiempo del sistema: {}", e))?
        .as_secs();

    let backup_file = backup_dir.join(format!("selections_{}.txt", timestamp));

    let output = Command::new("dpkg")
        .arg("--get-selections")
        .output()
        .map_err(|e| format!("Error al ejecutar dpkg: {}", e))?;

    if output.status.success() {
        fs::write(&backup_file, &output.stdout)
            .map_err(|e| format!("Error al escribir el archivo de respaldo: {}", e))?;
        Ok(backup_file.to_string_lossy().to_string())
    } else {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        Err(format!("Fallo al obtener selecciones de paquetes: {}", err_msg))
    }
}