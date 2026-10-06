fn main() -> std::process::ExitCode {
    eprintln!(
        "CLI commands are not implemented yet (Arrow schema v{}).",
        atif_arrow::SCHEMA_VERSION
    );
    std::process::ExitCode::FAILURE
}
