//! # Cache Module
//!
//! This module provides caching functionality for Rust crates and their documentation.
//!
//! ## Key Components
//!
//! - [`service`] - Main caching service that coordinates all cache operations
//! - [`storage`] - Low-level storage operations for cached crates
//! - [`downloader`] - Downloads crates from various sources (crates.io, GitHub, local)
//! - [`docgen`] - Generates JSON documentation using cargo rustdoc
//! - [`source`] - Source type detection and parsing (crates.io, GitHub, local paths)
//! - [`tools`] - MCP tool implementations for cache operations
//! - [`transaction`] - Transactional updates with automatic rollback
//! - [`types`] - Type definitions for improved type safety
//! - [`utils`] - Common utilities including response formatting
//! - [`workspace`] - Workspace crate handling
//! - [`outputs`] - Output types for cache operations

pub mod constants;
pub mod docgen;
pub mod downloader;
pub mod member_utils;
pub mod outputs;
pub mod service;
pub mod source;
pub mod storage;
pub mod task_formatter;
pub mod task_manager;
pub mod tools;
pub mod transaction;
pub mod types;
pub mod utils;
pub mod workspace;

pub use service::CrateCache;
