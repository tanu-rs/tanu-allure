use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

/// Represents an Allure test result file.
#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    // Identifiers
    /// A unique identifier of the test result.
    pub uuid: Uuid,
    /// An identifier used by Allure Report. Two runs of the same test with the same set
    /// of parameters will always have the same `historyId`.
    pub history_id: String,
    /// An identifier used by Allure TestOps. Two runs of the same test will always have
    /// the same `testCaseId`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_case_id: Option<String>,

    // Metadata
    /// The title of the test or the name of the step.
    pub name: String,
    /// A unique identifier based on the file name and the test name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    /// The description of the test or step in Markdown format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The description of the test or step in HTML format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description_html: Option<String>,
    /// An array of links added to the test or step.
    #[serde(default)]
    pub links: Vec<Link>,
    /// An array of various labels added to the test or step.
    #[serde(default)]
    pub labels: Vec<Label>,
    /// An array of parameters added to the test or step.
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    /// An array of attachments added to the test or step.
    #[serde(default)]
    pub attachments: Vec<Attachment>,

    // Execution
    /// The status with which the test or step finished.
    pub status: Status,
    /// Detailed information about the test status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_details: Option<StatusDetails>,
    /// The stage in the lifecycle of the test or step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<Stage>,
    /// The time when the execution of the test or step started, in UNIX timestamp format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    /// The time when the execution of the test or step finished, in UNIX timestamp format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<i64>,
    /// An array of test steps.
    #[serde(default)]
    pub steps: Vec<Step>,
}

/// Represents a link in an Allure test result.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    /// The type of the link, e.g., "issue" or "tms".
    pub r#type: String,
    /// The link's name that will be displayed in the test report.
    pub name: String,
    /// The full URL of the link.
    pub url: url::Url,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Labels {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub story: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_suite: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suite: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_suite: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,

    #[serde(flatten)]
    pub labels: HashMap<String, String>,
}

/// Represents a label in an Allure test result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", content = "value")]
#[serde(rename_all = "camelCase")]
pub enum Label {
    Tag(String),
    Severity(String),
    Owner(String),
    Epic(String),
    Feature(String),
    Story(String),
    ParentSuite(String),
    Suite(String),
    SubSuite(String),
    Package(String),
    #[serde(untagged)]
    Custom {
        name: String,
        value: String,
    },
}

impl Label {
    /// Creates a custom label with the given name and value
    pub fn custom(name: impl Into<String>, value: impl Into<String>) -> Self {
        Label::Custom {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// Represents a parameter in an Allure test result.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameter {
    /// The name of the parameter.
    pub name: String,
    /// The value of the parameter.
    pub value: String,
    /// If true, Allure will not use the parameter when comparing the
    /// current result with the previous one in the history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded: Option<bool>,
    /// How the parameter will be shown in the report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<ParameterMode>,
}

/// Represents parameter display mode in Allure report.
#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ParameterMode {
    /// The parameter and its value will be shown in a table along with other parameters.
    #[default]
    Default,
    /// The parameter will be shown in the table, but its value will be hidden.
    Masked,
    /// The parameter and its value will not be shown in the test report.
    Hidden,
}

/// Represents an attachment in an Allure test result.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// The human-readable name of the attachment.
    pub name: String,
    /// The name of the file with the attachment's content.
    pub source: String,
    /// The media type of the content.
    pub r#type: String,
}

/// Represents the status of a test or step.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Failed,
    Broken,
    Passed,
    Skipped,
    #[default]
    Unknown,
}

/// Represents detailed information about the test status.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusDetails {
    /// Indicates that the test fails because of a known bug.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub known: Option<bool>,
    /// Indicates that the result must not affect the statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    /// Indicates that this test or step is known to be unstable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flaky: Option<bool>,
    /// The short text message to display in the test details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The full stack trace to display in the test details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<String>,
}

