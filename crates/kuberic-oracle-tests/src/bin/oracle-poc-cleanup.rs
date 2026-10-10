fn main() {
    let result = kuberic_oracle_tests::fixture::FixtureConfig::from_environment()
        .and_then(kuberic_oracle_tests::fixture::OracleFixture::cleanup_existing);
    if let Err(failure) = result {
        eprintln!("{failure}");
        std::process::exit(1);
    }
}
