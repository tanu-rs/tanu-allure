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
