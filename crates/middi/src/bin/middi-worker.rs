fn main() {
    if let Err(error) = middi::transport::subprocess::serve() {
        eprintln!("middi-worker: {error}");
        std::process::exit(1);
    }
}
