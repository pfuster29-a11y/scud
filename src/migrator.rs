use std::fs;
use std::path::Path;
use crate::backup::SystemBackup;

pub struct Migrator {
    sources_path: String,
}

impl Migrator {
    pub fn new(sources_path: &str) -> Self {
        Self {
            sources_path: sources_path.to_string(),
        }
    }

    /// Prepara la migración respaldando fuentes y cambiando el codename a Sid
    pub fn prepare_migration(&self, target_codename: &str) -> Result<(), String> {
        let path = Path::new(&self.sources_path);
        if !path.exists() {
            return Err(format!("El archivo de fuentes no existe en: {}", self.sources_path));
        }

        // 1. Crear respaldo previo usando el módulo backup (.bak)
        println!("[Migrator] Generando respaldo de seguridad de las fuentes de APT...");
        SystemBackup::create_sources_backup(&self.sources_path)?;

        // 2. Leer y modificar el archivo sources.list preservando argumentos (non-free, contrib, etc.)
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Error al leer las fuentes de APT: {}", e))?;

        let mut updated_lines = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("deb ") || trimmed.starts_with("deb-src ") {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 3 {
                    let mut new_parts = parts.clone();
                    // Reemplaza el codename actual (ej. trixie) manteniendo componentes posteriores (main, non-free...)
                    new_parts[2] = target_codename;
                    updated_lines.push(new_parts.join(" "));
                } else {
                    updated_lines.push(line.to_string());
                }
            } else {
                updated_lines.push(line.to_string());
            }
        }

        let new_content = updated_lines.join("\n") + "\n";

        // 3. Escribir los cambios
        fs::write(path, new_content)
            .map_err(|e| format!("Error al escribir el nuevo sources.list: {}", e))?;

        println!("[Migrator] Migración a '{}' preparada con éxito.", target_codename);
        Ok(())
    }
}