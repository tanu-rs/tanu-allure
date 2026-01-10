mod tests {
    #[tanu::test]
    async fn http_get() -> tanu::eyre::Result<()> {
        let http = tanu::http::Client::new();
        tanu::check!(http.get("https://httpbin.org/get").send().await.is_ok());
        Ok(())
    }
}

#[tanu::main]
#[tokio::main]
async fn main() -> tanu::eyre::Result<()> {
    let runner = run();
    let mut app = tanu::App::new();

    // Create Allure reporter with environment information
    let mut reporter = tanu_allure::AllureReporter::with_results_dir("allure-results");
    reporter.add_environment("os_platform", std::env::consts::OS);
    reporter.add_environment("os_arch", std::env::consts::ARCH);
    reporter.add_environment("tanu_allure_version", env!("CARGO_PKG_VERSION"));

    app.install_reporter("allure", reporter);
    app.run(runner).await
}
