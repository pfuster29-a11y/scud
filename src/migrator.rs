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

    /// Prepara la migración preservando las preferencias del usuario.
    /// Comenta automáticamente los repositorios incompatibles con Sid (security y updates) 
    /// para evitar errores 404 sin perder la referencia original.
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
        let mut seen_signatures = HashSet::new();

        // Encabezado claro de Scud
        updated_lines.push(format!(
            "# =====================================================================\n\
             # Archivo sources.list modificado y gestionado automáticamente por Scud\n\
             # Rama de destino: Debian {} (Unstable)\n\
             # =====================================================================",
            target_codename
        ));

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.is_empty() {
                updated_lines.push("".to_string());
                continue;
            }

            // Preservar comentarios del usuario (descartando encabezados viejos de Scud)
            if trimmed.starts_with('#') {
                if !trimmed.contains("gestionado automáticamente por Scud") 
                    && !trimmed.contains("Modificado automáticamente por Scud") 
                    && !trimmed.contains("Rama de destino") {
                    updated_lines.push(line.to_string());
                }
                continue;
            }

            if trimmed.starts_with("deb ") || trimmed.starts_with("deb-src ") {
                // Si el destino es Sid, las líneas de seguridad y actualizaciones no existen por separado.
                // En lugar de borrarlas, las comentamos para mantener la config del usuario intacta pero inactiva.
                let is_incompatible_for_sid = is_sid && (
                    trimmed.contains("security.debian.org") || 
                    trimmed.contains("debian-security") || 
                    trimmed.contains("-updates") || 
                    trimmed.contains("/updates")
                );

                if is_incompatible_for_sid {
                    updated_lines.push(format!(
                        "# [Scud] Desactivado para Sid (no requerido en Unstable):\n# {}", 
                        trimmed
                    ));
                    continue;
                }

                // Análisis por tokens para actualizar el codename manteniendo componentes del usuario
                let tokens: Vec<&str> = trimmed.split_whitespace().collect();
                if tokens.len() >= 3 {
                    let mut url_idx = None;
                    for (i, token) in tokens.iter().enumerate() {
                        if token.contains("://") {
                            url_idx = Some(i);
                            break;
                        }
                    }

                    if let Some(u_idx) = url_idx {
                        let codename_idx = u_idx + 1;
                        if codename_idx < tokens.len() {
                            let mut new_tokens = tokens.clone();
                            // Reemplazar únicamente el codename por el de destino (ej. sid)
                            new_tokens[codename_idx] = target_codename;

                            let url = tokens[u_idx];
                            let components = &new_tokens[codename_idx + 1..];
                            let signature = format!("{}|{}", url, components.join(" "));

                            if seen_signatures.insert(signature) {
                                updated_lines.push(new_tokens.join(" "));
                            }
                        } else {
                            updated_lines.push(line.to_string());
                        }
                    } else {
                        updated_lines.push(line.to_string());
                    }
                } else {
                    updated_lines.push(line.to_string());
                }
            } else {
                // Repositorios de terceros o líneas especiales se mantienen sin cambios
                if seen_signatures.insert(trimmed.to_string()) {
                    updated_lines.push(line.to_string());
                }
            }
        }

        let new_content = updated_lines.join("\n") + "\n";

        // 3. Escribir el nuevo archivo
        fs::write(path, new_content)
            .map_err(|e| format!("Error al escribir el nuevo sources.list: {}", e))?;

        println!("[Migrator] Migración a '{}' preparada con éxito (repositorios incompatibles comentados).", target_codename);
        Ok(())
    }
}