//! Isolated metrics capture: each poll installs its recorder only for that poll.

#![allow(clippy::expect_used)]

use std::future::{Future, poll_fn};
use std::pin::pin;

use metrics_exporter_prometheus::PrometheusBuilder;

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
                .append(true)
                .open(path)
                .expect("CI summary"),
            "- {report}"
        )
        .expect("write CI summary");
    }
}
