fn main() {
    println!(
        "dd {} (ente lib {})",
        env!("CARGO_PKG_VERSION"),
        ente::version()
    );
}
