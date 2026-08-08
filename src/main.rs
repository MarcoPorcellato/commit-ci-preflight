const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let mut arguments = std::env::args().skip(1);

    match arguments.next().as_deref() {
        Some("--version" | "-V") => println!("commit-ci-preflight {VERSION}"),
        Some("--help" | "-h") | None => print_help(),
        Some(argument) => {
            eprintln!("error: unsupported bootstrap argument: {argument}");
            std::process::exit(2);
        }
    }
}

fn print_help() {
    println!(
        "Commit CI Preflight {VERSION}\n\
         \n\
         Bootstrap CLI. No CI parity or attestation command is active yet.\n\
         \n\
         Usage:\n\
           commit-ci-preflight --help\n\
           commit-ci-preflight --version"
    );
}

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn package_version_is_available() {
        assert!(!VERSION.is_empty());
    }
}
