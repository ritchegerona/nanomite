//! Advanced Attack Path Analysis: BloodHound-style graph, choke points, crown jewel paths.
//!
//! This module builds and analyzes attack graphs from collected data (AD, cloud, network).
//! All analysis is passive - it consumes findings from other modules.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use super::ModuleReport;
use crate::hacker::findings::{Confidence, Finding};
use crate::hacker::risk::RiskLevel;

/// Max nodes/edges in attack graph.
pub const MAX_GRAPH_NODES: usize = 5000;
pub const MAX_GRAPH_EDGES: usize = 20000;
/// Max path length for path finding.
pub const MAX_PATH_LENGTH: usize = 10;

/// Node in the attack graph.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub node_type: NodeType,
    pub properties: HashMap<String, String>,
    pub risk_score: f32, // 0.0 - 10.0
    pub tier: u8,        // 0 = crown jewel, 1 = high value, 2 = pivot, 3 = entry
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NodeType {
    User,
    Computer,
    Group,
    Domain,
    GPO,
    OU,
    Service,
    Database,
    ApiEndpoint,
    CloudResource,
    Container,
    KubernetesResource,
    Secret,
    Certificate,
    Vulnerability,
    Credential,
    Session,
    NetworkSegment,
    TrustRelationship,
}

/// Edge in the attack graph.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub edge_type: EdgeType,
    pub technique: String, // MITRE technique ID
    pub weight: f32,       // 0.0 - 1.0 (difficulty/risk)
    pub properties: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EdgeType {
    MemberOf,
    HasSession,
    AdminTo,
    CanRDP,
    CanPSRemote,
    CanWMI,
    CanDCOM,
    HasSIDHistory,
    TrustedBy,
    Delegation, // Unconstrained/Constrained/RBCD
    SPNTarget,
    ACLControl, // GenericAll, WriteDacl, WriteOwner, etc.
    GPOApplied,
    Contains,
    Owns,
    CanRead,
    CanWrite,
    CanExecute,
    NetworkAccess,
    CloudPermission,
    K8sPermission,
    CertificateAccess,
    VulnerableTo,
    CredentialAccess,
    LateralMove,
    Persistence,
    PrivilegeEscalation,
}

/// Attack graph container.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AttackGraph {
    pub nodes: HashMap<String, GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub metadata: GraphMetadata,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GraphMetadata {
    pub generated_at: String,
    pub source_assessments: Vec<String>,
    pub node_count: usize,
    pub edge_count: usize,
    pub crown_jewels: Vec<String>, // node IDs marked as tier 0
    pub entry_points: Vec<String>, // node IDs marked as tier 3
}

/// Builds an attack graph from various data sources.
pub fn build_graph(
    ad_data: Option<&AdData>,
    cloud_data: Option<&CloudData>,
    network_data: Option<&NetworkData>,
    vuln_data: Option<&VulnData>,
) -> Result<AttackGraph, String> {
    let mut graph = AttackGraph::default();

    // Add AD nodes/edges
    if let Some(ad) = ad_data {
        add_ad_to_graph(&mut graph, ad)?;
    }

    // Add cloud nodes/edges
    if let Some(cloud) = cloud_data {
        add_cloud_to_graph(&mut graph, cloud)?;
    }

    // Add network nodes/edges
    if let Some(net) = network_data {
        add_network_to_graph(&mut graph, net)?;
    }

    // Add vuln nodes/edges
    if let Some(vuln) = vuln_data {
        add_vuln_to_graph(&mut graph, vuln)?;
    }

    // Compute risk scores and tiers
    compute_risk_scores(&mut graph);
    assign_tiers(&mut graph);

    graph.metadata.node_count = graph.nodes.len();
    graph.metadata.edge_count = graph.edges.len();
    graph.metadata.generated_at = chrono::Utc::now().to_rfc3339();

    Ok(graph)
}

/// AD data structure for graph building.
#[derive(Debug, Clone, Default)]
pub struct AdData {
    pub users: Vec<AdUser>,
    pub computers: Vec<AdComputer>,
    pub groups: Vec<AdGroup>,
    pub gpos: Vec<AdGpo>,
    pub ous: Vec<AdOu>,
    pub trusts: Vec<AdTrust>,
    pub delegation: Vec<AdDelegation>,
    pub aces: Vec<AdAce>,
    pub sessions: Vec<AdSession>,
    pub spns: Vec<AdSpn>,
}

#[derive(Debug, Clone)]
pub struct AdUser {
    pub sam_account_name: String,
    pub distinguished_name: String,
    pub sid: String,
    pub admin_count: bool,
    pub service_account: bool,
    pub dont_req_preauth: bool,
    pub password_last_set: Option<String>,
    pub last_logon: Option<String>,
    pub description: String,
    pub member_of: Vec<String>, // group DNs
}

