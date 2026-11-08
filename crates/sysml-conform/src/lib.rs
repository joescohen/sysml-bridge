//! Checks SysML v2 text against the official OMG textual grammar.
//!
//! The grammar is data: the vendored `.kebnf` files under `grammar/` are read at
//! start-up and a general recogniser runs on them. Nothing here shares code or
//! assumptions with `sysml-core`. See `docs/conformance-checker.md`.

pub mod earley;
pub mod grammar;
pub mod kebnf;
pub mod lexer;

use grammar::{Grammar, Sources};

pub const SYSML_GRAMMAR: &str = include_str!("../grammar/SysML-textual-bnf.kebnf");
pub const KERML_GRAMMAR: &str = include_str!("../grammar/KerML-textual-bnf.kebnf");
pub const CORRECTIONS: &str = include_str!("../grammar/corrections.kebnf");
/// Tag of the OMG release the grammar files come from.
pub const GRAMMAR_RELEASE: &str = "2026-08";

/// Bounds on one check. A text over a bound is "not checked", never "valid". There is no limit on the size of a text
/// and no work budget shared by the files of a run: a text is read in steps in proportion to its size (`step_limit`).
#[derive(Debug, Clone)]
pub struct Limits {
    pub max_tokens: usize,
    /// Stored items (12 bytes each).
    pub max_items: usize,
    /// A text may take `steps_base + steps_per_byte * bytes` steps and no more
    /// (see `step_limit`). The 402 files of the OMG release and of this crate's
    /// tests take 3.8 steps for each byte together and at most 16 in one file,
    /// and the work grows in step with the text for every form we know (the
    /// test `work_grows_in_step_with_the_text`). A text that needs more than 200
    /// is one the recogniser reads in more than linear time; it is stopped early
    /// and reported as not checked.
    pub steps_per_byte: u64,
    pub steps_base: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_tokens: 4_000_000,
            max_items: 10_000_000,
            steps_per_byte: 200,
            steps_base: 100_000,
        }
    }
}

impl Limits {
    /// The steps a text of `bytes` bytes may take: in proportion to its size.
    pub fn step_limit(&self, bytes: usize) -> u64 {
        self.steps_base
            .saturating_add(self.steps_per_byte.saturating_mul(bytes as u64))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fault {
    /// 1-based line and column (in characters) of the token found.
    pub line: u32,
    pub column: u32,
    /// The token text, or `end of file`.
    pub found: String,
    /// `keyword`, `symbol`, a token class such as `NAME`, `end of file`, or
    /// `lexical error: ...`.
    pub found_kind: String,
    /// Every token the grammar allows at this place, sorted.
    pub allowed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Valid,
    Invalid(Fault),
    NotChecked(String),
}

#[derive(Debug, Clone)]
pub struct Report {
    pub verdict: Verdict,
    pub bytes: usize,
    pub tokens: usize,
    pub steps: u64,
}

pub struct Checker {
    pub grammar: Grammar,
}

impl Checker {
    /// A checker on the vendored official grammar.
    pub fn new() -> Checker {
        Checker::from_sources(&Sources {
            sysml: SYSML_GRAMMAR,
            kerml: KERML_GRAMMAR,
            corrections: CORRECTIONS,
        })
        .expect("the vendored grammar files compile")
    }

    pub fn from_sources(sources: &Sources<'_>) -> Result<Checker, String> {
        Ok(Checker {
            grammar: Grammar::compile(sources)?,
        })
    }

    /// A checker on the rules exactly as the files give them, without the
    /// rewrites of `Grammar::compile`. Same verdicts, more steps; for the tests.
    pub fn plain() -> Checker {
        Checker {
            grammar: Grammar::compile_with(
                &Sources {
                    sysml: SYSML_GRAMMAR,
                    kerml: KERML_GRAMMAR,
                    corrections: CORRECTIONS,
                },
                false,
            )
            .expect("the vendored grammar files compile"),
        }
    }

    pub fn check_bytes(&self, bytes: &[u8], limits: &Limits) -> Report {
        let not_checked = |reason: String| Report {
            verdict: Verdict::NotChecked(reason),
            bytes: bytes.len(),
            tokens: 0,
            steps: 0,
        };
        match std::str::from_utf8(bytes) {
            Ok(text) => self.check_str(text, limits),
            Err(_) => not_checked("file is not UTF-8 text".into()),
        }
    }

    pub fn check_str(&self, text: &str, limits: &Limits) -> Report {
        let g = &self.grammar;
        let mut report = Report {
            verdict: Verdict::Valid,
            bytes: text.len(),
            tokens: 0,
            steps: 0,
        };
        let chars: Vec<char> = text.chars().collect();
        let tokens = match lexer::lex(g, &chars, limits.max_tokens) {
            Ok(tokens) => tokens,
            Err(lexer::LexStop::TooManyTokens) => {
                report.verdict =
                    Verdict::NotChecked(format!("file has more than {} tokens", limits.max_tokens));
                return report;
            }
        };
        report.tokens = tokens.len();
        let kinds: Vec<u16> = tokens.iter().map(|t| t.kind).collect();
        let step_limit = limits.step_limit(text.len());
        let outcome = earley::recognise(
            g,
            &kinds,
            step_limit,
            limits.max_items,
            &mut report.steps,
        );
        report.verdict = match outcome {
            earley::Outcome::Accept => Verdict::Valid,
            earley::Outcome::Stopped(reason) => Verdict::NotChecked(format!(
                "{reason} (limits: {step_limit} steps for this text, {} stored items)",
                limits.max_items
            )),
            earley::Outcome::Reject { token, allowed } => {
                let allowed = allowed.iter().map(|id| g.show_terminal(*id)).collect();
                Verdict::Invalid(match tokens.get(token) {
                    Some(tok) => {
                        let text = &chars[tok.start..tok.end];
                        let shown = text
                            .iter()
                            .take(60)
                            .take_while(|c| **c != '\n' && **c != '\r');
                        let mut found: String = shown.collect();
                        if found.chars().count() < text.len() {
                            found.push_str("...");
                        }
                        let found_kind = if let Some(fault) = tok.fault {
                            format!("lexical error: {fault}")
                        } else if g.terminal_is_class[tok.kind as usize] {
                            g.terminals[tok.kind as usize].clone()
                        } else if g.keywords.contains_key(&g.terminals[tok.kind as usize]) {
                            "keyword".into()
                        } else {
                            "symbol".into()
                        };
                        Fault {
                            line: tok.line,
                            column: tok.column,
                            found,
                            found_kind,
                            allowed,
                        }
                    }
                    None => {
                        let (mut line, mut column) = (1, 1);
                        for c in &chars {
                            if *c == '\n' {
                                line += 1;
                                column = 1;
                            } else {
                                column += 1;
                            }
                        }
                        Fault {
                            line,
                            column,
                            found: "end of file".into(),
                            found_kind: "end of file".into(),
                            allowed,
                        }
                    }
                })
            }
        };
        report
    }
}

impl Default for Checker {
    fn default() -> Self {
        Checker::new()
    }
}
