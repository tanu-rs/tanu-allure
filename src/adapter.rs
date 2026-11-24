use indexmap::IndexMap;
use serde_json;
use std::{collections::HashMap, fs, path::Path, time::SystemTime};
use tanu_core::{
    eyre, http,
    runner::{self, Test},
    ModuleName, ProjectName, Reporter, TestName,
};

use crate::models::{
    generate_history_id, Attachment, Category, Executor, History, HistoryItem, HistoryTime, Label,
    Parameter, ParameterMode, Stage, Status, StatusDetails, Step, TestResult, MAX_HISTORY_ITEMS,
};

fn to_test_status(test: &Test) -> Status {
    match &test.result {
        Ok(_) => Status::Passed,
        Err(runner::Error::ErrorReturned(_)) => Status::Failed,
        Err(runner::Error::Panicked(_)) => Status::Broken,
    }
}

fn system_time_to_unix_millis(time: SystemTime) -> i64 {
    time.duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn push_header_parameters(
    parameters: &mut Vec<Parameter>,
    prefix: &str,
    headers: &http::header::HeaderMap,
) {
    for (name, value) in headers.iter() {
        let header_name = name.as_str();
        let is_sensitive = matches!(
            header_name,
            "authorization"
                | "proxy-authorization"
                | "cookie"
                | "set-cookie"
                | "x-api-key"
                | "x-auth-token"
        );

        let (value, mode) = if is_sensitive {
            ("<masked>".to_string(), Some(ParameterMode::Masked))
        } else {
            (String::from_utf8_lossy(value.as_bytes()).into_owned(), None)
        };

        parameters.push(Parameter {
            name: format!("{prefix}.{header_name}"),
            value,
            excluded: None,
            mode,
        });
    }
}

/// Step name for an HTTP call, e.g. `GET /get?x=1 → 200 OK`.
fn http_step_name(log: &http::Log) -> String {
    let url = &log.request.url;
    let query = url.query().map(|q| format!("?{q}")).unwrap_or_default();
    format!(
        "{} {}{} → {}",
        log.request.method,
        url.path(),
        query,
        log.response.status
    )
}

/// Picks the attachment media type and file extension from a `Content-Type` header.
fn media_type(headers: &http::header::HeaderMap) -> (&'static str, &'static str) {
    let content_type = headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.contains("json") {
        ("application/json", "json")
    } else if content_type.starts_with("text/html") {
        ("text/html", "html")
    } else {
        ("text/plain", "txt")
    }
}

/// Default categories for tanu failures, written to `categories.json`.
///
/// Patterns are wrapped in `[\s\S]*` so they match both Allure 2 (full match)
/// and Allure 3 (partial match) on multi-line messages.
fn default_categories() -> Vec<Category> {
    vec![
        Category {
            name: "Assertion failures".to_string(),
            matched_statuses: vec![Status::Failed],
            message_regex: Some(r"[\s\S]*check failed[\s\S]*".to_string()),
        },
        Category {
            name: "Request errors".to_string(),
            matched_statuses: vec![Status::Failed],
            message_regex: Some(
                r"[\s\S]*(HttpError|TlsError|Request timed out|gRPC transport error)[\s\S]*"
                    .to_string(),
            ),
        },
        Category {
            name: "Panics".to_string(),
            matched_statuses: vec![Status::Broken],
            message_regex: Some(r"[\s\S]*panic[\s\S]*".to_string()),
        },
    ]
}

/// Builds `executor.json` contents from GitHub Actions environment variables.
fn github_executor(env: impl Fn(&str) -> Option<String>) -> Option<Executor> {
    if env("GITHUB_ACTIONS").as_deref() != Some("true") {
        return None;
    }
    let run_number = env("GITHUB_RUN_NUMBER");
    let build_url = match (
        env("GITHUB_SERVER_URL"),
        env("GITHUB_REPOSITORY"),
        env("GITHUB_RUN_ID"),
    ) {
        (Some(server), Some(repo), Some(run_id)) => {
            Some(format!("{server}/{repo}/actions/runs/{run_id}"))
        }
        _ => None,
    };
    Some(Executor {
        name: "GitHub Actions".to_string(),
        r#type: "github".to_string(),
        build_name: run_number.as_ref().map(|n| format!("#{n}")),
        build_order: run_number.as_ref().and_then(|n| n.parse().ok()),
        build_url,
        report_name: None,
        report_url: None,
    })
}

pub struct AllureReporter {
    pub results_dir: String,
    buffer: IndexMap<(ProjectName, ModuleName, TestName), Buffer>,
    history: History,
    current_run_results: Vec<RunResult>,
    environment: HashMap<String, String>,
    hostname: String,
}

/// Tracks a single test result for history update
struct RunResult {
    history_id: String,
    status: Status,
    status_details: Option<String>,
    start: i64,
    stop: i64,
    uuid: String,
}

enum Event {
    Check {
        at: SystemTime,
        check: Box<runner::Check>,
    },
    Http(Box<http::Log>),
}

#[derive(Default)]
struct Buffer {
    events: Vec<Event>,
}

impl Default for AllureReporter {
    fn default() -> Self {
        AllureReporter::new()
    }
}

impl AllureReporter {
    pub fn new() -> Self {
        Self::with_results_dir("allure-results")
    }

    pub fn with_results_dir(results_dir: impl Into<String>) -> Self {
        let results_dir = results_dir.into();
        let history = Self::load_history(&results_dir);
        let environment = Self::initialize_environment();
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".to_string());

        AllureReporter {
            results_dir,
            buffer: IndexMap::new(),
            history,
            current_run_results: Vec::new(),
            environment,
            hostname,
        }
    }

    /// Initializes environment variables by loading preset values and TANU_ALLURE_* variables
    fn initialize_environment() -> HashMap<String, String> {
        let mut environment = Self::load_default_environment();
        Self::load_env_with_prefix(&mut environment, "TANU_ALLURE_");
        environment
    }

    /// Loads preset environment values (os_platform, os_arch, tanu_allure_version)
    fn load_default_environment() -> HashMap<String, String> {
        let mut environment = HashMap::new();
        environment.insert("os_platform".to_string(), std::env::consts::OS.to_string());
        environment.insert("os_arch".to_string(), std::env::consts::ARCH.to_string());
        environment.insert(
            "tanu_allure_version".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        );
        environment
    }

    /// Loads environment variables with a specific prefix into the provided HashMap
    fn load_env_with_prefix(environment: &mut HashMap<String, String>, prefix: &str) {
        for (key, value) in std::env::vars() {
            if let Some(stripped_key) = key.strip_prefix(prefix) {
                environment.insert(stripped_key.to_string(), value);
            }
        }
    }

    /// Adds a single environment variable to be included in the environment.properties file.
    ///
    /// Note: The reporter automatically loads preset values (os_platform, os_arch, tanu_allure_version)
    /// and TANU_ALLURE_* environment variables. Use this method to add additional custom values.
    pub fn add_environment(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.environment.insert(key.into(), value.into());
    }

    /// Sets multiple environment variables at once.
    ///
    /// Note: The reporter automatically loads preset values and TANU_ALLURE_* environment variables.
    /// Use this method to add additional custom values.
    pub fn set_environment(&mut self, env: HashMap<String, String>) {
        self.environment.extend(env);
    }

    /// Loads environment variables from system environment with a specific prefix.
    /// Variables with the prefix will be added with the prefix stripped.
    ///
    /// Note: The reporter automatically loads TANU_ALLURE_* variables on initialization.
    /// Use this method only if you need to load additional prefixed variables.
    ///
    /// # Example
    ///
    /// If `MY_APP_VERSION=1.0.0` is set in the environment and you call
    /// `load_from_env("MY_APP_")`, it will add `VERSION = 1.0.0` to the
    /// environment.properties file.
    pub fn load_from_env(&mut self, prefix: &str) {
        Self::load_env_with_prefix(&mut self.environment, prefix);
    }

    /// Loads existing history.json from the history subdirectory
    fn load_history(results_dir: &str) -> History {
        let path = Path::new(results_dir).join("history").join("history.json");
        if !path.exists() {
            return History::new();
        }
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn ensure_results_dir(&self) -> eyre::Result<()> {
        let path = Path::new(&self.results_dir);
        if !path.exists() {
            fs::create_dir_all(path)?;
        }
        Ok(())
    }

    /// Writes an HTTP body as an attachment file and returns its reference.
    fn write_attachment(
        &self,
        name: &str,
        body: &str,
        headers: &http::header::HeaderMap,
    ) -> eyre::Result<Attachment> {
        let (media_type, ext) = media_type(headers);
        let content = if media_type == "application/json" {
            serde_json::from_str::<serde_json::Value>(body)
                .and_then(|v| serde_json::to_string_pretty(&v))
                .unwrap_or_else(|_| body.to_string())
        } else {
            body.to_string()
        };
        let source = format!("{}-attachment.{ext}", uuid::Uuid::new_v4());
        fs::write(Path::new(&self.results_dir).join(&source), content)?;
        Ok(Attachment {
            name: name.to_string(),
            source,
            r#type: media_type.to_string(),
        })
    }

    fn to_step(&self, event: &Event) -> eyre::Result<Step> {
        Ok(match event {
            Event::Check { at, check } => {
                let at = system_time_to_unix_millis(*at);
                // tanu's expression already reads "check failed: ..." / "check succeeded: ...".
                // `check_eq!` appends a multi-line diff, so the step is named after the
                // first line and the full text goes into the step's message.
                let expr = strip_ansi_escapes::strip_str(&check.expr);
                let name = expr.lines().next().unwrap_or_default().trim().to_string();
                let multiline = expr.trim().contains('\n');
                let status_details = (!check.result || multiline).then(|| StatusDetails {
                    known: None,
                    muted: None,
                    flaky: None,
                    message: Some(expr.trim().to_string()),
                    trace: None,
                });
                let status = if check.result {
                    Status::Passed
                } else {
                    Status::Failed
                };
                Step {
                    name,
                    parameters: Default::default(),
                    attachments: Default::default(),
                    status,
                    status_details,
                    stage: Some(Stage::Finished),
                    start: Some(at),
                    stop: Some(at),
                    steps: vec![],
                }
            }
            Event::Http(log) => {
                let mut parameters = vec![Parameter {
                    name: "url".to_string(),
                    value: log.request.url.to_string(),
                    excluded: None,
                    mode: None,
                }];
                push_header_parameters(&mut parameters, "request.header", &log.request.headers);
                push_header_parameters(&mut parameters, "response.header", &log.response.headers);

                let mut attachments = Vec::new();
                if let Some(body) = log.request.body.as_deref().filter(|b| !b.is_empty()) {
                    attachments.push(self.write_attachment(
                        "Request body",
                        body,
                        &log.request.headers,
                    )?);
                }
                if !log.response.body.is_empty() {
                    attachments.push(self.write_attachment(
                        "Response body",
                        &log.response.body,
                        &log.response.headers,
                    )?);
                }

                Step {
                    name: http_step_name(log),
                    parameters,
                    attachments,
                    // A response was received; whether its status is expected is up to the
                    // test's checks, so the call itself is never marked as failed.
                    status: Status::Passed,
                    status_details: Default::default(),
                    stage: Some(Stage::Finished),
                    start: Some(system_time_to_unix_millis(log.started_at)),
                    stop: Some(system_time_to_unix_millis(log.ended_at)),
                    steps: vec![],
                }
            }
        })
    }

    fn map_to_allure_test_result(
        &self,
        project: &str,
        module: &str,
        test_name: &str,
        events: &[Event],
        test: &Test,
    ) -> eyre::Result<TestResult> {
        let status = to_test_status(test);

        let status_details = if let Err(e) = &test.result {
            Some(StatusDetails {
                known: None,
                muted: None,
                flaky: None,
                message: Some(strip_ansi_escapes::strip_str(e.to_string())),
                trace: None,
            })
        } else {
            None
        };

        let steps = events
            .iter()
            .map(|event| self.to_step(event))
            .collect::<eyre::Result<Vec<_>>>()?;

        // Create parameters first so we can use them for history_id generation
        let parameters = vec![Parameter {
            name: "Project".to_string(),
            value: project.to_string(),
            excluded: Some(true), // Exclude from history_id calculation
            mode: Default::default(),
        }];

        // Generate deterministic history_id based on test identity
        let history_id = generate_history_id(project, module, test_name, &parameters);

        let description = test
            .info
            .doc
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(str::to_string);

        Ok(TestResult {
            uuid: uuid::Uuid::new_v4(),
            history_id,
            test_case_id: Default::default(),
            name: test_name.to_string(),
            full_name: Some(format!("{project}::{module}::{test_name}")),
            description,
            description_html: Default::default(),
            links: Default::default(),
            labels: vec![
                Label::ParentSuite(project.to_string()),
                Label::Suite(module.to_string()),
                Label::Package(module.to_string()),
                Label::custom("language", "rust"),
                Label::custom("framework", "tanu"),
                Label::Host(self.hostname.clone()),
                Label::Thread(test.worker_id.to_string()),
            ],
            parameters,
            attachments: Default::default(),
            status,
            status_details,
            stage: Some(Stage::Finished),
            start: Some(system_time_to_unix_millis(test.started_at)),
            stop: Some(system_time_to_unix_millis(test.ended_at)),
            steps,
        })
    }

    /// Converts the buffered events of one test attempt into a result file.
    fn write_test_result(
        &mut self,
        project: String,
        module: String,
        test_name: String,
        test: &Test,
    ) -> eyre::Result<TestResult> {
        self.ensure_results_dir()?;

        // Taking the buffer ensures each attempt only contains its own steps and frees memory.
        let events = self
            .buffer
            .shift_remove(&(project.clone(), module.clone(), test_name.clone()))
            .unwrap_or_default()
            .events;

        let test_result =
            self.map_to_allure_test_result(&project, &module, &test_name, &events, test)?;

        let file_name = format!("{}-result.json", test_result.uuid);
        let file_path = Path::new(&self.results_dir).join(file_name);
        fs::write(file_path, serde_json::to_string_pretty(&test_result)?)?;

        Ok(test_result)
    }
}

#[async_trait::async_trait]
impl Reporter for AllureReporter {
    async fn on_check(
        &mut self,
        project_name: String,
        module_name: String,
        test_name: String,
        check: Box<runner::Check>,
    ) -> eyre::Result<()> {
        self.buffer
            .entry((project_name, module_name, test_name))
            .or_default()
            .events
            .push(Event::Check {
                at: SystemTime::now(),
                check,
            });
        Ok(())
    }

    async fn on_call(
        &mut self,
        project_name: String,
        module_name: String,
        test_name: String,
        log: runner::CallLog,
    ) -> eyre::Result<()> {
        if let runner::CallLog::Http(http_log) = log {
            self.buffer
                .entry((project_name, module_name, test_name))
                .or_default()
                .events
                .push(Event::Http(http_log));
        }
        Ok(())
    }

    /// Writes the failed attempt as its own result. Allure groups results that share a
    /// `historyId` and shows the earlier attempts under "Retries".
    async fn on_retry(
        &mut self,
        project: String,
        module: String,
        test_name: String,
        test: Test,
    ) -> eyre::Result<()> {
        self.write_test_result(project, module, test_name, &test)?;
        Ok(())
    }

    async fn on_end(
        &mut self,
        project: String,
        module: String,
        test_name: String,
        test: Test,
    ) -> eyre::Result<()> {
        let test_result = self.write_test_result(project, module, test_name, &test)?;

        // Track result for history update (final attempt only)
        self.current_run_results.push(RunResult {
            history_id: test_result.history_id.clone(),
            status: test_result.status.clone(),
            status_details: test_result
                .status_details
                .as_ref()
                .and_then(|d| d.message.clone()),
            start: test_result.start.unwrap_or(0),
            stop: test_result.stop.unwrap_or(0),
            uuid: test_result.uuid.to_string(),
        });

        Ok(())
    }

    async fn on_summary(&mut self, _summary: runner::TestSummary) -> eyre::Result<()> {
        self.write_history()?;
        self.write_environment()?;
        self.write_categories()?;
        self.write_executor()?;
        Ok(())
    }
}

impl AllureReporter {
    /// Writes updated history.json after all tests complete
    fn write_history(&mut self) -> eyre::Result<()> {
        for result in &self.current_run_results {
            let entry = self.history.entry(result.history_id.clone()).or_default();

            // Update statistics
            entry.statistic.record(&result.status);

            // Add new history item at the beginning
            entry.items.insert(
                0,
                HistoryItem {
                    uid: result.uuid.clone(),
                    report_url: None,
                    status: result.status.clone(),
                    status_details: result.status_details.clone(),
                    time: HistoryTime {
                        start: result.start,
                        stop: result.stop,
                        duration: (result.stop - result.start) / 1000,
                    },
                },
            );

            // Trim to max items
            entry.items.truncate(MAX_HISTORY_ITEMS);
        }

        // Ensure history directory exists
        let history_dir = Path::new(&self.results_dir).join("history");
        fs::create_dir_all(&history_dir)?;

        // Write history.json
        let json = serde_json::to_string_pretty(&self.history)?;
        fs::write(history_dir.join("history.json"), json)?;

        Ok(())
    }

    /// Writes categories.json unless the user already provided one in the results directory.
    fn write_categories(&self) -> eyre::Result<()> {
        self.ensure_results_dir()?;
        let path = Path::new(&self.results_dir).join("categories.json");
        if path.exists() {
            return Ok(());
        }
        fs::write(path, serde_json::to_string_pretty(&default_categories())?)?;
        Ok(())
    }

    /// Writes executor.json when running in GitHub Actions so the report links to the CI run.
    fn write_executor(&self) -> eyre::Result<()> {
        let Some(executor) = github_executor(|key| std::env::var(key).ok()) else {
            return Ok(());
        };
        self.ensure_results_dir()?;
        let path = Path::new(&self.results_dir).join("executor.json");
        fs::write(path, serde_json::to_string_pretty(&executor)?)?;
        Ok(())
    }

    /// Writes environment.properties file with environment variables
    fn write_environment(&self) -> eyre::Result<()> {
        if self.environment.is_empty() {
            return Ok(());
        }

        // Ensure results directory exists
        self.ensure_results_dir()?;

        // Build properties file content
        let mut lines: Vec<String> = self
            .environment
            .iter()
            .map(|(key, value)| {
                // Escape special characters for Java properties format
                let escaped_key = key
                    .replace('\\', "\\\\")
                    .replace('=', "\\=")
                    .replace(':', "\\:");
                let escaped_value = value
                    .replace('\\', "\\\\")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r");
                format!("{} = {}", escaped_key, escaped_value)
            })
            .collect();

        // Sort for deterministic output
        lines.sort();

        // Write environment.properties file
        let file_path = Path::new(&self.results_dir).join("environment.properties");
        fs::write(file_path, lines.join("\n"))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tanu_core::runner::TestInfo;

    fn temp_results_dir() -> String {
        let dir = std::env::temp_dir().join(format!("tanu-allure-test-{}", uuid::Uuid::new_v4()));
        dir.to_string_lossy().into_owned()
    }

    fn test(result: Result<(), runner::Error>) -> Test {
        Test {
            info: Arc::new(TestInfo {
                module: "tests".to_string(),
                name: "t".to_string(),
                doc: Some("  Checks the endpoint.  ".to_string()),
                ..Default::default()
            }),
            worker_id: 0,
            started_at: SystemTime::now(),
            ended_at: SystemTime::now(),
            request_time: Default::default(),
            result,
        }
    }

    fn http_log(url: &str, status: u16, body: &str) -> Box<http::Log> {
        let mut headers = http::header::HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            "application/json".parse().unwrap(),
        );
        Box::new(http::Log {
            request: http::LogRequest {
                url: url.parse().unwrap(),
                method: http::Method::POST,
                headers: headers.clone(),
                body: Some(r#"{"a":1}"#.to_string()),
            },
            response: http::LogResponse {
                headers,
                body: body.to_string(),
                status: http::StatusCode::from_u16(status).unwrap(),
                duration_req: Default::default(),
            },
            started_at: SystemTime::now(),
            ended_at: SystemTime::now(),
        })
    }

    fn read_results(dir: &str) -> Vec<TestResult> {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| {
                let path = e.unwrap().path();
                path.to_string_lossy()
                    .ends_with("-result.json")
                    .then(|| serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap())
            })
            .collect()
    }

    fn key() -> (String, String, String) {
        ("p".to_string(), "tests".to_string(), "t".to_string())
    }

    #[tokio::test]
    async fn http_step_has_readable_name_and_body_attachments() {
        let dir = temp_results_dir();
        let mut reporter = AllureReporter::with_results_dir(&dir);
        let (p, m, t) = key();
        reporter
            .on_call(
                p.clone(),
                m.clone(),
                t.clone(),
                runner::CallLog::Http(http_log("http://x/status/404?q=1", 404, r#"{"b":2}"#)),
            )
            .await
            .unwrap();
        reporter.on_end(p, m, t, test(Ok(()))).await.unwrap();

        let results = read_results(&dir);
        let step = &results[0].steps[0];
        assert_eq!(step.name, "POST /status/404?q=1 → 404 Not Found");
        assert_eq!(step.status, Status::Passed);
        assert_eq!(step.parameters[0].value, "http://x/status/404?q=1");
        let names: Vec<_> = step.attachments.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Request body", "Response body"]);
        let response = &step.attachments[1];
        assert_eq!(response.r#type, "application/json");
        let content = fs::read_to_string(Path::new(&dir).join(&response.source)).unwrap();
        assert_eq!(content, "{\n  \"b\": 2\n}");
        assert_eq!(
            results[0].description.as_deref(),
            Some("Checks the endpoint.")
        );
        assert!(results[0]
            .labels
            .contains(&Label::custom("framework", "tanu")));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn retries_are_written_as_separate_results() {
        let dir = temp_results_dir();
        let mut reporter = AllureReporter::with_results_dir(&dir);
        let (p, m, t) = key();
        reporter
            .on_check(
                p.clone(),
                m.clone(),
                t.clone(),
                Box::new(runner::Check::error(
                    "check failed: `(left == right)`\n\nDiff < left / right > :\n<1\n>2\n",
                )),
            )
            .await
            .unwrap();
        reporter
            .on_retry(
                p.clone(),
                m.clone(),
                t.clone(),
                test(Err(runner::Error::ErrorReturned(
                    "check failed: a == b".into(),
                ))),
            )
            .await
            .unwrap();
        reporter
            .on_check(
                p.clone(),
                m.clone(),
                t.clone(),
                Box::new(runner::Check::success("a == a")),
            )
            .await
            .unwrap();
        reporter.on_end(p, m, t, test(Ok(()))).await.unwrap();

        let mut results = read_results(&dir);
        results.sort_by_key(|r| r.status != Status::Failed);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].history_id, results[1].history_id);
        assert_eq!(results[0].steps.len(), 1);
        assert_eq!(results[1].steps.len(), 1);
        let failed_step = &results[0].steps[0];
        assert_eq!(failed_step.status, Status::Failed);
        assert_eq!(failed_step.name, "check failed: `(left == right)`");
        let message = failed_step.status_details.as_ref().unwrap().message.clone();
        assert_eq!(
            message.as_deref(),
            Some("check failed: `(left == right)`\n\nDiff < left / right > :\n<1\n>2")
        );
        assert!(results[1].steps[0].status_details.is_none());
        assert_eq!(reporter.current_run_results.len(), 1);
        assert!(reporter.buffer.is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn test_without_events_is_reported() {
        let dir = temp_results_dir();
        let mut reporter = AllureReporter::with_results_dir(&dir);
        let (p, m, t) = key();
        reporter.on_end(p, m, t, test(Ok(()))).await.unwrap();
        assert_eq!(read_results(&dir).len(), 1);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn categories_and_executor() {
        let dir = temp_results_dir();
        let reporter = AllureReporter::with_results_dir(&dir);
        reporter.write_categories().unwrap();
        let categories: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(Path::new(&dir).join("categories.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(categories[0]["name"], "Assertion failures");
        assert_eq!(categories[0]["matchedStatuses"][0], "failed");
        fs::remove_dir_all(dir).ok();

        assert!(github_executor(|_| None).is_none());
        let env: HashMap<&str, &str> = [
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_RUN_NUMBER", "42"),
            ("GITHUB_SERVER_URL", "https://github.com"),
            ("GITHUB_REPOSITORY", "o/r"),
            ("GITHUB_RUN_ID", "7"),
        ]
        .into_iter()
        .collect();
        let executor = github_executor(|k| env.get(k).map(|v| v.to_string())).unwrap();
        assert_eq!(executor.build_name.as_deref(), Some("#42"));
        assert_eq!(executor.build_order, Some(42));
        assert_eq!(
            executor.build_url.as_deref(),
            Some("https://github.com/o/r/actions/runs/7")
        );
    }
}