#[derive(Debug, Clone)]
pub struct AdComputer {
    pub name: String,
    pub dns_host_name: String,
    pub distinguished_name: String,
    pub sid: String,
    pub operating_system: String,
    pub unconstrained_delegation: bool,
    pub constrained_delegation: Vec<String>, // SPNs
    pub rbcd_allowed_to_act: Vec<String>,    // SIDs
    pub admin_count: bool,
    pub member_of: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AdGroup {
    pub name: String,
    pub distinguished_name: String,
    pub sid: String,
    pub group_type: String,   // Security, Distribution
    pub members: Vec<String>, // DNs of users/computers/groups
    pub admin_count: bool,
}

#[derive(Debug, Clone)]
pub struct AdGpo {
    pub name: String,
    pub guid: String,
    pub linked_ous: Vec<String>, // OU DNs
    pub settings: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct AdOu {
    pub name: String,
    pub distinguished_name: String,
    pub linked_gpos: Vec<String>, // GPO GUIDs
    pub blocks_inheritance: bool,
}

#[derive(Debug, Clone)]
pub struct AdTrust {
    pub source_domain: String,
    pub target_domain: String,
    pub trust_type: String, // ParentChild, TreeRoot, External, Forest
    pub trust_attributes: String,
    pub direction: String, // Inbound, Outbound, Bidirectional
}

#[derive(Debug, Clone)]
pub struct AdDelegation {
    pub account_dn: String,
    pub delegation_type: String, // Unconstrained, Constrained, RBCD
    pub target_spns: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AdAce {
    pub object_dn: String,
    pub trustee_sid: String,
    pub ace_type: String,    // Allow, Deny
    pub rights: Vec<String>, // GenericAll, WriteDacl, WriteOwner, etc.
    pub inheritance: String,
}

#[derive(Debug, Clone)]
pub struct AdSession {
    pub user_dn: String,
    pub computer_dn: String,
    pub logon_type: String, // Interactive, Network, Service, Batch
    pub logon_time: String,
}

#[derive(Debug, Clone)]
pub struct AdSpn {
    pub service_account_dn: String,
    pub spn: String,
}

/// Cloud data for graph building.
#[derive(Debug, Clone, Default)]
pub struct CloudData {
    pub aws_accounts: Vec<AwsAccount>,
    pub azure_subscriptions: Vec<AzureSubscription>,
    pub gcp_projects: Vec<GcpProject>,
    pub iam_policies: Vec<IamPolicy>,
    pub resources: Vec<CloudResource>,
}

#[derive(Debug, Clone)]
pub struct AwsAccount {
    pub account_id: String,
    pub name: String,
    pub roles: Vec<AwsRole>,
    pub users: Vec<AwsUser>,
}

#[derive(Debug, Clone)]
pub struct AwsRole {
    pub arn: String,
    pub name: String,
    pub assume_role_policy: String,
    pub attached_policies: Vec<String>,
    pub inline_policies: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct AwsUser {
    pub arn: String,
    pub name: String,
    pub groups: Vec<String>,
    pub attached_policies: Vec<String>,
    pub access_keys: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AzureSubscription {
    pub subscription_id: String,
    pub name: String,
    pub role_assignments: Vec<AzureRoleAssignment>,
}

#[derive(Debug, Clone)]
pub struct AzureRoleAssignment {
    pub principal_id: String,
    pub principal_type: String, // User, Group, ServicePrincipal
    pub role_definition_id: String,
    pub scope: String,
}

#[derive(Debug, Clone)]
pub struct GcpProject {
    pub project_id: String,
    pub name: String,
    pub iam_bindings: Vec<GcpIamBinding>,
}

#[derive(Debug, Clone)]
pub struct GcpIamBinding {
    pub role: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IamPolicy {
    pub policy_arn: String,
    pub document: String,
}

#[derive(Debug, Clone)]
pub struct CloudResource {
    pub id: String,
    pub type_: String, // EC2, S3, RDS, Lambda, etc.
    pub account: String,
    pub region: String,
    pub tags: HashMap<String, String>,
    pub network_interfaces: Vec<String>,
}

/// Network data for graph building.
#[derive(Debug, Clone, Default)]
pub struct NetworkData {
    pub segments: Vec<NetworkSegment>,
    pub hosts: Vec<NetworkHost>,
    pub connections: Vec<NetworkConnection>,
    pub firewalls: Vec<FirewallRule>,
}

#[derive(Debug, Clone)]
pub struct NetworkSegment {
    pub cidr: String,
    pub name: String,
    pub vlan: Option<u16>,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct NetworkHost {
    pub ip: String,
    pub hostname: Option<String>,
    pub mac: Option<String>,
    pub os: Option<String>,
    pub open_ports: Vec<u16>,
    pub segment: String, // CIDR
}

#[derive(Debug, Clone)]
pub struct NetworkConnection {
    pub src_ip: String,
    pub dst_ip: String,
    pub port: u16,
    pub protocol: String,
    pub timestamp: String,
}

#[derive(Debug, Clone)]
pub struct FirewallRule {
    pub name: String,
    pub src_cidr: String,
    pub dst_cidr: String,
    pub ports: Vec<u16>,
    pub action: String, // Allow, Deny
    pub protocol: String,
}

/// Vulnerability data for graph building.
#[derive(Debug, Clone, Default)]
pub struct VulnData {
    pub findings: Vec<VulnFinding>,
}

#[derive(Debug, Clone)]
pub struct VulnFinding {
    pub asset_id: String,
    pub cve_id: String,
    pub title: String,
    pub severity: String, // Critical, High, Medium, Low
    pub cvss: Option<f32>,
    pub exploitable: bool,
    pub exploit_available: bool,
}

fn add_ad_to_graph(graph: &mut AttackGraph, ad: &AdData) -> Result<(), String> {
    // Users
    for user in &ad.users {
        let id = format!("user:{}", user.sid);
        let mut props = HashMap::new();
        props.insert("sam".into(), user.sam_account_name.clone());
        props.insert("dn".into(), user.distinguished_name.clone());
        props.insert("adminCount".into(), user.admin_count.to_string());
        props.insert("serviceAccount".into(), user.service_account.to_string());
        props.insert("dontReqPreauth".into(), user.dont_req_preauth.to_string());

        let node = GraphNode {
            id: id.clone(),
            label: user.sam_account_name.clone(),
            node_type: NodeType::User,
            properties: props,
            risk_score: if user.admin_count {
                8.0
            } else if user.service_account {
                6.0
            } else {
                3.0
            },
            tier: if user.admin_count { 1 } else { 3 },
        };
        graph.nodes.insert(id.clone(), node);

        // MemberOf edges
        for group_dn in &user.member_of {
            graph.edges.push(GraphEdge {
                from: id.clone(),
                to: format!("group:{}", group_dn),
                edge_type: EdgeType::MemberOf,
                technique: "T1069.002".into(),
                weight: 0.1,
                properties: HashMap::new(),
            });
        }

        // DontReqPreauth -> AS-REP Roasting
        if user.dont_req_preauth {
            graph.edges.push(GraphEdge {
                from: id.clone(),
                to: format!("vuln:asrep-{}", user.sid),
                edge_type: EdgeType::VulnerableTo,
                technique: "T1558.004".into(),
                weight: 0.3,
                properties: [("type".into(), "AS-REP Roasting".into())].into(),
            });
        }
    }

    // Computers
    for comp in &ad.computers {
        let id = format!("computer:{}", comp.sid);
        let mut props = HashMap::new();
        props.insert("name".into(), comp.name.clone());
        props.insert("dns".into(), comp.dns_host_name.clone());
        props.insert("dn".into(), comp.distinguished_name.clone());
        props.insert("os".into(), comp.operating_system.clone());
        props.insert(
            "unconstrained".into(),
            comp.unconstrained_delegation.to_string(),
        );

        let node = GraphNode {
            id: id.clone(),
            label: comp.name.clone(),
            node_type: NodeType::Computer,
            properties: props,
            risk_score: if comp.unconstrained_delegation {
                9.0
            } else if comp.admin_count {
                7.0
            } else {
                4.0
            },
            tier: if comp.unconstrained_delegation {
                0
            } else if comp.admin_count {
                1
            } else {
                3
            },
        };
        graph.nodes.insert(id.clone(), node);

        // MemberOf
        for group_dn in &comp.member_of {
            graph.edges.push(GraphEdge {
                from: id.clone(),
                to: format!("group:{}", group_dn),
                edge_type: EdgeType::MemberOf,
                technique: "T1069.002".into(),
                weight: 0.1,
                properties: HashMap::new(),
            });
        }

        // Unconstrained delegation
        if comp.unconstrained_delegation {
            graph.edges.push(GraphEdge {
                from: id.clone(),
                to: "technique:unconstrained-delegation".into(),
                edge_type: EdgeType::Delegation,
                technique: "T1558.003".into(),
                weight: 0.2,
                properties: [("type".into(), "Unconstrained".into())].into(),
            });
        }

        // Constrained delegation
        for spn in &comp.constrained_delegation {
            graph.edges.push(GraphEdge {
                from: id.clone(),
                to: format!("spn:{}", spn),
                edge_type: EdgeType::Delegation,
                technique: "T1558.003".into(),
                weight: 0.3,
                properties: [
                    ("type".into(), "Constrained".into()),
                    ("spn".into(), spn.clone()),
                ]
                .into(),
            });
        }

        // RBCD
        for sid in &comp.rbcd_allowed_to_act {
            graph.edges.push(GraphEdge {
                from: format!("computer:{}", sid),
                to: id.clone(),
                edge_type: EdgeType::Delegation,
                technique: "T1558.003".into(),
                weight: 0.4,
                properties: [("type".into(), "RBCD".into())].into(),
            });
        }
    }

    // Groups
    for group in &ad.groups {
        let id = format!("group:{}", group.distinguished_name);
        let mut props = HashMap::new();
        props.insert("name".into(), group.name.clone());
        props.insert("sid".into(), group.sid.clone());
        props.insert("type".into(), group.group_type.clone());
        props.insert("adminCount".into(), group.admin_count.to_string());

        let node = GraphNode {
            id: id.clone(),
            label: group.name.clone(),
            node_type: NodeType::Group,
            properties: props,
            risk_score: if group.admin_count { 8.0 } else { 4.0 },
            tier: if group.admin_count { 1 } else { 2 },
        };
        graph.nodes.insert(id.clone(), node);

        // Members
        for member_dn in &group.members {
            // Could be user, computer, or group
            let member_type =
                if member_dn.contains("CN=Users") || member_dn.contains("CN=Computers") {
                    "user"
                } else {
                    "group"
                };
            graph.edges.push(GraphEdge {
                from: format!("{}:{}", member_type, member_dn),
                to: id.clone(),
                edge_type: EdgeType::MemberOf,
                technique: "T1069.002".into(),
                weight: 0.1,
                properties: HashMap::new(),
            });
        }
    }

    // ACLs (ACEs)
    for ace in &ad.aces {
        if ace.ace_type == "Allow" {
            for right in &ace.rights {
                let edge_type = match right.as_str() {
                    "GenericAll" | "WriteDacl" | "WriteOwner" | "AllExtendedRights" => {
                        EdgeType::ACLControl
                    }
                    "ExtendedRight"
                        if ace
                            .rights
                            .iter()
                            .any(|r| r.contains("DS-Replication-Get-Changes")) =>
                    {
                        EdgeType::ACLControl
                    }
                    _ => continue,
                };
                graph.edges.push(GraphEdge {
                    from: format!("principal:{}", ace.trustee_sid),
                    to: format!("object:{}", ace.object_dn),
                    edge_type,
                    technique: "T1098".into(),
                    weight: match right.as_str() {
                        "GenericAll" => 0.2,
                        "WriteDacl" | "WriteOwner" => 0.3,
                        "AllExtendedRights" => 0.25,
                        _ => 0.5,
                    },
                    properties: [("right".into(), right.clone())].into(),
                });
            }
        }
    }

    // Sessions
    for sess in &ad.sessions {
        graph.edges.push(GraphEdge {
            from: format!("user:{}", sess.user_dn),
            to: format!("computer:{}", sess.computer_dn),
            edge_type: EdgeType::HasSession,
            technique: "T1550.002".into(),
            weight: match sess.logon_type.as_str() {
                "Interactive" => 0.1,
                "Network" => 0.3,
                _ => 0.5,
            },
            properties: [("logonType".into(), sess.logon_type.clone())].into(),
        });
    }

    // SPNs
    for spn in &ad.spns {
        let id = format!("spn:{}", spn.spn);
        let node = GraphNode {
            id: id.clone(),
            label: spn.spn.clone(),
            node_type: NodeType::Service,
            properties: [("serviceAccount".into(), spn.service_account_dn.clone())].into(),
            risk_score: 5.0,
            tier: 2,
        };
        graph.nodes.insert(id.clone(), node);

        graph.edges.push(GraphEdge {
            from: format!("user:{}", spn.service_account_dn),
            to: id.clone(),
            edge_type: EdgeType::SPNTarget,
            technique: "T1558.003".into(),
            weight: 0.3,
            properties: HashMap::new(),
        });
    }

    // Trusts
    for trust in &ad.trusts {
        graph.edges.push(GraphEdge {
            from: format!("domain:{}", trust.source_domain),
            to: format!("domain:{}", trust.target_domain),
            edge_type: EdgeType::TrustedBy,
            technique: "T1550.003".into(),
            weight: 0.4,
            properties: [
                ("type".into(), trust.trust_type.clone()),
                ("direction".into(), trust.direction.clone()),
            ]
            .into(),
        });
    }

    Ok(())
}

fn add_cloud_to_graph(graph: &mut AttackGraph, cloud: &CloudData) -> Result<(), String> {
    // AWS
    for acct in &cloud.aws_accounts {
        let acct_id = format!("aws:account:{}", acct.account_id);
        graph.nodes.insert(
            acct_id.clone(),
            GraphNode {
                id: acct_id.clone(),
                label: format!("AWS:{}", acct.name),
                node_type: NodeType::CloudResource,
                properties: [
                    ("type".into(), "account".into()),
                    ("id".into(), acct.account_id.clone()),
                ]
                .into(),
                risk_score: 7.0,
                tier: 1,
            },
        );

        for role in &acct.roles {
            let role_id = format!("aws:role:{}", role.arn);
            graph.nodes.insert(
                role_id.clone(),
                GraphNode {
                    id: role_id.clone(),
                    label: role.name.clone(),
                    node_type: NodeType::Service,
                    properties: [("arn".into(), role.arn.clone())].into(),
                    risk_score: 5.0,
                    tier: 2,
                },
            );

            // Trust policy edges (who can assume)
            // Simplified - would parse assume_role_policy
        }

        for user in &acct.users {
            let user_id = format!("aws:user:{}", user.arn);
            graph.nodes.insert(
                user_id.clone(),
                GraphNode {
                    id: user_id.clone(),
                    label: user.name.clone(),
                    node_type: NodeType::User,
                    properties: [("arn".into(), user.arn.clone())].into(),
                    risk_score: 4.0,
                    tier: 3,
                },
            );

            for group in &user.groups {
                graph.edges.push(GraphEdge {
                    from: user_id.clone(),
                    to: format!("aws:group:{}", group),
                    edge_type: EdgeType::MemberOf,
                    technique: "T1069.003".into(),
                    weight: 0.1,
                    properties: HashMap::new(),
                });
            }
        }
    }

    // Azure
    for sub in &cloud.azure_subscriptions {
        let sub_id = format!("azure:sub:{}", sub.subscription_id);
        graph.nodes.insert(
            sub_id.clone(),
            GraphNode {
                id: sub_id.clone(),
                label: format!("Azure:{}", sub.name),
                node_type: NodeType::CloudResource,
                properties: [("type".into(), "subscription".into())].into(),
                risk_score: 7.0,
                tier: 1,
            },
        );

        for ra in &sub.role_assignments {
            let principal_id = format!("azure:principal:{}", ra.principal_id);
            let scope_id = format!("azure:scope:{}", ra.scope);
            graph.edges.push(GraphEdge {
                from: principal_id,
                to: scope_id,
                edge_type: EdgeType::CloudPermission,
                technique: "T1098.001".into(),
                weight: 0.3,
                properties: [("role".into(), ra.role_definition_id.clone())].into(),
            });
        }
    }

    // GCP
    for proj in &cloud.gcp_projects {
        let proj_id = format!("gcp:project:{}", proj.project_id);
        graph.nodes.insert(
            proj_id.clone(),
            GraphNode {
                id: proj_id.clone(),
                label: format!("GCP:{}", proj.name),
                node_type: NodeType::CloudResource,
                properties: [("type".into(), "project".into())].into(),
                risk_score: 7.0,
                tier: 1,
            },
        );

        for binding in &proj.iam_bindings {
            for member in &binding.members {
                let member_id = format!("gcp:member:{}", member);
                graph.edges.push(GraphEdge {
                    from: member_id,
                    to: proj_id.clone(),
                    edge_type: EdgeType::CloudPermission,
                    technique: "T1098.001".into(),
                    weight: 0.3,
                    properties: [("role".into(), binding.role.clone())].into(),
                });
            }
        }
    }

    Ok(())
}

fn add_network_to_graph(graph: &mut AttackGraph, net: &NetworkData) -> Result<(), String> {
    // Segments
    for seg in &net.segments {
        let id = format!("segment:{}", seg.cidr.replace('/', "_"));
        graph.nodes.insert(
            id.clone(),
            GraphNode {
                id: id.clone(),
                label: seg.name.clone(),
                node_type: NodeType::NetworkSegment,
                properties: [("cidr".into(), seg.cidr.clone())].into(),
                risk_score: 3.0,
                tier: 3,
            },
        );
    }

    // Hosts
    for host in &net.hosts {
        let id = format!("host:{}", host.ip);
        let mut props = HashMap::new();
        props.insert("ip".into(), host.ip.clone());
        if let Some(h) = &host.hostname {
            props.insert("hostname".into(), h.clone());
        }
        if let Some(m) = &host.mac {
            props.insert("mac".into(), m.clone());
        }
        if let Some(o) = &host.os {
            props.insert("os".into(), o.clone());
        }
        props.insert(
            "ports".into(),
            host.open_ports
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(","),
        );
        props.insert("segment".into(), host.segment.clone());

        let risk = if host.open_ports.contains(&445) {
            6.0
        } else if host.open_ports.contains(&3389) {
            5.0
        } else {
            3.0
        };
        graph.nodes.insert(
            id.clone(),
            GraphNode {
                id: id.clone(),
                label: host.hostname.clone().unwrap_or_else(|| host.ip.clone()),
                node_type: NodeType::Computer,
                properties: props,
                risk_score: risk,
                tier: 3,
            },
        );

        // Segment membership
        let seg_id = format!("segment:{}", host.segment.replace('/', "_"));
        graph.edges.push(GraphEdge {
            from: id.clone(),
            to: seg_id,
            edge_type: EdgeType::NetworkAccess,
            technique: "T1590".into(),
            weight: 0.1,
            properties: HashMap::new(),
        });

        // Open ports -> potential lateral movement
        if host.open_ports.contains(&445) {
            graph.edges.push(GraphEdge {
                from: id.clone(),
                to: "technique:smb".into(),
                edge_type: EdgeType::LateralMove,
                technique: "T1021.002".into(),
                weight: 0.3,
                properties: HashMap::new(),
            });
        }
        if host.open_ports.contains(&3389) {
            graph.edges.push(GraphEdge {
                from: id.clone(),
                to: "technique:rdp".into(),
                edge_type: EdgeType::LateralMove,
                technique: "T1021.001".into(),
                weight: 0.3,
                properties: HashMap::new(),
            });
        }
        if host.open_ports.contains(&5985) || host.open_ports.contains(&5986) {
            graph.edges.push(GraphEdge {
                from: id.clone(),
                to: "technique:winrm".into(),
                edge_type: EdgeType::LateralMove,
                technique: "T1021.006".into(),
                weight: 0.3,
                properties: HashMap::new(),
            });
        }
        if host.open_ports.contains(&135) {
            graph.edges.push(GraphEdge {
                from: id.clone(),
                to: "technique:wmi".into(),
                edge_type: EdgeType::LateralMove,
                technique: "T1047".into(),
                weight: 0.4,
                properties: HashMap::new(),
            });
        }
    }

    // Firewall rules (simplified)
    for _fw in &net.firewalls {
        // Would create allow/deny edges between segments
    }

    Ok(())
}

fn add_vuln_to_graph(graph: &mut AttackGraph, vuln: &VulnData) -> Result<(), String> {
    for finding in &vuln.findings {
        let vuln_id = format!("vuln:{}:{}", finding.asset_id, finding.cve_id);
        let mut props = HashMap::new();
        props.insert("cve".into(), finding.cve_id.clone());
        props.insert("title".into(), finding.title.clone());
        props.insert("severity".into(), finding.severity.clone());
        props.insert(
            "cvss".into(),
            finding.cvss.map(|c| c.to_string()).unwrap_or_default(),
        );
        props.insert("exploitable".into(), finding.exploitable.to_string());
        props.insert(
            "exploitAvailable".into(),
            finding.exploit_available.to_string(),
        );

        let risk = match finding.severity.as_str() {
            "Critical" => 9.0,
            "High" => 7.0,
            "Medium" => 5.0,
            _ => 3.0,
        };

        graph.nodes.insert(
            vuln_id.clone(),
            GraphNode {
                id: vuln_id.clone(),
                label: format!("{}: {}", finding.cve_id, finding.title),
                node_type: NodeType::Vulnerability,
                properties: props,
                risk_score: risk,
                tier: 2,
            },
        );

        // Link to asset
        graph.edges.push(GraphEdge {
            from: format!("asset:{}", finding.asset_id),
            to: vuln_id.clone(),
            edge_type: EdgeType::VulnerableTo,
            technique: "T1190".into(),
            weight: if finding.exploitable && finding.exploit_available {
                0.2
            } else {
                0.5
            },
            properties: [
                ("exploitable".into(), finding.exploitable.to_string()),
                (
                    "exploitAvailable".into(),
                    finding.exploit_available.to_string(),
                ),
            ]
            .into(),
        });
    }
    Ok(())
}

fn compute_risk_scores(graph: &mut AttackGraph) {
    // Propagate risk scores through edges
    // Simple iterative propagation: risk flows backwards (from target to source)
    let mut changed = true;
    for _ in 0..10 {
        if !changed {
            break;
        }
        changed = false;
        for edge in &graph.edges {
            if let (Some(_from_node), Some(to_node)) =
                (graph.nodes.get(&edge.from), graph.nodes.get(&edge.to))
            {
                // Risk flows from target to source for attack paths
                let propagated = to_node.risk_score * (1.0 - edge.weight) * 0.8;
                if let Some(from_mut) = graph.nodes.get_mut(&edge.from) {
                    if propagated > from_mut.risk_score {
                        from_mut.risk_score = propagated;
                        changed = true;
                    }
                }
            }
        }
    }
}

fn assign_tiers(graph: &mut AttackGraph) {
    // Tier 0: Crown jewels (Domain Controllers, Tier 0 admins, root cloud accounts)
    // Tier 1: High value (Enterprise admins, DC computers, cloud admin roles)
    // Tier 2: Pivot points (Servers, service accounts, standard cloud roles)
    // Tier 3: Entry points (Workstations, regular users, public endpoints)

    for node in graph.nodes.values_mut() {
        node.tier = match node.node_type {
            NodeType::Domain => 0,
            NodeType::Group if node.properties.get("adminCount") == Some(&"true".to_string()) => 0,
            NodeType::User if node.properties.get("adminCount") == Some(&"true".to_string()) => 1,
            NodeType::Computer
                if node.properties.get("unconstrained") == Some(&"true".to_string()) =>
            {
                0
            }
            NodeType::Computer
                if node.properties.get("adminCount") == Some(&"true".to_string()) =>
            {
                1
            }
            NodeType::CloudResource
                if node.properties.get("type") == Some(&"account".to_string()) =>
            {
                1
            }
            NodeType::Service => 2,
            _ => 3,
        };
    }

    // Collect crown jewels and entry points
    graph.metadata.crown_jewels = graph
        .nodes
        .values()
        .filter(|n| n.tier == 0)
        .map(|n| n.id.clone())
        .collect();
    graph.metadata.entry_points = graph
        .nodes
        .values()
        .filter(|n| n.tier == 3)
        .map(|n| n.id.clone())
        .collect();
}

/// Finds shortest attack paths from entry points to crown jewels.
pub fn find_attack_paths(graph: &AttackGraph, max_paths: usize) -> Vec<AttackPath> {
    let mut paths = Vec::new();

    for entry_id in &graph.metadata.entry_points {
        for target_id in &graph.metadata.crown_jewels {
            if let Some(path) = bfs_shortest_path(graph, entry_id, target_id) {
                paths.push(path);
                if paths.len() >= max_paths {
                    return paths;
                }
            }
        }
    }

    // Sort by total weight (easiest paths first)
    paths.sort_by(|a, b| a.total_weight.partial_cmp(&b.total_weight).unwrap());
    paths.truncate(max_paths);
    paths
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AttackPath {
    pub nodes: Vec<String>,
    pub edges: Vec<GraphEdge>,
    pub total_weight: f32,
    pub techniques: Vec<String>,
    pub risk_score: f32,
}

fn bfs_shortest_path(graph: &AttackGraph, start: &str, goal: &str) -> Option<AttackPath> {
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    let mut parent: HashMap<String, (String, GraphEdge)> = HashMap::new();

    queue.push_back(start.to_string());
    visited.insert(start.to_string());

    while let Some(current) = queue.pop_front() {
        if current == goal {
            // Reconstruct path
            let mut path_nodes = Vec::new();
            let mut path_edges = Vec::new();
            let mut node = goal.to_string();

            while node != start {
                path_nodes.push(node.clone());
                if let Some((prev, edge)) = parent.get(&node) {
                    path_edges.push(edge.clone());
                    node = prev.clone();
                } else {
                    break;
                }
            }
            path_nodes.push(start.to_string());
            path_nodes.reverse();
            path_edges.reverse();

            let total_weight: f32 = path_edges.iter().map(|e| e.weight).sum();
            let techniques: Vec<String> = path_edges.iter().map(|e| e.technique.clone()).collect();
            let risk_score = path_nodes
                .iter()
                .filter_map(|n| graph.nodes.get(n))
                .map(|n| n.risk_score)
                .fold(0.0, f32::max);

            return Some(AttackPath {
                nodes: path_nodes,
                edges: path_edges,
                total_weight,
                techniques,
                risk_score,
            });
        }

        // Find neighbors
        for edge in &graph.edges {
            if edge.from == current && !visited.contains(&edge.to) {
                visited.insert(edge.to.clone());
                parent.insert(edge.to.clone(), (current.clone(), edge.clone()));
                queue.push_back(edge.to.clone());
            }
        }
    }

    None
}

/// Identifies choke points: nodes that appear in many attack paths.
pub fn find_choke_points(
    graph: &AttackGraph,
    paths: &[AttackPath],
    top_n: usize,
) -> Vec<ChokePoint> {
    let mut node_counts: HashMap<String, usize> = HashMap::new();

    for path in paths {
        // Entry and exit nodes are not choke points: a choke point is a node
        // every path has to traverse *through*, so only interior nodes count.
        if path.nodes.len() < 3 {
            continue;
        }
        for node_id in &path.nodes[1..path.nodes.len() - 1] {
            *node_counts.entry(node_id.clone()).or_default() += 1;
        }
    }

    let mut chokes: Vec<ChokePoint> = node_counts
        .into_iter()
        .filter(|(id, _)| graph.nodes.contains_key(id))
        .map(|(id, count)| {
            let node = &graph.nodes[&id];
            ChokePoint {
                node_id: id,
                label: node.label.clone(),
                node_type: node.node_type,
                path_count: count,
                risk_score: node.risk_score,
                tier: node.tier,
            }
        })
        .collect();

    chokes.sort_by(|a, b| {
        b.path_count
            .cmp(&a.path_count)
            .then(b.risk_score.partial_cmp(&a.risk_score).unwrap())
    });
    chokes.truncate(top_n);
    chokes
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChokePoint {
    pub node_id: String,
    pub label: String,
    pub node_type: NodeType,
    pub path_count: usize,
    pub risk_score: f32,
    pub tier: u8,
}

/// Exports graph to GraphML for visualization in yEd, Gephi, BloodHound.
pub fn export_graphml(graph: &AttackGraph, path: &Path) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;

    writeln!(file, r#"<?xml version="1.0" encoding="UTF-8"?>"#).map_err(|e| e.to_string())?;
    writeln!(
        file,
        r#"<graphml xmlns="http://graphml.graphdrawing.org/xmlns">"#
    )
    .map_err(|e| e.to_string())?;
    writeln!(
        file,
        r#"  <key id="label" for="node" attr.name="label" attr.type="string"/>"#
    )
    .map_err(|e| e.to_string())?;
    writeln!(
        file,
        r#"  <key id="type" for="node" attr.name="type" attr.type="string"/>"#
    )
    .map_err(|e| e.to_string())?;
    writeln!(
        file,
        r#"  <key id="risk" for="node" attr.name="risk" attr.type="double"/>"#
    )
    .map_err(|e| e.to_string())?;
    writeln!(
        file,
        r#"  <key id="tier" for="node" attr.name="tier" attr.type="int"/>"#
    )
    .map_err(|e| e.to_string())?;
    writeln!(
        file,
        r#"  <key id="technique" for="edge" attr.name="technique" attr.type="string"/>"#
    )
    .map_err(|e| e.to_string())?;
    writeln!(
        file,
        r#"  <key id="weight" for="edge" attr.name="weight" attr.type="double"/>"#
    )
    .map_err(|e| e.to_string())?;
    writeln!(
        file,
        r#"  <graph id="attack-graph" edgedefault="directed">"#
    )
    .map_err(|e| e.to_string())?;

    for node in graph.nodes.values() {
        writeln!(file, r#"    <node id="{}">"#, node.id).map_err(|e| e.to_string())?;
        writeln!(
            file,
            r#"      <data key="label">{}</data>"#,
            escape_xml(&node.label)
        )
        .map_err(|e| e.to_string())?;
        writeln!(
            file,
            r#"      <data key="type">{:?}</data>"#,
            node.node_type
        )
        .map_err(|e| e.to_string())?;
        writeln!(
            file,
            r#"      <data key="risk">{:.1}</data>"#,
            node.risk_score
        )
        .map_err(|e| e.to_string())?;
        writeln!(file, r#"      <data key="tier">{}</data>"#, node.tier)
            .map_err(|e| e.to_string())?;
        writeln!(file, r#"    </node>"#).map_err(|e| e.to_string())?;
    }

    for edge in &graph.edges {
        writeln!(
            file,
            r#"    <edge source="{}" target="{}">"#,
            edge.from, edge.to
        )
        .map_err(|e| e.to_string())?;
        writeln!(
            file,
            r#"      <data key="technique">{}</data>"#,
            edge.technique
        )
        .map_err(|e| e.to_string())?;
        writeln!(
            file,
            r#"      <data key="weight">{:.2}</data>"#,
            edge.weight
        )
        .map_err(|e| e.to_string())?;
        writeln!(file, r#"    </edge>"#).map_err(|e| e.to_string())?;
    }

    writeln!(file, r#"  </graph>"#).map_err(|e| e.to_string())?;
    writeln!(file, r#"</graphml>"#).map_err(|e| e.to_string())?;

    Ok(())
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Generates an attack path analysis report.
pub fn analyze_report(
    graph: &AttackGraph,
    paths: &[AttackPath],
    chokes: &[ChokePoint],
) -> ModuleReport {
    let mut report = ModuleReport::new(
        "attack_path",
        "analyze",
        "attack-graph",
        format!(
            "{} nodes, {} edges, {} paths, {} choke points",
            graph.metadata.node_count,
            graph.metadata.edge_count,
            paths.len(),
            chokes.len()
        ),
    );

    report = report.line(format!(
        "Crown Jewels (Tier 0): {}",
        graph.metadata.crown_jewels.len()
    ));
    for cj in &graph.metadata.crown_jewels {
        if let Some(node) = graph.nodes.get(cj) {
            report = report.line(format!(
                "  [{}] {} ({:?}) - risk {:.1}",
                node.tier, node.label, node.node_type, node.risk_score
            ));
        }
    }

    report = report.line(format!(
        "Entry Points (Tier 3): {}",
        graph.metadata.entry_points.len()
    ));

    report = report.line("");
    report = report.line("=== Attack Paths ===");
    for (i, path) in paths.iter().enumerate().take(10) {
        report = report.line(format!(
            "Path {}: weight={:.2} risk={:.1} techniques=[{}]",
            i + 1,
            path.total_weight,
            path.risk_score,
            path.techniques.join(", ")
        ));
        for (j, node_id) in path.nodes.iter().enumerate() {
            if let Some(node) = graph.nodes.get(node_id) {
                let prefix = if j == 0 {
                    "  START "
                } else if j == path.nodes.len() - 1 {
                    "  END "
                } else {
                    "  -> "
                };
                report = report.line(format!(
                    "{} [{}] {} ({:?})",
                    prefix, node.tier, node.label, node.node_type
                ));
            }
        }
    }

    report = report.line("");
    report = report.line("=== Top Choke Points ===");
    for (i, choke) in chokes.iter().enumerate().take(10) {
        report = report.line(format!(
            "{}. {} ({:?}) - in {} paths, risk {:.1}, tier {}",
            i + 1,
            choke.label,
            choke.node_type,
            choke.path_count,
            choke.risk_score,
            choke.tier
        ));
    }

    report = report.line("");
    report = report.line("=== Recommendations ===");
    report = report.line("1. Prioritize monitoring/detection on choke points");
    report = report.line("2. Break paths at lowest-weight edges (strongest controls)");
    report = report.line("3. Remove unnecessary delegations, admin rights, trust relationships");
    report = report.line("4. Segment network to eliminate lateral movement edges");
    report = report.line("5. Enable Credential Guard, LAPS, tiered administration");

    if !paths.is_empty() {
        report = report.finding(
            Finding::new(
                "attack_path",
                "analyze",
                "attack-graph",
                format!("found {} attack paths to crown jewels", paths.len()),
                RiskLevel::High,
            )
            .confidence(Confidence::Suspected)
            .detail(format!(
                "Shortest path weight: {:.2}. Top choke: {}",
                paths[0].total_weight,
                chokes.first().map(|c| c.label.as_str()).unwrap_or("none")
            )),
        );
    }

    report
}

/// Full attack path analysis from collected data.
pub fn full_analysis(
    ad_data: Option<&AdData>,
    cloud_data: Option<&CloudData>,
    network_data: Option<&NetworkData>,
    vuln_data: Option<&VulnData>,
) -> Result<ModuleReport, String> {
    let graph = build_graph(ad_data, cloud_data, network_data, vuln_data)?;
    let paths = find_attack_paths(&graph, 20);
    let chokes = find_choke_points(&graph, &paths, 15);
    let report = analyze_report(&graph, &paths, &chokes);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_builds_from_ad_data() {
        let ad = AdData {
            users: vec![AdUser {
                sam_account_name: "admin".into(),
                distinguished_name: "CN=admin,DC=corp".into(),
                sid: "S-1-5-21-123-456-789-500".into(),
                admin_count: true,
                service_account: false,
                dont_req_preauth: false,
                password_last_set: None,
                last_logon: None,
                description: "".into(),
                member_of: vec!["CN=Domain Admins,DC=corp".into()],
            }],
            groups: vec![AdGroup {
                name: "Domain Admins".into(),
                distinguished_name: "CN=Domain Admins,DC=corp".into(),
                sid: "S-1-5-21-123-456-789-512".into(),
                group_type: "Security".into(),
                members: vec!["CN=admin,DC=corp".into()],
                admin_count: true,
            }],
            ..Default::default()
        };

        let graph = build_graph(Some(&ad), None, None, None).unwrap();
        assert!(graph.nodes.contains_key("user:S-1-5-21-123-456-789-500"));
        assert!(graph.nodes.contains_key("group:CN=Domain Admins,DC=corp"));
        assert!(!graph.metadata.crown_jewels.is_empty());
    }

    #[test]
    fn find_attack_paths_finds_path() {
        let mut graph = AttackGraph::default();
        graph.nodes.insert(
            "entry".into(),
            GraphNode {
                id: "entry".into(),
                label: "Entry".into(),
                node_type: NodeType::User,
                properties: HashMap::new(),
                risk_score: 3.0,
                tier: 3,
            },
        );
        graph.nodes.insert(
            "pivot".into(),
            GraphNode {
                id: "pivot".into(),
                label: "Pivot".into(),
                node_type: NodeType::Computer,
                properties: HashMap::new(),
                risk_score: 5.0,
                tier: 2,
            },
        );
        graph.nodes.insert(
            "target".into(),
            GraphNode {
                id: "target".into(),
                label: "Target".into(),
                node_type: NodeType::Domain,
                properties: HashMap::new(),
                risk_score: 9.0,
                tier: 0,
            },
        );
        graph.edges.push(GraphEdge {
            from: "entry".into(),
            to: "pivot".into(),
            edge_type: EdgeType::HasSession,
            technique: "T1550".into(),
            weight: 0.2,
            properties: HashMap::new(),
        });
        graph.edges.push(GraphEdge {
            from: "pivot".into(),
            to: "target".into(),
            edge_type: EdgeType::AdminTo,
            technique: "T1021".into(),
            weight: 0.3,
            properties: HashMap::new(),
        });
        graph.metadata.entry_points.push("entry".into());
        graph.metadata.crown_jewels.push("target".into());

        let paths = find_attack_paths(&graph, 5);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].nodes, vec!["entry", "pivot", "target"]);
        assert!((paths[0].total_weight - 0.5).abs() < 0.01);
    }

    #[test]
    fn find_choke_points_ranks_correctly() {
        let mut graph = AttackGraph::default();
        graph.nodes.insert(
            "a".into(),
            GraphNode {
                id: "a".into(),
                label: "A".into(),
                node_type: NodeType::User,
                properties: HashMap::new(),
                risk_score: 3.0,
                tier: 3,
            },
        );
        graph.nodes.insert(
            "b".into(),
            GraphNode {
                id: "b".into(),
                label: "B".into(),
                node_type: NodeType::Computer,
                properties: HashMap::new(),
                risk_score: 5.0,
                tier: 2,
            },
        );
        graph.nodes.insert(
            "c".into(),
            GraphNode {
                id: "c".into(),
                label: "C".into(),
                node_type: NodeType::Domain,
                properties: HashMap::new(),
                risk_score: 9.0,
                tier: 0,
            },
        );

        let paths = vec![
            AttackPath {
                nodes: vec!["a".into(), "b".into(), "c".into()],
                edges: vec![],
                total_weight: 0.5,
                techniques: vec![],
                risk_score: 9.0,
            },
            AttackPath {
                nodes: vec!["a".into(), "b".into(), "c".into()],
                edges: vec![],
                total_weight: 0.6,
                techniques: vec![],
                risk_score: 9.0,
            },
        ];

        let chokes = find_choke_points(&graph, &paths, 5);
        assert_eq!(chokes[0].node_id, "b");
        assert_eq!(chokes[0].path_count, 2);
    }
}
