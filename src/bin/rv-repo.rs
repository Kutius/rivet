use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use rivet::catalog::{self, AddOptions};
use rivet::index;
use rivet::trust;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(
    name = "rv-repo",
    version,
    about = "Rivet maintainer tooling for package repositories"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Build index.json from packages/*.toml
    Build {
        path: String,
        #[arg(long)]
        output: Option<String>,
    },
    /// Add a package from the latest GitHub release
    Add {
        /// GitHub repository in owner/repo form
        repository: String,
        /// Repository root containing packages/ (defaults to repo/main)
        #[arg(long, default_value = "repo/main")]
        path: String,
        /// Package name (defaults to the repository name)
        #[arg(long)]
        name: Option<String>,
        /// Release asset name or substring; auto-detected when omitted
        #[arg(long)]
        asset: Option<String>,
        /// Executable path inside the extracted archive
        #[arg(long)]
        bin: Option<String>,
    },
    /// Update GitHub-hosted packages to their latest release
    Update {
        /// Repository root containing packages/ (defaults to repo/main)
        #[arg(default_value = "repo/main")]
        path: String,
        /// Show available updates without changing files
        #[arg(long)]
        check: bool,
        /// Limit the update to one package
        #[arg(long)]
        package: Option<String>,
    },
    /// Add every missing package listed in sources.toml
    Sync {
        /// Repository root containing packages/ (defaults to repo/main)
        #[arg(default_value = "repo/main")]
        path: String,
        /// Source list (defaults to <path>/sources.toml)
        #[arg(long)]
        sources: Option<String>,
        /// Show what would be added without changing files
        #[arg(long)]
        dry_run: bool,
        /// Add at most N missing packages
        #[arg(long)]
        limit: Option<usize>,
        /// Exit successfully when some sources fail
        #[arg(long)]
        allow_failures: bool,
    },
    /// Generate an Ed25519 signing key pair
    Keygen {
        /// Directory for rivet-signing.key and rivet-signing.pub
        #[arg(long)]
        out: Option<String>,
    },
    /// Sign index.json with an Ed25519 private key
    Sign {
        /// Repository directory or index.json path
        path: String,
        /// Private key produced by rv-repo keygen
        #[arg(long)]
        key: String,
    },
    /// Print the public key for a private key
    PublicKey {
        /// Private key produced by rv-repo keygen
        #[arg(long)]
        key: String,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rv-repo: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build { path, output } => {
            let path = PathBuf::from(path);
            let output = output.map(PathBuf::from);
            let index = index::build_index(&path, output.as_deref())?;
            println!("built index with {} package versions", index.packages.len());
            Ok(())
        }
        Commands::Add {
            repository,
            path,
            name,
            asset,
            bin,
        } => {
            let manifest = catalog::add_github_package(
                Path::new(&path),
                &repository,
                &AddOptions { name, asset, bin },
            )?;
            println!("wrote {}", manifest.display());
            println!("index rebuilt; sign it before publishing");
            Ok(())
        }
        Commands::Update {
            path,
            check,
            package,
        } => {
            let changes =
                catalog::update_github_packages(Path::new(&path), check, package.as_deref())?;
            if changes.is_empty() {
                println!("all GitHub-hosted packages are up to date");
            } else if check {
                println!("{} update(s) available", changes.len());
            } else {
                println!("updated {} package(s); index rebuilt", changes.len());
            }
            Ok(())
        }
        Commands::Sync {
            path,
            sources,
            dry_run,
            limit,
            allow_failures,
        } => {
            let root = PathBuf::from(path);
            let sources = sources
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("sources.toml"));
            let report =
                catalog::sync_github_packages(&root, &sources, dry_run, limit, allow_failures)?;
            let verb = if dry_run { "would add" } else { "added" };
            println!(
                "{} {} package(s), {} already present, {} failed",
                verb,
                report.added.len(),
                report.skipped.len(),
                report.failed.len()
            );
            Ok(())
        }
        Commands::Keygen { out } => {
            let directory = out
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("keys"));
            let (private, public) = trust::keygen(&directory)?;
            let key_hex = fs::read_to_string(&public)?.trim().to_string();
            println!("private key: {}", private.display());
            println!("public key:  {}", public.display());
            println!("public key hex: {key_hex}");
            println!("trust this key with: rivet config set trusted_key {key_hex}");
            Ok(())
        }
        Commands::Sign { path, key } => {
            let path = PathBuf::from(path);
            let index_path = if path.is_dir() {
                path.join("index.json")
            } else {
                path
            };
            if !index_path.exists() {
                bail!("index file not found: {}", index_path.display());
            }
            let pair = trust::load_private_key(Path::new(&key))?;
            let data = fs::read(&index_path)
                .with_context(|| format!("reading {}", index_path.display()))?;
            let signature = trust::sign(&pair, &data);
            let signature_path = trust::signature_path(&index_path);
            fs::write(
                &signature_path,
                format!("{}\n", serde_json::to_string_pretty(&signature)?),
            )?;
            println!(
                "signed {} (key {})",
                index_path.display(),
                signature.public_key
            );
            Ok(())
        }
        Commands::PublicKey { key } => {
            let pair = trust::load_private_key(Path::new(&key))?;
            println!("{}", trust::public_key_hex(&pair));
            Ok(())
        }
    }
}
