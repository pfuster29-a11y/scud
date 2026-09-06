use std::collections::HashSet;
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

    /// Prepara la migración preservando estrictamente las preferencias del usuario (componentes y repos terceros)
    /// pero adaptando el codename y filtrando los conflictos incompatibles con la rama de destino (ej. Sid).
    pub fn prepare_migration(&self, target_codename: &str) -> Result<(), String> {
        let path = Path::new(&self.sources_path);
        if !path.exists() {
            return Err(format!("El archivo de fuentes no existe en: {}", self.sources_path));
        }

        // 1. Crear respaldo previo (.bak)
        println!("[Migrator] Generando respaldo de seguridad de las fuentes de APT...");
        SystemBackup::create_sources_backup(&self.sources_path)?;

        let content = fs::read_to_string(path)
            .map_err(|e| format!("Error al leer las fuentes de APT: {}", e))?;

        let is_sid = target_codename.eq_ignore_ascii_case("sid");
        let mut updated_lines = Vec::new();
        let mut seen_lines = HashSet::new();

        // Agregar un encabezado claro indicando la autoría de Scud
        updated_lines.push(format!(
            "# =====================================================================\n\
             # Archivo sources.list modificado y gestionado automáticamente por Scud\n\
             # Rama de destino: Debian {} \n\
             # =====================================================================",
            target_codename
        ));

        for line in content.lines() {
            let trimmed = line.trim();

            // Preservar líneas vacías para mantener la legibilidad
            if trimmed.is_empty() {
                updated_lines.push("".to_string());
                continue;
            }

            // Preservar comentarios del usuario (descartando cabeceras viejas de Scud si se re-ejecuta)
            if trimmed.starts_with('#') {
                if !trimmed.contains("gestionado automáticamente por Scud") 
                    && !trimmed.contains("Modificado automáticamente por Scud") {
                    if seen_lines.insert(trimmed.to_string()) {
                        updated_lines.push(line.to_string());
                    }
                }
                continue;
            }

            if trimmed.starts_with("deb ") || trimmed.starts_with("deb-src ") {
                // Si el destino es Sid, filtramos y descartamos por completo los repositorios 
                // de seguridad y de actualizaciones (-updates), ya que causan errores 404 y conflictos.
                if is_sid && (
                    trimmed.contains("security.debian.org") || 
                    trimmed.contains("debian-security") || 
                    trimmed.contains("-updates") || 
                    trimmed.contains("/updates")
                ) {
                    continue; 
                }

                // Analizar los componentes de la línea APT para respetar las preferencias del usuario
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 3 {
                    let mut new_parts = parts.clone();
                    
                    // parts[0] = deb / deb-src
                    // parts[1] = URL del repositorio
                    // parts[2] = Codename / Suite original (ej. trixie)
                    // parts[3..] = Componentes elegidos por el usuario (main, contrib, non-free, etc.) -> ¡Se conservan intactos!
                    
                    if is_sid && (parts[1].contains("deb.debian.org") || parts[1].contains("debian.org")) {
                        new_parts[2] = target_codename;
                    } else if !is_sid {
                        new_parts[2] = target_codename;
                    }

                    let reconstructed = new_parts.join(" ");
                    // Evitar duplicados exactos si el archivo original tenía múltiples referencias cruzadas
                    if seen_lines.insert(reconstructed.clone()) {
                        updated_lines.push(reconstructed);
                    }
                } else {
                    if seen_lines.insert(line.to_string()) {
                        updated_lines.push(line.to_string());
                    }
                }
            } else {
                // Repositorios de terceros o configuraciones especiales del usuario se mantienen tal cual
                if seen_lines.insert(line.to_string()) {
                    updated_lines.push(line.to_string());
                }
            }
        }

        let new_content = updated_lines.join("\n") + "\n";

        // 3. Escribir los cambios limpios y filtrados
        fs::write(path, new_content)
            .map_err(|e| format!("Error al escribir el nuevo sources.list: {}", e))?;

        println!("[Migrator] Migración a '{}' preparada con éxito respetando preferencias.", target_codename);
        Ok(())
    }
}