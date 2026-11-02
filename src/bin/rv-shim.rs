fn main() {
    if let Err(error) = rivet::shim::run_from_current_exe() {
        eprintln!("rv-shim: {error:#}");
        std::process::exit(1);
    }
}
