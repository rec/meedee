fn main() {
    if let Err(error) = middy::transport::subprocess::serve() {
        eprintln!("middy-worker: {error}");
        std::process::exit(1);
    }
}
