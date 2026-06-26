fn main() {
    if let Err(err) = lovely::cli::run() {
        eprintln!("lovely: {err}");
        std::process::exit(1);
    }
}
