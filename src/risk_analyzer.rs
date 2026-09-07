/// Risk Analysis Module for Debian Sid Packages
/// Determines safety levels for package updates

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SafetyLevel {
    Green = 0,   // Very safe
    Yellow = 1,  // Moderately risky
    Red = 2,     // Very dangerous
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageRiskAnalyzer {
    known_problematic: Vec<String>,
    critical_packages: Vec<String>,
    update_history: HashMap<String, UpdateRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRecord {
    pub package: String,
    pub version_from: String,
    pub version_to: String,
    pub success: bool,
    pub timestamp: i64,
}

impl Default for PackageRiskAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl PackageRiskAnalyzer {
    pub fn new() -> Self {
        Self {
            known_problematic: vec![
                "nvidia-driver".to_string(),
                "wine".to_string(),
                "qemu".to_string(),
                "mesa".to_string(),
                "virtualbox".to_string(),
            ],
            critical_packages: vec![
                "linux-image-amd64".to_string(),
                "grub-pc".to_string(),
                "systemd".to_string(),
                "apt".to_string(),
                "dpkg".to_string(),
                "glibc".to_string(),
                "gcc-base".to_string(),
                "gcc-14-base".to_string(),
                "gcc-13-base".to_string(),
                "perl".to_string(),
                "x11-common".to_string(),
                "xorg".to_string(),
                "openssl".to_string(),
                "libssl3".to_string(),
                "libgcc-s1".to_string(),
            ],
            update_history: HashMap::new(),
        }
    }

    /// Analyze a single package to determine its safety level
    pub fn analyze_package(
        &self,
        name: &str,
        version_from: &str,
        version_to: &str,
        dependent_count: usize,
    ) -> SafetyLevel {
        // 1. Known problematic packages → RED
        if self
            .known_problematic
            .iter()
            .any(|p| name.contains(p))
        {
            return SafetyLevel::Red;
        }

        // 2. Critical system packages → YELLOW
        if self.critical_packages.iter().any(|p| name == p) {
            return SafetyLevel::Yellow;
        }

        // 3. Many dependents → YELLOW
        if dependent_count > 10 {
            return SafetyLevel::Yellow;
        }

        // 4. Major version jump (1.x → 2.x) → YELLOW
        if Self::is_major_version_jump(version_from, version_to) {
            return SafetyLevel::Yellow;
        }

        // 5. Historical failures → RED
        if let Some(record) = self.update_history.get(name) {
            if !record.success {
                return SafetyLevel::Red;
            }
        }

        // Default: GREEN
        SafetyLevel::Green
    }

    fn is_major_version_jump(from: &str, to: &str) -> bool {
        let from_major = from.split('.').next().unwrap_or("0");
        let to_major = to.split('.').next().unwrap_or("0");
        from_major != to_major
    }

    pub fn load_history(&mut self) -> Result<(), String> {
        let history_path = dirs::data_dir()
            .ok_or("No se pudo encontrar directorio de datos")?
            .join("scud/history.json");

        if history_path.exists() {
            let content = std::fs::read_to_string(&history_path)
                .map_err(|e| format!("Error leyendo historial: {}", e))?;

            self.update_history = serde_json::from_str(&content)
                .map_err(|e| format!("Error parseando JSON: {}", e))?;
        }
        Ok(())
    }

    pub fn save_history(&self) -> Result<(), String> {
        let data_dir = dirs::data_dir()
            .ok_or("No se pudo encontrar directorio de datos")?
            .join("scud");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Error creando directorio: {}", e))?;

        let history_path = data_dir.join("history.json");
        let json = serde_json::to_string_pretty(&self.update_history)
            .map_err(|e| format!("Error serializando: {}", e))?;

        std::fs::write(&history_path, json)
            .map_err(|e| format!("Error escribiendo historial: {}", e))
    }

    pub fn record_update(&mut self, record: UpdateRecord) {
        self.update_history.insert(record.package.clone(), record);
    }

    pub fn get_breaking_change_risks(package_name: &str) -> Vec<String> {
        let mut risks = Vec::new();

        // Changes in fundamental libraries
        if ["glibc", "gcc", "openssl", "curl", "libssl"]
            .iter()
            .any(|&p| package_name.contains(p))
        {
            risks.push(
                "⚠️  Cambio en librería fundamental - puede requerir recompilación de dependientes"
                    .to_string(),
            );
        }

        // Python version changes
        if package_name.contains("python") && !package_name.contains("python3.11") {
            risks.push("⚠️  Actualización de Python - verificá compatibilidad de paquetes".to_string());
        }

        // X11/Wayland changes
        if ["xorg", "wayland", "mesa", "xwayland"]
            .iter()
            .any(|&p| package_name.contains(p))
        {
            risks.push("🚨 Cambio en servidor gráfico - puede dejar sin display".to_string());
        }

        // Bootloader changes
        if ["grub", "shim", "efibootmgr"]
            .iter()
            .any(|&p| package_name.contains(p))
        {
            risks.push("🚨 Cambio en bootloader - riesgo de no poder iniciar".to_string());
        }

        // systemd changes
        if package_name == "systemd" {
            risks.push("🚨 Cambio en systemd - puede afectar inicio del sistema".to_string());
        }

        risks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_critical_package_detection() {
        let analyzer = PackageRiskAnalyzer::new();
        let safety = analyzer.analyze_package("systemd", "250", "251", 5);
        assert_eq!(safety, SafetyLevel::Yellow);
    }

    #[test]
    fn test_safe_package_detection() {
        let analyzer = PackageRiskAnalyzer::new();
        let safety = analyzer.analyze_package("curl", "7.88.0", "7.88.1", 2);
        assert_eq!(safety, SafetyLevel::Green);
    }

    #[test]
    fn test_major_version_jump() {
        let analyzer = PackageRiskAnalyzer::new();
        let safety = analyzer.analyze_package("postgresql", "14.0", "15.0", 3);
        assert_eq!(safety, SafetyLevel::Yellow);
    }

    #[test]
    fn test_known_problematic() {
        let analyzer = PackageRiskAnalyzer::new();
        let safety = analyzer.analyze_package("nvidia-driver", "520", "525", 5);
        assert_eq!(safety, SafetyLevel::Red);
    }
}
