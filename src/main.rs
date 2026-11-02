use clap::Parser;
use rivet::cli::Cli;

fn main() {
    rivet::download::init_crypto();
    let cli = Cli::parse();
    if let Err(error) = rivet::run(cli) {
        eprintln!("rv: {error:#}");
        std::process::exit(1);
    }
}
