//! Type definitions for improved type safety in the cache module
//!
//! This module provides strongly-typed wrappers for common data patterns
//! to prevent stringly-typed errors and improve API clarity.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Validate that a crate name is safe for use in file paths
fn validate_crate_name(name: &str) -> Result<()> {
    // Check for path traversal attempts
    if name.contains("..") || name.contains("/") || name.contains("\\") {
        bail!(
            "Invalid crate name '{}': contains path separators or traversal sequences",
            name
        );
    }

    // Check for absolute paths
    if name.starts_with('/')
        || name.starts_with('\\')
        || (name.len() > 2 && name.chars().nth(1) == Some(':'))
    {
        bail!(
            "Invalid crate name '{}': appears to be an absolute path",
            name
        );
    }

    // Ensure it's a valid crate name pattern (alphanumeric, underscore, dash)
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        bail!(
            "Invalid crate name '{}': contains invalid characters. Only alphanumeric, underscore, and dash are allowed",
            name
        );
    }

    Ok(())
}

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

        // Validate for path traversal and other security issues
        validate_crate_name(&name)?;

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

    #[test]
    fn test_validate_crate_name() {
        // Valid names
        assert!(validate_crate_name("serde").is_ok());
        assert!(validate_crate_name("tokio-util").is_ok());
        assert!(validate_crate_name("async_trait").is_ok());
        assert!(validate_crate_name("log2").is_ok());
        assert!(validate_crate_name("h3").is_ok());

        // Path traversal attempts
        assert!(validate_crate_name("../etc/passwd").is_err());
        assert!(validate_crate_name("crate/../../../etc").is_err());
        assert!(validate_crate_name("..").is_err());
        assert!(validate_crate_name("./config").is_err());
        assert!(validate_crate_name("crate/..").is_err());

        // Path separators
        assert!(validate_crate_name("some/path").is_err());
        assert!(validate_crate_name("some\\path").is_err());
        assert!(validate_crate_name("path/to/crate").is_err());

        // Absolute paths
        assert!(validate_crate_name("/etc/passwd").is_err());
        assert!(validate_crate_name("\\Windows\\System32").is_err());
        assert!(validate_crate_name("C:\\Windows").is_err());
        assert!(validate_crate_name("C:").is_err());

        // Invalid characters
        assert!(validate_crate_name("crate@2.0").is_err());
        assert!(validate_crate_name("my crate").is_err());
        assert!(validate_crate_name("crate!name").is_err());
        assert!(validate_crate_name("crate#name").is_err());
        assert!(validate_crate_name("crate$name").is_err());
    }

    #[test]
    fn test_crate_identifier_validation() {
        // Valid crate identifiers
        assert!(CrateIdentifier::new("serde", "1.0.0").is_ok());
        assert!(CrateIdentifier::new("tokio-util", "0.7.0").is_ok());

        // Invalid names should fail
        assert!(CrateIdentifier::new("../malicious", "1.0.0").is_err());
        assert!(CrateIdentifier::new("/etc/passwd", "1.0.0").is_err());
        assert!(CrateIdentifier::new("crate@2.0", "1.0.0").is_err());

        // Empty names/versions should fail
        assert!(CrateIdentifier::new("", "1.0.0").is_err());
        assert!(CrateIdentifier::new("serde", "").is_err());
    }
}
