//! Source type detection and parsing for crates
//!
//! This module handles the detection and parsing of different crate sources,
//! including crates.io, GitHub repositories, and local paths.

use serde::{Deserialize, Serialize};

/// Represents the different sources from which a crate can be obtained
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SourceType {
    /// Crate from crates.io registry
    CratesIo,
    /// Crate from a GitHub repository
    GitHub {
        /// The base repository URL (e.g., https://github.com/user/repo)
        url: String,
        /// Optional path within the repository to the crate
        repo_path: Option<String>,
        /// Branch or tag reference
        reference: GitReference,
    },
    /// Crate from a local file system path
    Local {
        /// The local path to the crate
        path: String,
    },
}

/// Git reference type (branch or tag)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum GitReference {
    Branch(String),
    Tag(String),
    Default,
}

/// Detects the source type from a source string
pub struct SourceDetector;

impl SourceDetector {
    /// Detect the source type from an optional source string
    pub fn detect(source: Option<&str>) -> SourceType {
        match source {
            None => SourceType::CratesIo,
            Some(s) => {
                if s.starts_with("http://") || s.starts_with("https://") {
                    Self::parse_url(s)
                } else if Self::is_local_path(s) {
                    SourceType::Local {
                        path: s.to_string(),
                    }
                } else {
                    SourceType::CratesIo
                }
            }
        }
    }

    /// Check if a string represents a local path
    fn is_local_path(s: &str) -> bool {
        s.starts_with('/')
            || s.starts_with("~/")
            || s.starts_with("../")
            || s.starts_with("./")
            || s.contains('/')
            || s.contains('\\')
    }

    /// Parse a URL to determine if it's a GitHub URL
    fn parse_url(url: &str) -> SourceType {
        // Check for #branch: or #tag: suffix
        let (base_url, reference) = if let Some(pos) = url.find("#branch:") {
            let (base, branch_part) = url.split_at(pos);
            let branch = branch_part.trim_start_matches("#branch:");
            (base.to_string(), Some(GitReference::Branch(branch.to_string())))
        } else if let Some(pos) = url.find("#tag:") {
            let (base, tag_part) = url.split_at(pos);
            let tag = tag_part.trim_start_matches("#tag:");
            (base.to_string(), Some(GitReference::Tag(tag.to_string())))
        } else {
            (url.to_string(), None)
        };

        // Normalize http to https for GitHub
        let normalized_url = if base_url.starts_with("http://github.com/") {
            base_url.replace("http://", "https://")
        } else {
            base_url
        };

        if let Some(github_part) = normalized_url.strip_prefix("https://github.com/") {
            Self::parse_github_url(github_part, reference)
        } else {
            // Not a GitHub URL, treat as local path
            SourceType::Local {
                path: url.to_string(),
            }
        }
    }

    /// Parse GitHub URL components
    fn parse_github_url(github_part: &str, explicit_reference: Option<GitReference>) -> SourceType {
        let parts: Vec<&str> = github_part.split('/').collect();

        if parts.len() >= 2 {
            let base_url = format!("https://github.com/{}/{}", parts[0], parts[1]);

            // Check if there's a path specification (tree/branch/path)
            if parts.len() > 4 && parts[2] == "tree" {
                // URL format: github.com/user/repo/tree/branch/path/to/crate
                let branch = parts[3];
                let repo_path = parts[4..].join("/");

                SourceType::GitHub {
                    url: base_url,
                    repo_path: Some(repo_path),
                    reference: explicit_reference.unwrap_or_else(|| GitReference::Branch(branch.to_string())),
                }
            } else {
                // Simple repository URL
                SourceType::GitHub {
                    url: base_url,
                    repo_path: None,
                    reference: explicit_reference.unwrap_or(GitReference::Default),
                }
            }
        } else {
            // Invalid GitHub URL format
            SourceType::Local {
                path: format!("https://github.com/{github_part}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_crates_io() {
        assert_eq!(SourceDetector::detect(None), SourceType::CratesIo);
        assert_eq!(SourceDetector::detect(Some("serde")), SourceType::CratesIo);
    }

    #[test]
    fn test_detect_local_paths() {
        assert!(matches!(
            SourceDetector::detect(Some("/absolute/path")),
            SourceType::Local { .. }
        ));
        assert!(matches!(
            SourceDetector::detect(Some("~/home/path")),
            SourceType::Local { .. }
        ));
        assert!(matches!(
            SourceDetector::detect(Some("./relative/path")),
            SourceType::Local { .. }
        ));
        assert!(matches!(
            SourceDetector::detect(Some("../parent/path")),
            SourceType::Local { .. }
        ));
    }

    #[test]
    fn test_detect_github_urls() {
        match SourceDetector::detect(Some("https://github.com/rust-lang/rust")) {
            SourceType::GitHub {
                url,
                repo_path,
                reference,
            } => {
                assert_eq!(url, "https://github.com/rust-lang/rust");
                assert_eq!(repo_path, None);
                assert_eq!(reference, GitReference::Default);
            }
            _ => panic!("Expected GitHub source"),
        }

        match SourceDetector::detect(Some(
            "https://github.com/rust-lang/rust/tree/master/src/libstd",
        )) {
            SourceType::GitHub {
                url,
                repo_path,
                reference,
            } => {
                assert_eq!(url, "https://github.com/rust-lang/rust");
                assert_eq!(repo_path, Some("src/libstd".to_string()));
                assert!(matches!(reference, GitReference::Branch(b) if b == "master"));
            }
            _ => panic!("Expected GitHub source with path"),
        }
    }

    #[test]
    fn test_detect_github_with_tag() {
        match SourceDetector::detect(Some("https://github.com/serde-rs/serde#tag:v1.0.136")) {
            SourceType::GitHub {
                url,
                repo_path,
                reference,
            } => {
                assert_eq!(url, "https://github.com/serde-rs/serde");
                assert_eq!(repo_path, None);
                assert!(matches!(reference, GitReference::Tag(t) if t == "v1.0.136"));
            }
            _ => panic!("Expected GitHub source with tag"),
        }
    }

    #[test]
    fn test_detect_github_with_branch() {
        match SourceDetector::detect(Some("https://github.com/rust-lang/rust-clippy#branch:master")) {
            SourceType::GitHub {
                url,
                repo_path,
                reference,
            } => {
                assert_eq!(url, "https://github.com/rust-lang/rust-clippy");
                assert_eq!(repo_path, None);
                assert!(matches!(reference, GitReference::Branch(b) if b == "master"));
            }
            _ => panic!("Expected GitHub source with branch"),
        }
    }
}
