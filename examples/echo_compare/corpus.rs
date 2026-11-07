//! The labeled corpus: poem files, synthetic fixture pairs, and the pairs
//! they yield.
//!
//! A poem file is a `key: value` header (`form`, `title`, `author`
//! required; `year`, `source`, `notes`, `template` optional), a blank line,
//! then the body. Blank lines in the body separate stanzas and are not
//! counted as lines.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mime_reader::echo::EchoTemplate;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Label {
    Echo,
    Not,
    Unlabeled,
}

#[derive(Debug, Clone)]
pub struct Poem {
    pub path: PathBuf,
    /// "public" or "private": the corpus directory it came from.
    pub source: String,
    pub form: String,
    pub title: String,
    pub author: String,
    pub year: Option<String>,
    pub template: EchoTemplate,
    /// The body's non-blank lines.
    pub lines: Vec<String>,
    /// Lines per stanza.
    pub stanzas: Vec<usize>,
}

impl Poem {
    /// A short id for tables: `form/file-stem`.
    pub fn id(&self) -> String {
        let stem = self
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!("{}/{stem}", self.form)
    }

    /// "Title — Author (year), N lines in M stanzas", for the per-poem table.
    pub fn describe(&self) -> String {
        let year = self
            .year
            .as_deref()
            .map(|y| format!(" ({y})"))
            .unwrap_or_default();
        format!(
            "{} — {}{year}, {} lines in {} stanzas",
            self.title,
            self.author,
            self.lines.len(),
            self.stanzas.len()
        )
    }
}

/// Parses one poem file.
pub fn parse_poem(text: &str, path: &Path, source: &str) -> Result<Poem, String> {
    let fail = |message: String| format!("{}: {message}", path.display());
    let mut header = BTreeMap::new();
    let mut body_lines = text.lines();
    for line in body_lines.by_ref() {
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| fail(format!("header line without a colon: {line:?}")))?;
        header.insert(key.trim().to_lowercase(), value.trim().to_string());
    }
    let required = |key: &str| {
        header
            .get(key)
            .cloned()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| fail(format!("missing header {key:?}")))
    };
    let form = required("form")?.to_lowercase();
    let title = required("title")?;
    let author = required("author")?;

    let mut lines = Vec::new();
    let mut stanzas = Vec::new();
    let mut stanza = 0;
    for line in body_lines {
        let line = line.trim_end();
        if line.trim().is_empty() {
            if stanza > 0 {
                stanzas.push(stanza);
                stanza = 0;
            }
        } else {
            lines.push(line.trim().to_string());
            stanza += 1;
        }
    }
    if stanza > 0 {
        stanzas.push(stanza);
    }
    if lines.is_empty() {
        return Err(fail("empty body".into()));
    }

    let template = template_for(
        &form,
        header.get("template").map(String::as_str),
        &lines,
        &stanzas,
    )
    .map_err(fail)?;
    Ok(Poem {
        path: path.to_path_buf(),
        source: source.to_string(),
        form,
        title,
        author,
        year: header.get("year").cloned(),
        template,
        lines,
        stanzas,
    })
}

/// The poem's template: an explicit `template:` header, or the form's
/// standard one after checking the poem has the form's shape.
fn template_for(
    form: &str,
    explicit: Option<&str>,
    lines: &[String],
    stanzas: &[usize],
) -> Result<EchoTemplate, String> {
    let template = match explicit {
        Some(scheme) => {
            EchoTemplate::parse(scheme).map_err(|e| format!("template {scheme:?}: {e}"))?
        }
        None => {
            let shaped = |size: usize| stanzas.iter().all(|&s| s == size);
            match form {
                "villanelle" => {
                    if stanzas.len() > 1 && stanzas != [3, 3, 3, 3, 3, 4] {
                        return Err(format!(
                            "villanelle stanzas {stanzas:?}, expected 3,3,3,3,3,4"
                        ));
                    }
                    EchoTemplate::villanelle()
                }
                "pantoum" if shaped(4) => EchoTemplate::pantoum(stanzas.len()),
                "pantoum" => {
                    return Err(format!("pantoum stanzas {stanzas:?}, expected quatrains"));
                }
                "blues" if shaped(3) => EchoTemplate::blues(stanzas.len()),
                "blues" => {
                    return Err(format!(
                        "blues stanzas {stanzas:?}, expected tercets (or a template)"
                    ));
                }
                "duplex" => EchoTemplate::duplex(),
                "triolet" => EchoTemplate::parse("AB_A__AB").expect("triolet template"),
                other => return Err(format!("form {other:?} needs an explicit template")),
            }
        }
    };
    if let Some(expected) = template.expected_line_count
        && expected != lines.len()
    {
        return Err(format!(
            "{form}: {} lines, template {:?} expects {expected}",
            lines.len(),
            template.name
        ));
    }
    Ok(template)
}

/// Every `.txt` poem under `dir`, sorted by path. `source` labels them.
pub fn load_dir(dir: &Path, source: &str) -> Result<Vec<Poem>, String> {
    let mut paths = Vec::new();
    collect_txt(dir, &mut paths).map_err(|e| format!("{}: {e}", dir.display()))?;
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            parse_poem(&text, path, source)
        })
        .collect()
}

fn collect_txt(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_txt(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "txt") {
            out.push(path);
        }
    }
    Ok(())
}

