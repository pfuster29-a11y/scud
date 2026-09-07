/// Enhanced APT Output Parser
/// Parses apt-get output with safety level detection

use crate::risk_analyzer::SafetyLevel;

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
}

/// Parses the raw output of `apt-get dist-upgrade --just-print`.
pub fn parse_apt_output(raw_output: &str) -> Vec<PackageChange> {
    let mut changes = Vec::new();

    for line in raw_output.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("Inst ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 4 {
                let name = parts[1].to_string();
                let version_from = parts[2].trim_matches(|c| c == '[' || c == ']').to_string();
                let version_to = parts[3].trim_matches(|c| c == '(' || c == ')').to_string();
                
                changes.push(PackageChange {
                    name,
                    action: "Upgrade".to_string(),
                    risk: RiskLevel::Safe,
                    version_from,
                    version_to,
                    safety_level: SafetyLevel::Green,
                });
            }
        } else if trimmed.starts_with("Remv ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                changes.push(PackageChange {
                    name: parts[1].to_string(),
                    action: "Remove".to_string(),
                    risk: RiskLevel::Critical,
                    version_from: parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
                    version_to: String::new(),
                    safety_level: SafetyLevel::Red,
                });
            }
        } else if trimmed.starts_with("Conf ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                changes.push(PackageChange {
                    name: parts[1].to_string(),
                    action: "Configure".to_string(),
                    risk: RiskLevel::Safe,
                    version_from: String::new(),
                    version_to: parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
                    safety_level: SafetyLevel::Green,
                });
            }
        } else if trimmed.starts_with("Purg ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                changes.push(PackageChange {
                    name: parts[1].to_string(),
                    action: "Purge".to_string(),
                    risk: RiskLevel::Critical,
                    version_from: parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
                    version_to: String::new(),
                    safety_level: SafetyLevel::Red,
                });
            }
        }
    }

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
        let output = "Inst curl [7.68.0-1] (7.72.0-1 Debian:sid)";
        let changes = parse_apt_output(output);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].name, "curl");
        assert_eq!(changes[0].action, "Upgrade");
    }

    #[test]
    fn test_parse_remv_line() {
        let output = "Remv old-package [1.0-1]";
        let changes = parse_apt_output(output);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].action, "Remove");
        assert_eq!(changes[0].safety_level, SafetyLevel::Red);
    }
}
