//! Type definitions for improved type safety in the cache module
//!
//! This module provides strongly-typed wrappers for common data patterns
//! to prevent stringly-typed errors and improve API clarity.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Represents a crate identifier with name and version
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CrateIdentifier {
    name: String,
    version: String,
}

impl CrateIdentifier {
    /// Create a new crate identifier
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Result<Self> {
        let name = name.into();
        let version = version.into();

        // Validate crate name
        if name.is_empty() {
            bail!("Crate name cannot be empty");
        }

        // Validate version
        if version.is_empty() {
            bail!("Crate version cannot be empty");
        }

        Ok(Self { name, version })
    }

    /// Get the crate name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the crate version
    pub fn version(&self) -> &str {
        &self.version
    }

}

impl fmt::Display for CrateIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.name, self.version)
    }
}

impl FromStr for CrateIdentifier {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.rsplitn(2, '-').collect();
        if parts.len() != 2 {
            bail!("Invalid crate identifier format. Expected 'name-version'");
        }

        // Note: rsplitn returns in reverse order
        let version = parts[0];
        let name = parts[1];

        Self::new(name, version)
    }
}

/// Represents a path to a workspace member
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemberPath {
    path: PathBuf,
    member_name: String,
}

impl MemberPath {
    /// Create a new member path
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        // Validate path
        if path.as_os_str().is_empty() {
            bail!("Member path cannot be empty");
        }

        // Extract member name from the path
        let member_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid member path: no file name component"))?
            .to_string();

        Ok(Self {
            path: path.to_path_buf(),
            member_name,
        })
    }

}

impl fmt::Display for MemberPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.display())
    }
}

impl FromStr for MemberPath {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}

impl AsRef<Path> for MemberPath {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crate_identifier() -> Result<()> {
        let id = CrateIdentifier::new("serde", "1.0.0")?;
        assert_eq!(id.name(), "serde");
        assert_eq!(id.version(), "1.0.0");
        assert_eq!(id.to_string(), "serde-1.0.0");

        // Test validation
        assert!(CrateIdentifier::new("", "1.0.0").is_err());
        assert!(CrateIdentifier::new("serde", "").is_err());

        Ok(())
    }

    #[test]
    fn test_crate_identifier_from_str() -> Result<()> {
        let id: CrateIdentifier = "serde-1.0.0".parse()?;
        assert_eq!(id.name(), "serde");
        assert_eq!(id.version(), "1.0.0");

        // Test with crate names containing hyphens
        let id: CrateIdentifier = "rust-docs-mcp-0.1.0".parse()?;
        assert_eq!(id.name(), "rust-docs-mcp");
        assert_eq!(id.version(), "0.1.0");

        // Test invalid format
        assert!("invalid".parse::<CrateIdentifier>().is_err());

        Ok(())
    }

    #[test]
    fn test_member_path() -> Result<()> {
        let member = MemberPath::new("crates/rmcp")?;
        assert_eq!(member.path, Path::new("crates/rmcp"));
        assert_eq!(member.member_name, "rmcp");

        // Test validation
        assert!(MemberPath::new("").is_err());

        Ok(())
    }

}
