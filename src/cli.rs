use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "rv",
    version,
    about = "Rivet: a fast, portable-first Windows package manager"
)]
pub struct Cli {
    #[arg(short, long, global = true, help = "Show extra diagnostic output")]
    pub verbose: bool,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Refresh the main index and any configured taps
    Update,
    /// Search the local package index
    Search {
        /// Search terms (omit to list packages)
        query: Option<String>,
    },
    /// List installed packages
    List,
    /// Show installed versions and available updates
    Status {
        #[arg(long, help = "Refresh metadata before checking")]
        check: bool,
    },
    /// Show package details
    Info {
        /// Package name, repo/name, or owner/repo/name
        package: String,
    },
    /// Install one or more packages
    Install {
        packages: Vec<String>,
        #[arg(long, help = "Do not install dependencies")]
        no_deps: bool,
        #[arg(long, help = "Reinstall even if the same version is present")]
        force: bool,
    },
    /// Remove installed packages
    Remove {
        packages: Vec<String>,
        #[arg(long, help = "Also delete persist data")]
        purge: bool,
    },
    /// Upgrade installed packages
    Upgrade {
        packages: Vec<String>,
        #[arg(long, help = "Upgrade every installed package")]
        all: bool,
    },
    /// Switch a package back to its previous version
    Rollback { packages: Vec<String> },
    /// Remove old store versions and stale cache entries
    Cleanup,
    /// Check the Rivet installation for common problems
    Doctor,
    /// Manage third-party taps
    Tap {
        #[command(subcommand)]
        command: TapCommands,
    },
    /// Maintainer tooling for package repositories
    Repo {
        #[command(subcommand)]
        command: RepoCommands,
    },
    /// Inspect or edit configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Add the Rivet bin directory to the user PATH
    Init,
}

#[derive(Subcommand, Debug)]
pub enum TapCommands {
    /// Add a tap from owner/repo or an explicit Git URL
    Add { name: String, url: Option<String> },
    /// List configured taps
    List,
    /// Remove a tap
    Remove { name: String },
}

#[derive(Subcommand, Debug, Clone)]
pub enum RepoCommands {
    /// Build index.json from packages/*.toml
    Build {
        path: String,
        #[arg(long)]
        output: Option<String>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigCommands {
    /// Print the config file path
    Path,
    /// Read a config value
    Get { key: String },
    /// Set a config value
    Set { key: String, value: String },
}
