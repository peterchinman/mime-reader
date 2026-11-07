//! Writes every scored pair, one similarity column per aligner.

use std::fmt::Write as _;
use std::path::Path;

use mime_reader::echo::AlignerKind;

use crate::Scored;
use crate::corpus::Label;

fn label(label: Label) -> &'static str {
    match label {
        Label::Echo => "echo",
        Label::Not => "not",
        Label::Unlabeled => "unlabeled",
    }
}

fn csv_field(field: &str) -> String {
    if field.contains([',', '"', '\n']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

pub fn csv(scored: &[Scored], aligners: &[AlignerKind], poem_ids: &[String]) -> String {
    let mut out = String::from("source,group,label,poem,i,j,a,b");
    for kind in aligners {
        write!(out, ",{kind}").unwrap();
    }
    out.push('\n');
    for row in scored {
        let pair = &row.pair;
        let (i, j) = pair.lines.map_or((String::new(), String::new()), |(i, j)| {
            (i.to_string(), j.to_string())
        });
        let poem = pair.poem.map(|p| poem_ids[p].clone()).unwrap_or_default();
        write!(
            out,
            "{},{},{},{},{i},{j},{},{}",
            pair.source,
            csv_field(&pair.group),
            label(pair.label),
            csv_field(&poem),
            csv_field(&pair.a),
            csv_field(&pair.b),
        )
        .unwrap();
        for score in &row.scores {
            write!(out, ",{score:.6}").unwrap();
        }
        out.push('\n');
    }
    out
}

pub fn json(scored: &[Scored], aligners: &[AlignerKind], poem_ids: &[String]) -> String {
    let rows: Vec<serde_json::Value> = scored
        .iter()
        .map(|row| {
            let pair = &row.pair;
            let scores: serde_json::Map<String, serde_json::Value> = aligners
                .iter()
                .zip(&row.scores)
                .map(|(kind, &score)| (kind.to_string(), serde_json::json!(score)))
                .collect();
            serde_json::json!({
                "source": pair.source,
                "group": pair.group,
                "label": label(pair.label),
                "poem": pair.poem.map(|p| poem_ids[p].clone()),
                "lines": pair.lines.map(|(i, j)| [i, j]),
                "a": pair.a,
                "b": pair.b,
                "similarity": scores,
            })
        })
        .collect();
    serde_json::to_string_pretty(&rows).expect("rows serialize")
}

/// Writes `.json` as JSON and anything else as CSV, creating directories.
pub fn write(
    path: &Path,
    scored: &[Scored],
    aligners: &[AlignerKind],
    poem_ids: &[String],
) -> Result<(), String> {
    let text = if path.extension().is_some_and(|e| e == "json") {
        json(scored, aligners, poem_ids)
    } else {
        csv(scored, aligners, poem_ids)
    };
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::LabeledPair;

    #[test]
    fn csv_quotes_fields_and_has_a_column_per_aligner() {
        let scored = vec![Scored {
            pair: LabeledPair {
                source: "fixture".into(),
                group: "01:verbatim".into(),
                label: Label::Echo,
                poem: None,
                lines: None,
                a: "the light, gone".into(),
                b: "say \"hi\"".into(),
            },
            scores: vec![1.0, 0.5],
        }];
        let text = csv(&scored, &[AlignerKind::SemiGlobal, AlignerKind::Jump], &[]);
        let mut lines = text.lines();
        assert_eq!(
            lines.next(),
            Some("source,group,label,poem,i,j,a,b,semi_global,jump")
        );
        assert_eq!(
            lines.next(),
            Some(
                "fixture,01:verbatim,echo,,,,\"the light, gone\",\"say \"\"hi\"\"\",1.000000,0.500000"
            )
        );
        let parsed: serde_json::Value = serde_json::from_str(&json(
            &scored,
            &[AlignerKind::SemiGlobal, AlignerKind::Jump],
            &[],
        ))
        .unwrap();
        assert_eq!(parsed[0]["similarity"]["jump"], 0.5);
    }
}
