fn main() {
    if let Err(e) = tpp::run() {
        eprintln!("tpp: {e:#}");
        std::process::exit(1);
    }
}
