use goldilocks::{format_source, FormatConfig};
use std::fs;
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn run_fixture(name: &str) {
    let input_path = fixture_path(&format!("{}.rb", name));
    let expected_path = fixture_path(&format!("{}.expected.rb", name));

    let input = fs::read_to_string(&input_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", input_path.display(), e));
    let expected = fs::read_to_string(&expected_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", expected_path.display(), e));

    let config = FormatConfig::default();
    let actual = format_source(&input, &config).unwrap_or_else(|e| {
        panic!("formatting failed for {}: {}", name, e);
    });

    // Normalize: ensure both end with exactly one newline.
    let expected = expected.trim_end().to_string() + "\n";
    let actual = actual.trim_end().to_string() + "\n";

    if actual != expected {
        eprintln!("=== FIXTURE: {} ===", name);
        eprintln!("--- EXPECTED ---");
        for (i, line) in expected.lines().enumerate() {
            eprintln!("{:4}| {}", i + 1, line);
        }
        eprintln!("--- ACTUAL ---");
        for (i, line) in actual.lines().enumerate() {
            eprintln!("{:4}| {}", i + 1, line);
        }
        eprintln!("--- DIFF ---");
        let expected_lines: Vec<&str> = expected.lines().collect();
        let actual_lines: Vec<&str> = actual.lines().collect();
        let max_lines = expected_lines.len().max(actual_lines.len());
        for i in 0..max_lines {
            let exp = expected_lines.get(i).unwrap_or(&"<missing>");
            let act = actual_lines.get(i).unwrap_or(&"<missing>");
            if exp != act {
                eprintln!("  line {}: ", i + 1);
                eprintln!("    exp: {:?}", exp);
                eprintln!("    act: {:?}", act);
            }
        }
        panic!("fixture {} did not match", name);
    }
}

#[test]
fn fixture_01_simple_assignments() {
    run_fixture("01_simple_assignments");
}

#[test]
fn fixture_02_method_definitions() {
    run_fixture("02_method_definitions");
}

#[test]
fn fixture_03_class_module() {
    run_fixture("03_class_module");
}

#[test]
fn fixture_04_conditionals() {
    run_fixture("04_conditionals");
}

#[test]
fn fixture_05_blocks() {
    run_fixture("05_blocks");
}

#[test]
fn fixture_06_hashes_arrays() {
    run_fixture("06_hashes_arrays");
}

#[test]
fn fixture_07_method_chains() {
    run_fixture("07_method_chains");
}

#[test]
fn fixture_08_strings_heredocs() {
    run_fixture("08_strings_heredocs");
}

#[test]
fn fixture_09_long_args() {
    run_fixture("09_long_args");
}

#[test]
fn fixture_10_realistic_mixed() {
    run_fixture("10_realistic_mixed");
}

#[test]
fn fixture_11_boundary() {
    run_fixture("11_boundary");
}
