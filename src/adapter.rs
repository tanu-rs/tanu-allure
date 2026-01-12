use indexmap::IndexMap;
use serde_json;
use std::{collections::HashMap, fs, path::Path};
use tanu_core::{
    eyre, http,
    runner::{self, Test},
    ModuleName, ProjectName, Reporter, TestName,
};

use crate::models::{
    generate_history_id, History, HistoryItem, HistoryTime, Label, Parameter, ParameterMode,
    Stage, Status, StatusDetails, Step, TestResult, MAX_HISTORY_ITEMS,
};

fn to_status(status: http::StatusCode) -> Status {
    if status.is_success() {
        Status::Passed
    } else if status.is_client_error() || status.is_server_error() {
        Status::Failed
    } else {
        Status::Broken
    }
}

fn to_test_status(test: &Test) -> Status {
    match &test.result {
        Ok(_) => Status::Passed,
        Err(runner::Error::ErrorReturned(_)) => Status::Failed,
        Err(runner::Error::Panicked(_)) => Status::Broken,
    }
}

fn system_time_to_unix_millis(time: std::time::SystemTime) -> i64 {
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

pub struct AllureReporter {
    pub results_dir: String,
    buffer: IndexMap<(ProjectName, ModuleName, TestName), Buffer>,
    history: History,
    current_run_results: Vec<RunResult>,
    environment: HashMap<String, String>,
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
    Check(Box<runner::Check>),
    Http(Box<http::Log>),
}

impl From<&Event> for Step {
    fn from(event: &Event) -> Self {
        match event {
            Event::Check(check) => {
                let now = system_time_to_unix_millis(std::time::SystemTime::now());
                Step {
                    name: strip_ansi_escapes::strip_str(&check.expr),
                    parameters: Default::default(),
                    attachments: Default::default(),
                    status: if check.result {
                        Status::Passed
                    } else {
                        Status::Failed
                    },
                    status_details: Default::default(),
                    stage: Some(Stage::Finished),
                    start: Some(now),
                    stop: Some(now),
                    steps: vec![],
                }
            },
            Event::Http(log) => Step {
                name: log.request.url.to_string(),
                parameters: {
                    let mut parameters = Vec::new();
                    push_header_parameters(&mut parameters, "request.header", &log.request.headers);
                    push_header_parameters(
                        &mut parameters,
                        "response.header",
                        &log.response.headers,
                    );
                    parameters
                },
                attachments: Default::default(),
                status: to_status(log.response.status),
                status_details: Default::default(),
                stage: Some(Stage::Finished),
                start: Some(system_time_to_unix_millis(log.started_at)),
                stop: Some(system_time_to_unix_millis(log.ended_at)),
                steps: vec![],
            },
        }
    }
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

        AllureReporter {
            results_dir,
            buffer: IndexMap::new(),
            history,
            current_run_results: Vec::new(),
            environment,
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

    fn map_to_allure_test_result(
        &self,
        project: &str,
        module: &str,
        test_name: &str,
        events: &[Event],
        test: &Test,
    ) -> TestResult {
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

        let steps: Vec<_> = events.iter().map(Step::from).collect();

        // Create parameters first so we can use them for history_id generation
        let parameters = vec![Parameter {
            name: "Project".to_string(),
            value: project.to_string(),
            excluded: Some(true), // Exclude from history_id calculation
            mode: Default::default(),
        }];

        // Generate deterministic history_id based on test identity
        let history_id = generate_history_id(project, module, test_name, &parameters);

        TestResult {
            uuid: uuid::Uuid::new_v4(),
            history_id,
            test_case_id: Default::default(),
            name: test_name.to_string(),
            full_name: Some(format!("{module}::{test_name}")),
            description: Default::default(),
            description_html: Default::default(),
            links: Default::default(),
            labels: vec![
                Label::ParentSuite(project.to_string()),
                Label::Suite(module.to_string()),
            ],
            parameters,
            attachments: Default::default(),
            status,
            status_details,
            stage: Some(Stage::Finished),
            start: Some(system_time_to_unix_millis(test.started_at)),
            stop: Some(system_time_to_unix_millis(test.ended_at)),
            steps,
        }
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
            .push(Event::Check(check));
        Ok(())
    }

    async fn on_http_call(
        &mut self,
        project_name: String,
        module_name: String,
        test_name: String,
        log: Box<http::Log>,
    ) -> eyre::Result<()> {
        self.buffer
            .entry((project_name, module_name, test_name))
            .or_default()
            .events
            .push(Event::Http(log));
        Ok(())
    }

    async fn on_end(
        &mut self,
        project: String,
        module: String,
        test_name: String,
        test: Test,
    ) -> eyre::Result<()> {
        self.ensure_results_dir()?;

        let buffer = self
            .buffer
            .get(&(project.clone(), module.clone(), test_name.clone()))
            .ok_or_else(|| eyre::eyre!("test case \"{test_name}\" not found in the buffer"))?;

        let test_result =
            self.map_to_allure_test_result(&project, &module, &test_name, &buffer.events, &test);

        let file_name = format!("{}-result.json", test_result.uuid);
        let file_path = Path::new(&self.results_dir).join(file_name);

        let json = serde_json::to_string_pretty(&test_result)?;

        fs::write(file_path, json)?;

        // Track result for history update
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
                let escaped_key = key.replace('\\', "\\\\").replace('=', "\\=").replace(':', "\\:");
                let escaped_value = value.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r");
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