/// One pair to score, with where it came from and what it should be.
#[derive(Debug, Clone)]
pub struct LabeledPair {
    /// "fixture", "public", "private" or "mutation".
    pub source: String,
    /// The reporting group: a fixture class, `form echo` / `form non-echo`,
    /// or a mutation kind.
    pub group: String,
    pub label: Label,
    /// The poem index, for corpus pairs.
    pub poem: Option<usize>,
    /// Line indices within the poem, for corpus pairs.
    pub lines: Option<(usize, usize)>,
    pub a: String,
    pub b: String,
}

/// A poem's pairs: the template's echoes as positives, every other pair of
/// lines as negatives.
pub fn poem_pairs(poem: &Poem, index: usize) -> Vec<LabeledPair> {
    let positives = poem.template.all_pairs();
    let mut pairs = Vec::new();
    for i in 0..poem.lines.len() {
        for j in i + 1..poem.lines.len() {
            let echo = positives.binary_search(&(i, j)).is_ok();
            pairs.push(LabeledPair {
                source: poem.source.clone(),
                group: format!("{} {}", poem.form, if echo { "echo" } else { "non-echo" }),
                label: if echo { Label::Echo } else { Label::Not },
                poem: Some(index),
                lines: Some((i, j)),
                a: poem.lines[i].clone(),
                b: poem.lines[j].clone(),
            });
        }
    }
    pairs
}

#[derive(Debug, Deserialize)]
struct FixtureGroup {
    #[allow(dead_code)]
    comment: String,
    class: u32,
    expected: String,
    pairs: Vec<(String, String)>,
}

/// The synthetic pairs of an `echo_pair_tests.json`-shaped file, grouped as
/// `class:name`.
pub fn load_fixture(path: &Path) -> Result<Vec<LabeledPair>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let groups: BTreeMap<String, FixtureGroup> =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut groups: Vec<(String, FixtureGroup)> = groups.into_iter().collect();
    groups.sort_by_key(|(name, group)| (group.class, name.clone()));
    let mut pairs = Vec::new();
    for (name, group) in groups {
        let label = match group.expected.as_str() {
            "echo" => Label::Echo,
            "not" => Label::Not,
            "unlabeled" => Label::Unlabeled,
            other => {
                return Err(format!(
                    "{name}: expected {other:?} is not echo|not|unlabeled"
                ));
            }
        };
        for (a, b) in group.pairs {
            pairs.push(LabeledPair {
                source: "fixture".into(),
                group: format!("{:02}:{name}", group.class),
                label,
                poem: None,
                lines: None,
                a,
                b,
            });
        }
    }
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::repo_path;

    #[test]
    fn committed_public_corpus_is_well_formed() {
        let dir = repo_path("data/echo/public");
        let poems = load_dir(&dir, "public").unwrap_or_else(|e| panic!("{e}"));
        assert!(!poems.is_empty(), "no poems under {}", dir.display());
        for poem in &poems {
            assert!(!poem.template.groups.is_empty(), "{}", poem.id());
            let last = poem
                .template
                .groups
                .iter()
                .flatten()
                .max()
                .copied()
                .unwrap();
            assert!(
                last < poem.lines.len(),
                "{}: template beyond the poem",
                poem.id()
            );
        }
    }

    #[test]
    fn the_committed_fixture_loads() {
        let pairs = load_fixture(&repo_path("echo_pair_tests.json")).unwrap();
        assert!(pairs.iter().any(|p| p.label == Label::Echo));
        assert!(pairs.iter().any(|p| p.label == Label::Not));
        assert!(pairs.iter().any(|p| p.group == "07:resegmentation"));
    }

    const VILLANELLE: &str = "form: villanelle\ntitle: T\nauthor: A\n\n\
        a1\nb\na2\n\nc\nd\na1\n\ne\nf\na2\n\ng\nh\na1\n\ni\nj\na2\n\nk\nl\na1\na2\n";

    #[test]
    fn a_poem_parses_with_its_forms_template() {
        let poem = parse_poem(VILLANELLE, Path::new("v.txt"), "public").unwrap();
        assert_eq!(poem.lines.len(), 19);
        assert_eq!(poem.stanzas, [3, 3, 3, 3, 3, 4]);
        let pairs = poem_pairs(&poem, 0);
        assert_eq!(pairs.len(), 171);
        assert_eq!(pairs.iter().filter(|p| p.label == Label::Echo).count(), 12);
        let (i, j) = pairs
            .iter()
            .find(|p| p.label == Label::Echo)
            .unwrap()
            .lines
            .unwrap();
        assert_eq!((i, j), (0, 5));
    }

    #[test]
    fn malformed_poems_are_rejected() {
        let short = VILLANELLE.replace("k\nl\na1\na2\n", "k\nl\na1\n");
        assert!(parse_poem(&short, Path::new("v.txt"), "public").is_err());
        assert!(parse_poem("title: T\nauthor: A\n\nx\n", Path::new("x.txt"), "public").is_err());
        let refrain = "form: refrain\ntitle: T\nauthor: A\n\nx\ny\nx\n";
        assert!(parse_poem(refrain, Path::new("r.txt"), "public").is_err());
        let with_template =
            "form: refrain\ntemplate: A_A\ntitle: T\nauthor: A\n\nx\ny\nx\n".to_string();
        assert_eq!(
            parse_poem(&with_template, Path::new("r.txt"), "public")
                .unwrap()
                .template
                .groups,
            [vec![0, 2]]
        );
    }
}
