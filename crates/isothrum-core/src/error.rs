//! What can be wrong with a topology, said so the user knows what to fix.

use crate::addressing::LINK_BASE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    UnknownNode {
        link: usize,
        node: String,
    },
    SelfLink {
        link: usize,
        node: String,
    },
    PortInUse {
        node: String,
        port: String,
    },
    TooManyLinks(usize),
    /// A node is on a LAN the lab doesn't have.
    UnknownLan {
        node: String,
        lan: String,
    },
    /// A LAN's name is one the cables use (`link-<n>`).
    ReservedLanName(String),
    /// A LAN's CIDR isn't `a.b.c.d/n`.
    BadCidr {
        lan: String,
        cidr: String,
    },
    /// Two nodes ask for the same address on a LAN.
    AddressTaken {
        lan: String,
        octet: u8,
        nodes: (String, String),
    },
    /// A LAN has more machines than free addresses.
    LanFull {
        lan: String,
        cidr: String,
    },
    /// A switch has the name of a LAN: its VLANs nest under a LAN of that name in the spec.
    SwitchNameTaken {
        switch: String,
    },
    Spec(String),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::UnknownNode { link, node } => {
                write!(
                    f,
                    "link {link} refers to node `{node}`, which the lab doesn't have"
                )
            }
            CompileError::SelfLink { link, node } => {
                write!(f, "link {link} cables `{node}` to itself")
            }
            CompileError::PortInUse { node, port } => {
                write!(f, "port `{port}` of `{node}` already has a cable")
            }
            CompileError::TooManyLinks(n) => {
                write!(
                    f,
                    "{n} cables don't fit in {}.{}.0.0/16",
                    LINK_BASE[0], LINK_BASE[1]
                )
            }
            CompileError::UnknownLan { node, lan } => {
                write!(f, "`{node}` is on LAN `{lan}`, which the lab doesn't have")
            }
            CompileError::ReservedLanName(lan) => {
                write!(
                    f,
                    "`{lan}` is a name the cables use; call the LAN something else"
                )
            }
            CompileError::BadCidr { lan, cidr } => {
                write!(f, "LAN `{lan}`: `{cidr}` isn't a block like 10.10.1.0/24")
            }
            CompileError::AddressTaken {
                lan,
                octet,
                nodes: (a, b),
            } => {
                write!(f, "on LAN `{lan}`, `{a}` and `{b}` both ask for .{octet}")
            }
            CompileError::LanFull { lan, cidr } => {
                write!(
                    f,
                    "LAN `{lan}` ({cidr}) has no free address left; use a larger block"
                )
            }
            CompileError::SwitchNameTaken { switch } => {
                write!(
                    f,
                    "switch `{switch}` has the name of a LAN; rename one of them"
                )
            }
            CompileError::Spec(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CompileError {}
