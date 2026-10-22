//! Isolated metrics capture: each poll installs its recorder only for that poll.

#![allow(clippy::expect_used)]

use std::future::{Future, poll_fn};
use std::pin::pin;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use metrics_exporter_prometheus::PrometheusBuilder;
use notegate_db::NewCommandInvocation;
use notegate_model::{Caller, CallerIdentity, Channel};
use serde_json::Value;
use uuid::Uuid;

use super::{InvocationWriteFuture, InvocationWriter};

#[derive(Debug, Clone)]
pub(crate) struct RecordedInvocation {
    pub(crate) invocation_id: Option<Uuid>,
    pub(crate) owner_user_id: Uuid,
    pub(crate) actor_account_id: Uuid,
    pub(crate) caller_kind: &'static str,
    pub(crate) surface: &'static str,
    pub(crate) tool: String,
    pub(crate) op: Option<String>,
    pub(crate) purpose: Option<String>,
    pub(crate) space_name: Option<String>,
    pub(crate) input: Value,
    pub(crate) response: Option<Value>,
    pub(crate) outcome: &'static str,
    pub(crate) error_code: Option<String>,
    pub(crate) duration_ms: i64,
}

#[derive(Default)]
pub(crate) struct RecordingWriter {
    records: Mutex<Vec<RecordedInvocation>>,
    attempts: AtomicUsize,
    failing: AtomicBool,
}

impl RecordingWriter {
    pub(crate) fn records(&self) -> Vec<RecordedInvocation> {
        self.records.lock().expect("recorded invocations").clone()
    }

    pub(crate) fn attempts(&self) -> usize {
        self.attempts.load(Ordering::SeqCst)
    }

    pub(crate) fn fail_writes(&self, failing: bool) {
        self.failing.store(failing, Ordering::SeqCst);
    }
}

impl InvocationWriter for RecordingWriter {
    fn write<'a>(&'a self, invocation: NewCommandInvocation<'a>) -> InvocationWriteFuture<'a> {
        Box::pin(async move {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            if self.failing.load(Ordering::SeqCst) {
                return Err(notegate_core::Error::internal(
                    "injected invocation write failure",
                ));
            }
            self.records
                .lock()
                .expect("recorded invocations")
                .push(RecordedInvocation {
                    invocation_id: invocation.invocation_id,
                    owner_user_id: invocation.owner_user_id,
                    actor_account_id: invocation.actor_account_id,
                    caller_kind: invocation.caller_kind,
                    surface: invocation.surface,
                    tool: invocation.tool.to_owned(),
                    op: invocation.op.map(str::to_owned),
                    purpose: invocation.purpose.map(str::to_owned),
                    space_name: invocation.space_name.map(str::to_owned),
                    input: invocation.input.clone(),
                    response: invocation.response.cloned(),
                    outcome: invocation.outcome,
                    error_code: invocation.error_code.map(str::to_owned),
                    duration_ms: invocation.duration_ms,
                });
            Ok(())
        })
    }
}

pub(crate) fn user_caller() -> Caller {
    let id = Uuid::new_v4();
    let created_at = chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("fixture timestamp");
    Caller {
        account: notegate_model::account::Account {
            id,
            kind: notegate_model::account::AccountKind::User,
            display_name: "invocation-test".to_owned(),
            is_active: true,
            deleted_at: None,
            deleted_by: None,
            created_at,
            updated_at: created_at,
        },
        identity: CallerIdentity::User(notegate_model::user::User {
            id,
            email: Some("invocation@example.test".to_owned()),
            tier: "default".to_owned(),
            anonymized_at: None,
        }),
        channel: Channel::Mcp,
    }
}

pub(crate) async fn capture<T>(future: impl Future<Output = T>) -> (T, String) {
    let recorder = PrometheusBuilder::new()
        .with_recommended_naming(true)
        .build_recorder();
    metrics::with_local_recorder(&recorder, || {
        for name in [
            "notegate_command_invocation_duration",
            "notegate_command_history_duration",
            "notegate_command_completion_duration",
        ] {
            metrics::describe_histogram!(name, metrics::Unit::Seconds, "Test timing");
        }
    });
    let mut future = pin!(future);
    let result =
        poll_fn(|cx| metrics::with_local_recorder(&recorder, || future.as_mut().poll(cx))).await;
    (result, recorder.handle().render())
}

pub(crate) fn sample(body: &str, name: &str, outcome: Option<&str>) -> Option<f64> {
    body.lines()
        .find(|line| {
            line.split('{').next() == Some(name)
                && outcome.is_none_or(|value| line.contains(&format!("outcome=\"{value}\"")))
        })
        .and_then(|line| line.split_whitespace().last())
        .and_then(|value| value.parse().ok())
}

pub(crate) fn assert_completed(body: &str, command_outcome: &str, history_outcome: &str) {
    for (name, outcome) in [
        ("notegate_command_invocations_total", command_outcome),
        ("notegate_command_history_records_total", history_outcome),
        (
            "notegate_command_completion_duration_seconds_count",
            command_outcome,
        ),
    ] {
        assert_eq!(sample(body, name, Some(outcome)), Some(1.0), "{body}");
    }
    assert_eq!(
        sample(body, "notegate_command_invocations_in_flight", None),
        Some(0.0),
        "{body}"
    );
    let execution = sample(
        body,
        "notegate_command_invocation_duration_seconds_sum",
        None,
    )
    .expect("execution timing");
    let history = sample(body, "notegate_command_history_duration_seconds_sum", None)
        .expect("history timing");
    let completion = sample(
        body,
        "notegate_command_completion_duration_seconds_sum",
        None,
    )
    .expect("completion timing");
    assert!(completion >= execution + history, "{body}");
}

pub(crate) fn report(surface: &str, scenario: &str, samples: &[String]) {
    let mut report = format!(
        "Command completion: {surface}/{scenario}, n={}",
        samples.len()
    );
    for (phase, name) in [
        (
            "execution",
            "notegate_command_invocation_duration_seconds_sum",
        ),
        ("history", "notegate_command_history_duration_seconds_sum"),
        (
            "completion",
            "notegate_command_completion_duration_seconds_sum",
        ),
    ] {
        let mut values: Vec<_> = samples
            .iter()
            .map(|body| sample(body, name, None).expect("timing sample") * 1000.0)
            .collect();
        values.sort_by(f64::total_cmp);
        let percentile = |percent: usize| {
            values
                .get((values.len() * percent).div_ceil(100).saturating_sub(1))
                .expect("nonempty measurements")
        };
        report.push_str(&format!(
            "; {phase} p50={:.3}ms p95={:.3}ms",
            percentile(50),
            percentile(95)
        ));
    }
    println!("{report}");
    if let Ok(path) = std::env::var("GITHUB_STEP_SUMMARY") {
        use std::io::Write as _;
        writeln!(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("CI summary"),
            "- {report}"
        )
        .expect("write CI summary");
    }
}
