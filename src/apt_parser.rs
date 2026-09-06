#[derive(Debug, PartialEq, Eq)]
pub enum RiskLevel {
    Safe,     // Package upgrade without removals
    Critical, // Package upgrade involving removals
}

#[derive(Debug)]
pub struct PackageChange {
    pub name: String,
    pub action: String,
    pub risk: RiskLevel,
}

/// Parses the raw output of `apt-get dist-upgrade --just-print`.
pub fn parse_apt_output(raw_output: &str) -> Vec<PackageChange> {
    let mut changes = Vec::new();

    for line in raw_output.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("Inst ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                changes.push(PackageChange {
                    name: parts[1].to_string(),
                    action: "Upgrade".to_string(),
                    risk: RiskLevel::Safe,
                });
            }
        } else if trimmed.starts_with("Remv ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                changes.push(PackageChange {
                    name: parts[1].to_string(),
                    action: "Remove".to_string(),
                    risk: RiskLevel::Critical,
                });
            }
        }
    }

    changes
}