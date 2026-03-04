use std::env;
use std::fs;
use std::io::{self, Read};
use std::process;

use goldilocks::{format_source, FormatConfig};

fn main() {
    let args: Vec<String> = env::args().collect();

    let config = FormatConfig::default();

    if args.len() < 2 || args[1] == "-" {
        // Read from stdin.
        let mut source = String::new();
        io::stdin()
            .read_to_string(&mut source)
            .expect("failed to read stdin");
        match format_source(&source, &config) {
            Ok(output) => print!("{}", output),
            Err(e) => {
                eprintln!("goldilocks: {}", e);
                process::exit(1);
            }
        }
    } else {
        // Process files.
        for path in &args[1..] {
            let source = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("goldilocks: {}: {}", path, e);
                    process::exit(1);
                }
            };
            match format_source(&source, &config) {
                Ok(output) => print!("{}", output),
                Err(e) => {
                    eprintln!("goldilocks: {}: {}", path, e);
                    process::exit(1);
                }
            }
        }
    }
}
