//! Role-Based Access Control for Blue/Red/Purple teams.

use crate::models::user::TeamRole;

/// Team access levels
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamAccess {
    None,
    Read,
    Write,
    Admin,
}

/// Module categories by team
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleTeam {
    Blue,
    Red,
    Purple,
    Shared,
}

/// Get the team associated with a module
pub fn module_team(module: &str) -> ModuleTeam {
    match module {
        "osint" | "socmed" | "shared" => ModuleTeam::Shared,
        "siem" | "threat_intel" | "incident_response" | "vulnerability_scanning"
        | "cloud_container" | "supply_chain" | "api_security" | "compliance"
        | "asset_inventory" | "detection_engineering" => ModuleTeam::Blue,
        "pentesting" | "red_team_infra" | "social_engineering" | "wireless"
        | "mobile_security" | "hardware_firmware" | "exploit_development" => ModuleTeam::Red,
        "attack_simulation" | "adversary_emulation" | "coverage_mapping" | "joint_exercises" => {
            ModuleTeam::Purple
        }
        _ => ModuleTeam::Shared,
    }
}

/// Check if a user role can access a module
pub fn can_access_module(role: &TeamRole, module: &str) -> TeamAccess {
    let module_team = module_team(module);

    match (role, module_team) {
        // CISO and Auditor can read everything
        (TeamRole::Ciso, _) => TeamAccess::Read,
        (TeamRole::Auditor, _) => TeamAccess::Read,

        // Shared modules - read for all, write for leads/ciso
        (_, ModuleTeam::Shared) => match role {
            TeamRole::BlueLead | TeamRole::RedLead | TeamRole::PurpleEngineer | TeamRole::Ciso => {
                TeamAccess::Write
            }
            _ => TeamAccess::Read,
        },

        // Blue modules
        (_, ModuleTeam::Blue) => match role {
            TeamRole::BlueAnalyst | TeamRole::BlueLead | TeamRole::Ciso => TeamAccess::Write,
            TeamRole::PurpleEngineer => TeamAccess::Read,
            _ => TeamAccess::None,
        },

        // Red modules
        (_, ModuleTeam::Red) => match role {
            TeamRole::RedOperator | TeamRole::RedLead | TeamRole::Ciso => TeamAccess::Write,
            TeamRole::PurpleEngineer => TeamAccess::Read,
            _ => TeamAccess::None,
        },

        // Purple modules
        (_, ModuleTeam::Purple) => match role {
            TeamRole::PurpleEngineer | TeamRole::Ciso => TeamAccess::Write,
            TeamRole::BlueLead | TeamRole::RedLead => TeamAccess::Read,
            _ => TeamAccess::None,
        },
    }
}

/// Check if a team can perform a team-specific action
pub fn can_access_team(role: &TeamRole, team: &str) -> bool {
    match (role, team) {
        (TeamRole::Ciso, _) => true,
        (TeamRole::Auditor, _) => true,
        (TeamRole::BlueAnalyst | TeamRole::BlueLead, "blue") => true,
        (TeamRole::RedOperator | TeamRole::RedLead, "red") => true,
        (TeamRole::PurpleEngineer, "purple") => true,
        (TeamRole::PurpleEngineer, "blue") => true,
        (TeamRole::PurpleEngineer, "red") => true,
        _ => false,
    }
}