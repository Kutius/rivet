use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "rivet",
    version,
    about = "Rivet: a fast, portable-first Windows package manager"
)]
pub struct Cli {
    #[arg(short, long, global = true, help = "Show extra diagnostic output")]
    pub verbose: bool,
    #[arg(
        long,
        global = true,
        help = "Allow unsigned remote indexes (insecure escape hatch)"
    )]
    pub insecure: bool,
    #[arg(long, global = true, help = "Print machine-readable JSON")]
    pub json: bool,
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
        #[arg(long, help = "Remove even if another package depends on it")]
        force: bool,
    },
    /// Upgrade installed packages
    Upgrade {
        packages: Vec<String>,
        #[arg(long, help = "Upgrade every installed package")]
        all: bool,
        #[arg(long, help = "Upgrade even if the package is pinned")]
        force: bool,
    },
    /// Pin installed packages at their current version
    Pin { packages: Vec<String> },
    /// Unpin installed packages
    Unpin { packages: Vec<String> },
    /// Check installed packages for damage
    Verify,
    /// Switch a package back to its previous version
    Rollback { packages: Vec<String> },
    /// Remove old store versions and stale cache entries
    Cleanup {
        #[arg(long, help = "List what would be removed without deleting it")]
        dry_run: bool,
    },
    /// Check the Rivet installation for common problems
    Doctor {
        #[arg(long, help = "Repair broken links and missing shims")]
        fix: bool,
    },
    /// Manage third-party taps
    Tap {
        #[command(subcommand)]
        command: TapCommands,
    },
    /// Inspect or edit configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Install the Rivet binaries and add the bin directory to the user PATH
    Init {
        #[arg(long, help = "Install the binaries without changing PATH")]
        no_path: bool,
    },
    /// Update the Rivet binaries from the latest release
    SelfUpdate {
        #[arg(long, help = "Reinstall even if the version is current")]
        force: bool,
    },
    /// Remove Rivet, its packages, and its data
    SelfUninstall {
        #[arg(long, help = "Confirm removal without prompting")]
        yes: bool,
    },
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
pub enum ConfigCommands {
    /// Print the config file path
    Path,
    /// Read a config value
    Get { key: String },
    /// Set a config value
    Set { key: String, value: String },
}
