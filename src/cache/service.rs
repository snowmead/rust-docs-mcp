use crate::cache::storage::CacheStorage;
use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use futures::StreamExt;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tar::Archive;

/// Service for managing crate caching and documentation generation
#[derive(Debug, Clone)]
pub struct CrateCache {
    storage: CacheStorage,
    client: reqwest::Client,
}

impl CrateCache {
    /// Create a new crate cache instance
    pub fn new() -> Result<Self> {
        Ok(Self {
            storage: CacheStorage::new()?,
            client: reqwest::Client::new(),
        })
    }

    /// Ensure a crate's documentation is available, downloading and generating if necessary
    pub async fn ensure_crate_docs(
        &self,
        name: &str,
        version: &str,
    ) -> Result<rustdoc_types::Crate> {
        // Check if docs already exist
        if self.storage.has_docs(name, version) {
            return self.load_docs(name, version).await;
        }

        // Check if crate is downloaded but docs not generated
        if !self.storage.is_cached(name, version) {
            self.download_crate(name, version).await?;
        }

        // Generate documentation
        self.generate_docs(name, version).await?;

        // Load and return the generated docs
        self.load_docs(name, version).await
    }

    /// Download a crate from crates.io
    pub async fn download_crate(&self, name: &str, version: &str) -> Result<PathBuf> {
        let url = format!(
            "https://crates.io/api/v1/crates/{}/{}/download",
            name, version
        );

        tracing::info!("Downloading crate {}-{} from {}", name, version, url);

        // Create response - don't use Accept: application/json as it returns JSON instead of redirect
        let response = self
            .client
            .get(&url)
            .header(
                "User-Agent",
                format!("{}/{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
            )
            .send()
            .await
            .context("Failed to download crate")?;

        if !response.status().is_success() {
            bail!(
                "Failed to download crate: HTTP {} - {}",
                response.status(),
                response
                    .status()
                    .canonical_reason()
                    .unwrap_or("Unknown error")
            );
        }

        // Create temporary file for download
        let temp_dir = std::env::temp_dir();
        let temp_file_path = temp_dir.join(format!("{}-{}.tar.gz", name, version));
        let mut temp_file =
            File::create(&temp_file_path).context("Failed to create temporary file")?;

        // Stream download to file
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Failed to read download chunk")?;
            temp_file
                .write_all(&chunk)
                .context("Failed to write to temporary file")?;
        }

        // Extract the crate
        let source_path = self.storage.source_path(name, version);
        self.storage.ensure_dir(&source_path)?;

        let tar_gz = File::open(&temp_file_path).context("Failed to open downloaded file")?;
        let tar = GzDecoder::new(tar_gz);
        let mut archive = Archive::new(tar);

        // Extract with proper path handling
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?;

            // Skip the top-level directory (crate-version/)
            let components: Vec<_> = path.components().collect();
            if components.len() > 1 {
                let relative_path: PathBuf = components[1..].iter().collect();
                let dest_path = source_path.join(relative_path);

                if let Some(parent) = dest_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                entry.unpack(&dest_path)?;
            }
        }

        // Clean up temp file
        std::fs::remove_file(&temp_file_path).ok();

        // Save metadata for the cached crate
        self.storage.save_metadata(name, version)?;

        tracing::info!("Successfully downloaded and extracted {}-{}", name, version);
        Ok(source_path)
    }

    /// Generate JSON documentation for a crate
    pub async fn generate_docs(&self, name: &str, version: &str) -> Result<PathBuf> {
        let source_path = self.storage.source_path(name, version);
        let docs_path = self.storage.docs_path(name, version);

        if !source_path.exists() {
            bail!(
                "Source not found for {}-{}. Download it first.",
                name,
                version
            );
        }

        tracing::info!("Generating documentation for {}-{}", name, version);

        // Run cargo rustdoc with JSON output
        let output = Command::new("cargo")
            .args(&[
                "+nightly",
                "rustdoc",
                "--",
                "--output-format",
                "json",
                "-Z",
                "unstable-options",
            ])
            .current_dir(&source_path)
            .output()
            .context("Failed to run cargo rustdoc")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to generate documentation: {}", stderr);
        }

        // Find the generated JSON file in target/doc
        let doc_dir = source_path.join("target").join("doc");
        let json_file = self.find_json_doc(&doc_dir, name)?;

        // Copy the JSON file to our cache location
        std::fs::copy(&json_file, &docs_path).context("Failed to copy documentation to cache")?;

        // Update metadata to reflect that docs are now generated
        self.storage.save_metadata(name, version)?;

        tracing::info!(
            "Successfully generated documentation for {}-{}",
            name,
            version
        );
        Ok(docs_path)
    }

    /// Find the JSON documentation file in the doc directory
    fn find_json_doc(&self, doc_dir: &Path, crate_name: &str) -> Result<PathBuf> {
        // The JSON file is usually named after the crate with underscores
        let normalized_name = crate_name.replace("-", "_");
        let json_path = doc_dir.join(format!("{}.json", normalized_name));

        if json_path.exists() {
            return Ok(json_path);
        }

        // If not found, search for any JSON file
        for entry in std::fs::read_dir(doc_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                return Ok(path);
            }
        }

        bail!("No JSON documentation file found in {:?}", doc_dir);
    }

    /// Load documentation from cache
    pub async fn load_docs(&self, name: &str, version: &str) -> Result<rustdoc_types::Crate> {
        let docs_path = self.storage.docs_path(name, version);

        if !docs_path.exists() {
            bail!("Documentation not found for {}-{}", name, version);
        }

        let json_string = tokio::fs::read_to_string(&docs_path)
            .await
            .context("Failed to read documentation file")?;

        let crate_docs: rustdoc_types::Crate =
            serde_json::from_str(&json_string).context("Failed to parse documentation JSON")?;

        Ok(crate_docs)
    }

    /// Get cached versions of a crate
    pub async fn get_cached_versions(&self, name: &str) -> Result<Vec<String>> {
        let cached = self.storage.list_cached_crates()?;
        let versions: Vec<String> = cached
            .into_iter()
            .filter(|meta| meta.name == name)
            .map(|meta| meta.version)
            .collect();

        Ok(versions)
    }

    /// Remove a cached crate version
    pub async fn remove_crate(&self, name: &str, version: &str) -> Result<()> {
        self.storage.remove_crate(name, version)
    }

    /// Get the source path for a crate
    pub fn get_source_path(&self, name: &str, version: &str) -> PathBuf {
        self.storage.source_path(name, version)
    }
}
