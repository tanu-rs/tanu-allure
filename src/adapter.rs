use indexmap::IndexMap;
use serde_json;
use std::{fs, path::Path};
use tanu_core::{
    eyre, http,
    runner::{self, Test},
    ModuleName, ProjectName, Reporter, TestName,
};

use crate::models::{
    Label, Parameter, ParameterMode, Stage, Status, StatusDetails, Step, TestResult,
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
        AllureReporter {
            results_dir: "allure-results".to_string(),
            buffer: IndexMap::new(),
        }
    }

    pub fn with_results_dir(results_dir: impl Into<String>) -> Self {
        AllureReporter {
            results_dir: results_dir.into(),
            buffer: IndexMap::new(),
        }
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

        TestResult {
            uuid: uuid::Uuid::new_v4(),
            history_id: uuid::Uuid::new_v4().to_string(),
            test_case_id: Default::default(),
            name: test_name.to_string(),
            full_name: Default::default(),
            description: Default::default(),
            description_html: Default::default(),
            links: Default::default(),
            labels: vec![
                Label::ParentSuite(project.to_string()),
                Label::Suite(module.to_string()),
            ],
            parameters: vec![Parameter {
                name: "Project".to_string(),
                value: project.to_string(),
                excluded: Default::default(),
                mode: Default::default(),
            }],
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

        Ok(())
    }
}
