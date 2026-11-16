mod tests {
    #[tanu::test]
    async fn http_get() -> tanu::eyre::Result<()> {
        let base_url = tanu::get_config().get_str("base_url")?.to_string();
        let http = tanu::http::Client::new();
        let res = http.get(format!("{base_url}/get")).send().await?;
        tanu::check!(res.status().is_success());
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
    // The results directory can be overridden with ALLURE_RESULTS_DIR so that
    // separate runs (e.g. per environment) can be merged into one report later.
    let results_dir =
        std::env::var("ALLURE_RESULTS_DIR").unwrap_or_else(|_| "allure-results".to_string());
    let reporter = tanu_allure::AllureReporter::with_results_dir(results_dir);

    app.install_reporter("allure", reporter);
    app.run(runner).await
}
