//! `ui:chart`: a small bar or line chart of numbers the model got from a
//! tool or the user (rich_plan §4.4; the renderer is step 10).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Num;
use super::read::{Obj, Res, cleaned};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartSpec {
    pub kind: ChartKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub x: XAxis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<YAxis>,
    pub series: Vec<Series>,
    /// Bar charts only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stacked: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartKind {
    Bar,
    Line,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XAxis {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub values: XValues,
}

/// All labels or all numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum XValues {
    Labels(Vec<String>),
    Numbers(Vec<Num>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YAxis {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Series {
    pub name: String,
    /// One per x value; `None` is a gap.
    pub values: Vec<Option<Num>>,
}

pub(super) fn validate(mut o: Obj) -> Res<ChartSpec> {
    let Some(kind) = o.word(
        "kind",
        &[("bar", ChartKind::Bar), ("line", ChartKind::Line)],
    )?
    else {
        return o.err("kind", "missing");
    };
    let title = o.text("title", 120, false)?;
    let Some(mut xo) = o.obj("x")? else {
        return o.err("x", "missing");
    };
    let x = XAxis {
        label: xo.text("label", 60, false)?,
        values: x_values(&mut xo)?,
    };
    xo.finish()?;
    let n = match &x.values {
        XValues::Labels(v) => v.len(),
        XValues::Numbers(v) => v.len(),
    };
    let y = match o.obj("y")? {
        None => None,
        Some(mut yo) => {
            let y = YAxis {
                label: yo.text("label", 60, false)?,
                unit: yo.text("unit", 12, false)?,
            };
            yo.finish()?;
            (y.label.is_some() || y.unit.is_some()).then_some(y)
        }
    };
    let list = o.list("series", 1, 6)?;
    let path = o.at("series");
    let mut series = Vec::with_capacity(list.len());
    for (i, v) in list.iter().enumerate() {
        let mut s = Obj::new(v, format!("{path}[{i}]"))?;
        let name = s.req_text("name", 40)?;
        let vals = s.list("values", 0, 100)?;
        if vals.len() != n {
            return s.err(
                "values",
                format_args!("expected {n} values, one per x value"),
            );
        }
        let at = s.at("values");
        let values = vals
            .iter()
            .enumerate()
            .map(|(j, v)| match v {
                Value::Null => Ok(None),
                v => v
                    .as_f64()
                    .map(|f| Some(Num(f)))
                    .ok_or_else(|| format!("{at}[{j}]: expected a number or null")),
            })
            .collect::<Res<Vec<_>>>()?;
        s.finish()?;
        series.push(Series { name, values });
    }
    let stacked = o.bool("stacked")?;
    let stacked = match kind {
        ChartKind::Bar => Some(stacked.unwrap_or(false)),
        ChartKind::Line if stacked.is_some() => return o.err("stacked", "only for bar charts"),
        ChartKind::Line => None,
    };
    o.finish()?;
    Ok(ChartSpec {
        kind,
        title,
        x,
        y,
        series,
        stacked,
    })
}

/// `x.values`: 1 to 100 labels (at most 40 characters) or numbers.
fn x_values(xo: &mut Obj) -> Res<XValues> {
    let list = xo.list("values", 1, 100)?;
    let at = xo.at("values");
    if list[0].is_number() {
        let nums = list.iter().enumerate().map(|(i, v)| {
            v.as_f64()
                .map(Num)
                .ok_or_else(|| format!("{at}[{i}]: expected a number like the first"))
        });
        return nums.collect::<Res<_>>().map(XValues::Numbers);
    }
    let labels = list.iter().enumerate().map(|(i, v)| {
        let here = format!("{at}[{i}]");
        let s = v
            .as_str()
            .ok_or_else(|| format!("{here}: expected a string like the first"))?;
        let s = cleaned(s, 40, false, &here)?;
        if s.is_empty() {
            return Err(format!("{here}: must not be empty"));
        }
        Ok(s)
    });
    labels.collect::<Res<_>>().map(XValues::Labels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(v: Value) -> Res<Value> {
        let o = Obj::new(&v, String::new())?;
        validate(o).map(|s| serde_json::to_value(s).unwrap())
    }

    #[test]
    fn normalizes() {
        let spec = json!({
            "kind": "bar", "title": "Build time by crate",
            "x": {"label": "crate", "values": ["core", "web", "master"]},
            "y": {"label": "time", "unit": "s"},
            "series": [{"name": "debug", "values": [12.5, 8, 30]}, {"name": "release", "values": [40, 22, null]}]
        });
        let mut want = spec.clone();
        want["stacked"] = json!(false);
        assert_eq!(run(spec), Ok(want));
        let line = json!({"kind": "line", "x": {"values": [1, 2]}, "y": {}, "series": [{"name": "a", "values": [1, null]}]});
        assert_eq!(
            run(line),
            Ok(
                json!({"kind": "line", "x": {"values": [1, 2]}, "series": [{"name": "a", "values": [1, null]}]})
            )
        );
    }

    #[test]
    fn rejects() {
        let x = json!({"values": ["a", "b"]});
        let s = json!([{"name": "s", "values": [1, 2]}]);
        for (spec, err) in [
            (json!({"x": x, "series": s}), "kind: missing"),
            (
                json!({"kind": "bar", "title": "x".repeat(121), "x": x, "series": s}),
                "title: longer than 120 characters",
            ),
            (
                json!({"kind": "bar", "x": {"label": "x".repeat(61), "values": ["a", "b"]}, "series": s}),
                "x.label: longer than 60 characters",
            ),
            (
                json!({"kind": "bar", "x": x, "y": {"label": "x".repeat(61)}, "series": s}),
                "y.label: longer than 60 characters",
            ),
            (
                json!({"kind": "pie", "x": x, "series": s}),
                "kind: must be bar or line",
            ),
            (
                json!({"kind": "line", "x": x, "series": s, "stacked": true}),
                "stacked: only for bar charts",
            ),
            (
                json!({"kind": "bar", "x": x, "series": [{"name": "s", "values": [1]}]}),
                "series[0].values: expected 2 values, one per x value",
            ),
            (
                json!({"kind": "bar", "x": x, "series": [{"name": "s", "values": [1, "2"]}]}),
                "series[0].values[1]: expected a number or null",
            ),
            (
                json!({"kind": "bar", "x": {"values": ["a", 2]}, "series": s}),
                "x.values[1]: expected a string like the first",
            ),
            (
                json!({"kind": "bar", "x": {"values": [1, "b"]}, "series": s}),
                "x.values[1]: expected a number like the first",
            ),
            (
                json!({"kind": "bar", "x": {"values": ["a", "x".repeat(41)]}, "series": s}),
                "x.values[1]: longer than 40 characters",
            ),
            (
                json!({"kind": "bar", "x": {"values": []}, "series": []}),
                "x.values: fewer than 1 entries",
            ),
            (
                json!({"kind": "bar", "x": {"values": vec![1; 101]}, "series": s}),
                "x.values: more than 100 entries",
            ),
            (
                json!({"kind": "bar", "x": x, "series": vec![s[0].clone(); 7]}),
                "series: more than 6 entries",
            ),
            (
                json!({"kind": "bar", "x": x, "series": [{"name": "x".repeat(41), "values": [1, 2]}]}),
                "series[0].name: longer than 40 characters",
            ),
            (
                json!({"kind": "bar", "x": x, "y": {"unit": "x".repeat(13)}, "series": s}),
                "y.unit: longer than 12 characters",
            ),
            (
                json!({"kind": "bar", "x": {"values": ["a", "b"], "ticks": 3}, "series": s}),
                "x.ticks: unknown key",
            ),
            (json!({"kind": "bar", "series": s}), "x: missing"),
            (
                json!({"kind": "bar", "x": [1], "series": s}),
                "x: expected an object",
            ),
        ] {
            assert_eq!(run(spec.clone()), Err(err.to_string()), "{spec}");
        }
    }
}
