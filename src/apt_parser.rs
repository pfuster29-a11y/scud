/// Enhanced APT Output Parser
/// Parses apt-get output with safety level detection

use crate::risk_analyzer::{PackageRiskAnalyzer, SafetyLevel};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskLevel {
    Safe,     // Package upgrade without removals
    Critical, // Package upgrade involving removals
}

#[derive(Debug, Clone)]
pub struct PackageChange {
    pub name: String,
    pub action: String,
    pub risk: RiskLevel,
    pub version_from: String,
    pub version_to: String,
    pub safety_level: SafetyLevel,
    /// Explicación en criollo de por qué se clasificó así. `None` para los
    /// paquetes Verdes (no hace falta justificar por qué algo es seguro).
    pub risk_reason: Option<String>,
}

/// Parses the raw output of `apt-get dist-upgrade --just-print`, usando el
/// `PackageRiskAnalyzer` para decidir el nivel de seguridad real de cada
/// paquete que se instala o configura (en vez de asumir que todo lo que no
/// es una remoción es automáticamente "seguro"), y guardando el motivo de
/// cada clasificación para poder mostrárselo al usuario.
///
/// El resultado queda ordenado de más peligroso a menos peligroso (Rojo
/// primero, después Amarillo, después Verde), para que lo que necesita
/// atención aparezca arriba de todo en vez de perderse al final de la lista.
///
/// Nota: `dependent_count` se pasa siempre en 0 por ahora, porque el output
/// de `--just-print` no nos da esa información directamente. Es un dato que
/// se podría calcular a futuro consultando `apt-cache rdepends` por paquete,
/// pero eso implicaría una consulta extra por cada paquete de la lista.
pub fn parse_apt_output(raw_output: &str, analyzer: &PackageRiskAnalyzer) -> Vec<PackageChange> {
    let mut changes = Vec::new();

    for line in raw_output.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("Inst ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 4 {
                let name = parts[1].to_string();
                let version_from = parts[2].trim_matches(|c| c == '[' || c == ']').to_string();
                let version_to = parts[3].trim_matches(|c| c == '(' || c == ')').to_string();

                let (safety_level, risk_reason) =
                    analyzer.analyze_package_detailed(&name, &version_from, &version_to, 0);
                let risk = if safety_level == SafetyLevel::Green { RiskLevel::Safe } else { RiskLevel::Critical };

                changes.push(PackageChange {
                    name,
                    action: "Upgrade".to_string(),
                    risk,
                    version_from,
                    version_to,
                    safety_level,
                    risk_reason,
                });
            }
        } else if trimmed.starts_with("Remv ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                // Una remoción durante un upgrade es, en sí misma, la señal de riesgo
                // más fuerte que hay: siempre Rojo, sin importar qué paquete sea.
                changes.push(PackageChange {
                    name: parts[1].to_string(),
                    action: "Remove".to_string(),
                    risk: RiskLevel::Critical,
                    version_from: parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
                    version_to: String::new(),
                    safety_level: SafetyLevel::Red,
                    risk_reason: Some("Esta actualización requiere remover el paquete del sistema".to_string()),
                });
            }
        } else if trimmed.starts_with("Conf ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                let name = parts[1].to_string();
                let version_to = parts.get(2).map(|s| s.to_string()).unwrap_or_default();
                let (safety_level, risk_reason) =
                    analyzer.analyze_package_detailed(&name, "", &version_to, 0);
                let risk = if safety_level == SafetyLevel::Green { RiskLevel::Safe } else { RiskLevel::Critical };

                changes.push(PackageChange {
                    name,
                    action: "Configure".to_string(),
                    risk,
                    version_from: String::new(),
                    version_to,
                    safety_level,
                    risk_reason,
                });
            }
        } else if trimmed.starts_with("Purg ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                // Igual que Remv: un purgado siempre es Rojo.
                changes.push(PackageChange {
                    name: parts[1].to_string(),
                    action: "Purge".to_string(),
                    risk: RiskLevel::Critical,
                    version_from: parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
                    version_to: String::new(),
                    safety_level: SafetyLevel::Red,
                    risk_reason: Some("Este paquete será purgado (borrado junto con su configuración)".to_string()),
                });
            }
        }
    }

    // Los más peligrosos van primero, para que salten a la vista de inmediato
    // en vez de quedar escondidos al final de una lista larga.
    changes.sort_by(|a, b| b.safety_level.cmp(&a.safety_level));

    changes
}

/// Count packages by safety level
pub fn count_by_safety(changes: &[PackageChange]) -> (usize, usize, usize) {
    let green = changes.iter().filter(|c| c.safety_level == SafetyLevel::Green).count();
    let yellow = changes.iter().filter(|c| c.safety_level == SafetyLevel::Yellow).count();
    let red = changes.iter().filter(|c| c.safety_level == SafetyLevel::Red).count();
    
    (green, yellow, red)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_inst_line() {
        let analyzer = PackageRiskAnalyzer::new();
        let output = "Inst curl [7.68.0-1] (7.72.0-1 Debian:sid)";
        let changes = parse_apt_output(output, &analyzer);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].name, "curl");
        assert_eq!(changes[0].action, "Upgrade");
    }

    #[test]
    fn test_parse_remv_line() {
        let analyzer = PackageRiskAnalyzer::new();
        let output = "Remv old-package [1.0-1]";
        let changes = parse_apt_output(output, &analyzer);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].action, "Remove");
        assert_eq!(changes[0].safety_level, SafetyLevel::Red);
    }
}
