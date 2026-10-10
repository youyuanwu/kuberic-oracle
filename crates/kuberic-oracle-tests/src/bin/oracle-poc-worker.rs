fn main() {
    if kuberic_oracle_tests::poc::worker::run_observation_worker().is_err() {
        std::process::exit(1);
    }
}
