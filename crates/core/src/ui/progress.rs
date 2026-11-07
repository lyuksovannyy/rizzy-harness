//! `ui:progress`: where a longer job stands (rich_plan §4.4). The model
//! repeats it with the same id after each phase; the UI shows the latest
//! one of a turn in full and the earlier ones as one-line snapshots.

use serde::{Deserialize, Serialize};

use super::Num;
use super::read::{Obj, Res};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressSpec {
    pub title: String,
    /// When absent, the UI works it out from the steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ProgressStatus>,
    /// 0 to 1; when absent, the share of finished steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Num>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressStatus {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub label: String,
    pub status: StepStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Active,
    Done,
    Failed,
    Skipped,
}

pub(super) fn validate(mut o: Obj) -> Res<ProgressSpec> {
    use ProgressStatus as P;
    use StepStatus as S;
    let title = o.req_text("title", 120)?;
    let status = o.word(
        "status",
        &[
            ("running", P::Running),
            ("done", P::Done),
            ("failed", P::Failed),
        ],
    )?;
    let value = o.num("value")?;
    if value.is_some_and(|v| !(0.0..=1.0).contains(&v)) {
        return o.err("value", "must be 0 to 1");
    }
    let list = o.list("steps", 1, 20)?;
    let path = o.at("steps");
    let mut steps = Vec::with_capacity(list.len());
    for (i, v) in list.iter().enumerate() {
        let mut s = Obj::new(v, format!("{path}[{i}]"))?;
        let label = s.req_text("label", 120)?;
        let words = [
            ("pending", S::Pending),
            ("active", S::Active),
            ("done", S::Done),
            ("failed", S::Failed),
            ("skipped", S::Skipped),
        ];
        let Some(status) = s.word("status", &words)? else {
            return s.err("status", "missing");
        };
        let detail = s.text("detail", 200, false)?;
        s.finish()?;
        steps.push(Step {
            label,
            status,
            detail,
        });
    }
    o.finish()?;
    Ok(ProgressSpec {
        title,
        status,
        value: value.map(Num),
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn run(v: Value) -> Res<Value> {
        let o = Obj::new(&v, String::new())?;
        validate(o).map(|s| serde_json::to_value(s).unwrap())
    }

    #[test]
    fn normalizes() {
        let spec = json!({
            "title": "Migrating config", "status": "running", "value": 0.4,
            "steps": [
                {"label": "Read config", "status": "done"},
                {"label": "Write schema", "status": "active", "detail": "3 of 7 tables"},
                {"label": "Run tests", "status": "pending"}
            ]
        });
        assert_eq!(run(spec.clone()), Ok(spec));
        assert_eq!(
            run(json!({"title": "T", "steps": [{"label": "a", "status": "skipped"}], "value": 1})),
            Ok(json!({"title": "T", "value": 1, "steps": [{"label": "a", "status": "skipped"}]}))
        );
    }

    #[test]
    fn rejects() {
        let step = json!({"label": "a", "status": "done"});
        for (spec, err) in [
            (json!({"steps": [step]}), "title: missing"),
            (
                json!({"title": "T", "steps": []}),
                "steps: fewer than 1 entries",
            ),
            (
                json!({"title": "T", "steps": vec![step.clone(); 21]}),
                "steps: more than 20 entries",
            ),
            (
                json!({"title": "T", "value": 1.5, "steps": [step]}),
                "value: must be 0 to 1",
            ),
            (
                json!({"title": "T", "status": "paused", "steps": [step]}),
                "status: must be running, done or failed",
            ),
            (
                json!({"title": "T", "steps": [{"label": "a"}]}),
                "steps[0].status: missing",
            ),
            (
                json!({"title": "T", "steps": [{"label": "a", "status": "busy"}]}),
                "steps[0].status: must be pending, active, done, failed or skipped",
            ),
            (
                json!({"title": "T", "steps": [{"label": "a", "status": "done", "detail": "x".repeat(201)}]}),
                "steps[0].detail: longer than 200 characters",
            ),
            (
                json!({"title": "T", "steps": [{"label": "a", "status": "done", "eta": 3}]}),
                "steps[0].eta: unknown key",
            ),
        ] {
            assert_eq!(run(spec.clone()), Err(err.to_string()), "{spec}");
        }
    }
}
