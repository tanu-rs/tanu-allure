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

    // Create Allure reporter
    // Automatically loads:
    // - Preset values: os_platform, os_arch, tanu_allure_version
    // - TANU_ALLURE_* environment variables (e.g., TANU_ALLURE_BUILD_NUMBER=123)
    let reporter = tanu_allure::AllureReporter::with_results_dir("allure-results");

    app.install_reporter("allure", reporter);
    app.run(runner).await
}
