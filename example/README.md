# Tanu Allure Example

This project demonstrates how to use [Allure](https://allurereport.org/) reporting with the [Tanu](https://github.com/tanu-rs/tanu) Rust testing framework.

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) and Cargo
- [Docker](https://docs.docker.com/get-docker/) (for running Allure report server)

## Project Structure

- `src/main.rs`: Example test using Tanu and HTTP client
- `Dockerfile`: Docker configuration for Allure CLI

## Running Tests

To run tests and generate Allure results:

```bash
cargo run test --reporters allure,list
```

This command executes the tests defined in the project and outputs the results in both the console (list) and Allure format. The Allure results will be saved in the `allure-results` directory.

## Generating and Viewing Allure Reports

### Build the Allure Docker Image

```bash
docker build -t allure-cli .
```

### Generate the Allure Report

```bash
docker run -v$PWD/allure-results:/app/allure-results \
           -v$PWD/allure-report:/app/allure-report \
           -i --rm allure-cli allure generate --clean -o allure-report
```

### View the Allure Report in a Web Browser

```bash
docker run -v$PWD/allure-report:/app/allure-report \
           -p4040:4040 -i --rm allure-cli allure open /app/allure-report --host 0.0.0.0 --port 4040
```

After running this command, open your web browser and navigate to [http://localhost:4040](http://localhost:4040) to view the generated report.

## Merging Reports from Multiple Runs

Results from separate runs (different environments, CI jobs, or machines) can be merged
into a single report. Each target is a tanu project in `tanu.toml` (`docker` and
`httpbin_org`), so every test gets a distinct `historyId` and its own parent suite.

```bash
# Local httpbin for the "docker" project
docker run -d --rm --name httpbin -p 8090:80 kennethreitz/httpbin

# Run each project into its own results directory
ALLURE_RESULTS_DIR=results-docker      cargo run -- test --projects docker      --reporters allure,list
ALLURE_RESULTS_DIR=results-httpbin-org cargo run -- test --projects httpbin_org --reporters allure,list

# Merge into one report (Allure 3, configured by allurerc.json)
rm -rf allure-report-merged
allure generate "results-*"

docker stop httpbin
```

The runs don't have to happen on the same machine: upload each results directory as a CI
artifact and run `allure generate` on all of them in a final job.

### Keeping History Across Runs

`allurerc.json` sets `historyPath`, so each `allure generate` appends one entry to
`allure-history.jsonl` (keeping the last 20). The report then shows each test's past results and
trend charts. Allure 3 doesn't read the reporter's `history/history.json`; `allure-history.jsonl`
is the history it uses.

The reporter doesn't delete earlier results, and Allure treats two results with the same
`historyId` as retries. Clear the old result files before each run:

```bash
find . -path './results-*/*-result.json' -delete

ALLURE_RESULTS_DIR=results-docker      cargo run -- test --projects docker      --reporters allure,list
ALLURE_RESULTS_DIR=results-httpbin-org cargo run -- test --projects httpbin_org --reporters allure,list

rm -rf allure-report-merged   # Allure 3 has no --clean; leftover files break the output layout
allure generate "results-*"
```

`allure-history.jsonl` is gitignored. In CI, publish it with the report and download it before the
next run. [`publish-report.yml`](../.github/workflows/publish-report.yml) does this with GitHub Pages.
