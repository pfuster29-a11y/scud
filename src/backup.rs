use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct SystemBackup;

impl SystemBackup {
    /// Crea una copia de respaldo segura del archivo sources.list actual (requerido por el migrator)
    pub fn create_sources_backup(sources_path: &str) -> Result<(), String> {
        let path = Path::new(sources_path);
        if !path.exists() {
            return Err(format!("El archivo de fuentes no existe: {}", sources_path));
        }

        let backup_path = format!("{}.scud.bak", sources_path);

        fs::copy(path, &backup_path)
            .map_err(|e| format!("Error al crear el respaldo de sources.list: {}", e))?;

        println!("[Backup] Respaldo de sources.list generado con éxito en: {}", backup_path);
        Ok(())
    }
}

/// Crea un respaldo de la selección actual de paquetes del sistema usando dpkg.
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