use lovely::cli;

fn main() {
    if let Err(err) = cli::run() {
        eprintln!("lovely: {err}");
        std::process::exit(1);
    }
}
