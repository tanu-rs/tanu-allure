mod tests {
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;

    fn base_url() -> tanu::eyre::Result<String> {
        Ok(tanu::get_config().get_str("base_url")?.to_string())
    }

    /// Sends a GET request and expects a successful response.
    #[tanu::test]
    async fn http_get() -> tanu::eyre::Result<()> {
        let http = tanu::http::Client::new();
        let res = http.get(format!("{}/get", base_url()?)).send().await?;
        tanu::check!(res.status().is_success());
        Ok(())
    }

    /// Posts a JSON body; the request and response bodies show up as attachments.
    #[tanu::test]
    async fn http_post_json() -> tanu::eyre::Result<()> {
        let http = tanu::http::Client::new();
        let body = HashMap::from([("name", "tanu"), ("reporter", "allure")]);
        let res = http
            .post(format!("{}/post", base_url()?))
            .json(&body)
            .send()
            .await?;
        tanu::check!(res.status().is_success());
        Ok(())
    }

    /// Expects a 404. The HTTP step stays green because the check decides the outcome.
    #[tanu::test]
    async fn http_not_found() -> tanu::eyre::Result<()> {
        let http = tanu::http::Client::new();
        let res = http
            .get(format!("{}/status/404", base_url()?))
            .send()
            .await?;
        tanu::check_eq!(404, res.status().as_u16());
        Ok(())
    }

    /// Fails on its first attempt in each project and passes on the retry
    /// (`retry.count` in tanu.toml), so the report shows the attempt under Retries.
    #[tanu::test]
    async fn flaky_on_first_attempt() -> tanu::eyre::Result<()> {
        static ATTEMPTED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
        let project = tanu::get_config().name.clone();
        let first_attempt = ATTEMPTED
            .lock()
            .unwrap()
            .get_or_insert_with(HashSet::new)
            .insert(project);

        let http = tanu::http::Client::new();
        let res = http.get(format!("{}/get", base_url()?)).send().await?;
        tanu::check!(!first_attempt, "first attempt fails on purpose");
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