/// Represents the stage in the lifecycle of a test or step.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Scheduled,
    Running,
    Finished,
    Pending,
    Interrupted,
}

/// Represents a test step in an Allure test result.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// The name of the step.
    pub name: String,
    /// An array of parameters added to the step.
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    /// An array of attachments added to the step.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// The status with which the step finished.
    pub status: Status,
    /// Detailed information about the step status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_details: Option<StatusDetails>,
    /// The stage in the lifecycle of the step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<Stage>,
    /// The time when the execution of the step started, in UNIX timestamp format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    /// The time when the execution of the step finished, in UNIX timestamp format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<i64>,
    /// An array of sub-steps within this step.
    #[serde(default)]
    pub steps: Vec<Step>,
}

// ============================================================================
// History types for tracking test execution history across runs
// ============================================================================

/// Statistics for a test's execution history
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryStatistic {
    pub failed: u32,
    pub broken: u32,
    pub skipped: u32,
    pub passed: u32,
    pub unknown: u32,
    pub total: u32,
}

impl HistoryStatistic {
    /// Updates statistics based on a test status
    pub fn record(&mut self, status: &Status) {
        match status {
            Status::Failed => self.failed += 1,
            Status::Broken => self.broken += 1,
            Status::Skipped => self.skipped += 1,
            Status::Passed => self.passed += 1,
            Status::Unknown => self.unknown += 1,
        }
        self.total += 1;
    }
}

/// Timing information for a history item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryTime {
    pub start: i64,
    pub stop: i64,
    pub duration: i64,
}

/// A single run entry in the history
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub uid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_url: Option<String>,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_details: Option<String>,
    pub time: HistoryTime,
}

/// History entry for a single test (identified by history_id)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryEntry {
    pub statistic: HistoryStatistic,
    pub items: Vec<HistoryItem>,
}

/// Complete history.json structure (key = history_id)
pub type History = HashMap<String, HistoryEntry>;

/// Maximum number of history items to keep per test
pub const MAX_HISTORY_ITEMS: usize = 20;

/// Generates a deterministic history_id from test identity.
///
/// The history_id is a SHA-256 hash of:
/// - project name
/// - module name
/// - test name
/// - non-excluded parameter values (sorted by name for consistency)
pub fn generate_history_id(
    project: &str,
    module: &str,
    test_name: &str,
    parameters: &[Parameter],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{project}::{module}::{test_name}"));

    // Include non-excluded parameters (sorted for determinism)
    let mut params: Vec<_> = parameters
        .iter()
        .filter(|p| p.excluded != Some(true))
        .map(|p| (&p.name, &p.value))
        .collect();
    params.sort_by_key(|(name, _)| *name);

    for (name, value) in params {
        hasher.update(format!("::{name}={value}"));
    }

    format!("{:x}", hasher.finalize())
}

impl TestResult {
    /// Creates a new TestResult with a random UUID v4.
    pub fn new(name: String) -> Self {
        TestResult {
            uuid: Uuid::new_v4(),
            history_id: String::new(), // This should be set based on test parameters
            test_case_id: None,
            name,
            full_name: None,
            description: None,
            description_html: None,
            links: Default::default(),
            labels: Default::default(),
            parameters: Default::default(),
            attachments: Default::default(),
            status: Status::Unknown,
            status_details: None,
            stage: None,
            start: None,
            stop: None,
            steps: Default::default(),
        }
    }

    /// Sets the start time to the current time
    pub fn start(&mut self) {
        self.start = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
        );
        self.stage = Some(Stage::Running);
    }

    /// Sets the stop time to the current time
    pub fn stop(&mut self) {
        self.stop = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
        );
        self.stage = Some(Stage::Finished);
    }

    /// Generates a history_id from test name and parameters
    pub fn set_history_id(&mut self) {
        // Simple implementation - in real code you might want to hash name + parameters
        self.history_id = format!("{}-history", self.name);
    }
}
