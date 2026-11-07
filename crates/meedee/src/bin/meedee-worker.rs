fn main() {
    if let Err(error) = meedee::transport::subprocess::serve() {
        eprintln!("meedee-worker: {error}");
        std::process::exit(1);
    }
}
