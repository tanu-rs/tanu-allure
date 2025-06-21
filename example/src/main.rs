use tanu::assert;

#[tanu::test]
async fn foo() -> tanu::eyre::Result<()> {
    let http = tanu::http::Client::new();
    assert!(http.get("https://httpbin.org/get").send().await.is_ok());
    Ok(())
}

#[tanu::main]
#[tokio::main]
async fn main() -> tanu::eyre::Result<()> {
    let runner = run();
    let mut app = tanu::App::new();
    app.install_reporter(
        "allure",
        tanu_allure::AllureReporter::with_results_dir("allure-results"),
    );
    app.run(runner).await?;

    Ok(())
}
