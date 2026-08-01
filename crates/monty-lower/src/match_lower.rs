use std::{error::Error, fmt};

use ruff_python_ast::{Expr, MatchCase, Pattern, Singleton, Stmt, StmtMatch};
use ruff_text_size::Ranged;

const MAX_PATTERN_ALTERNATIVES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MatchEdit {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MatchLowerError {
    InvalidSourceRange { start: usize, end: usize },
    EmptyCaseBody,
    GeneratedNameExhausted,
    TooManyPatternAlternatives { limit: usize },
}

impl fmt::Display for MatchLowerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSourceRange { start, end } => {
                write!(formatter, "invalid match source range {start}..{end}")
            }
            Self::EmptyCaseBody => formatter.write_str("match case has no body"),
            Self::GeneratedNameExhausted => {
                formatter.write_str("unable to allocate a collision-free match temporary")
            }
            Self::TooManyPatternAlternatives { limit } => write!(
                formatter,
                "expanded OR patterns exceed the safety limit of {limit} alternatives"
            ),
        }
    }
}

impl Error for MatchLowerError {}

#[derive(Debug, Clone)]
struct PatternPlan {
    condition: String,
    bindings: Vec<String>,
}

impl PatternPlan {
    fn always() -> Self {
        Self {
            condition: "True".to_owned(),
            bindings: Vec::new(),
        }
    }

    fn combine(self, other: Self) -> Self {
        Self {
            condition: format!("({}) and ({})", self.condition, other.condition),
            bindings: self.bindings.into_iter().chain(other.bindings).collect(),
        }
    }
}

struct PatternCompiler<'source> {
    source: &'source str,
    counter: usize,
}

impl<'source> PatternCompiler<'source> {
    const fn new(source: &'source str) -> Self {
        Self { source, counter: 0 }
    }

    fn fresh_name(&mut self, role: &str) -> Result<String, MatchLowerError> {
        for _ in 0..=self.source.len() {
            let name = format!("_monty_compat_match_{role}_{}", self.counter);
            self.counter = self
                .counter
                .checked_add(1)
                .ok_or(MatchLowerError::GeneratedNameExhausted)?;
            if !self.source.contains(&name) {
                return Ok(name);
            }
        }
        Err(MatchLowerError::GeneratedNameExhausted)
    }

    fn source_expr(&self, expression: &Expr) -> Result<String, MatchLowerError> {
        source_slice(self.source, expression.range()).map(str::to_owned)
    }

    fn compile(
        &mut self,
        pattern: &Pattern,
        subject: &str,
    ) -> Result<Vec<PatternPlan>, MatchLowerError> {
        match pattern {
            Pattern::MatchValue(value) => {
                let value = self.source_expr(&value.value)?;
                Ok(vec![PatternPlan {
                    condition: format!("({subject}) == ({value})"),
                    bindings: Vec::new(),
                }])
            }
            Pattern::MatchSingleton(singleton) => {
                let value = match singleton.value {
                    Singleton::None => "None",
                    Singleton::True => "True",
                    Singleton::False => "False",
                };
                Ok(vec![PatternPlan {
                    condition: format!("({subject}) is {value}"),
                    bindings: Vec::new(),
                }])
            }
            Pattern::MatchSequence(sequence) => self.compile_sequence(&sequence.patterns, subject),
            Pattern::MatchMapping(mapping) => {
                let mut plans = vec![PatternPlan {
                    condition: format!("isinstance(({subject}), dict)"),
                    bindings: Vec::new(),
                }];
                let mut key_names = Vec::with_capacity(mapping.keys.len());
                for (key, nested) in mapping.keys.iter().zip(&mapping.patterns) {
                    let key_source = self.source_expr(key)?;
                    let key_name = self.fresh_name("key")?;
                    let value_name = self.fresh_name("value")?;
                    let key_condition = PatternPlan {
                        condition: format!(
                            concat!(
                                "((({key_name} := ({key_source})) in ({subject})) and ",
                                "((({value_name} := ({subject})[{key_name}]) is {value_name})))"
                            ),
                            key_name = key_name,
                            key_source = key_source,
                            subject = subject,
                            value_name = value_name
                        ),
                        bindings: Vec::new(),
                    };
                    plans = combine_plan_sets(plans, vec![key_condition])?;
                    plans = combine_plan_sets(plans, self.compile(nested, &value_name)?)?;
                    key_names.push(key_name);
                }
                if let Some(rest) = &mapping.rest {
                    for plan in &mut plans {
                        plan.bindings.push(format!("{rest} = ({subject}).copy()"));
                        for key in &key_names {
                            plan.bindings.push(format!("{rest}.pop({key}, None)"));
                        }
                    }
                }
                Ok(plans)
            }
            Pattern::MatchClass(class) => self.compile_class(class, subject),
            Pattern::MatchStar(star) => {
                let mut plan = PatternPlan::always();
                if let Some(name) = &star.name {
                    plan.bindings.push(format!("{name} = list({subject})"));
                }
                Ok(vec![plan])
            }
            Pattern::MatchAs(as_pattern) => {
                let mut plans = if let Some(nested) = as_pattern.pattern.as_deref() {
                    self.compile(nested, subject)?
                } else {
                    vec![PatternPlan::always()]
                };
                if let Some(name) = &as_pattern.name {
                    for plan in &mut plans {
                        plan.bindings.push(format!("{name} = {subject}"));
                    }
                }
                Ok(plans)
            }
            Pattern::MatchOr(or_pattern) => {
                let mut plans = Vec::new();
                for nested in &or_pattern.patterns {
                    plans.extend(self.compile(nested, subject)?);
                    if plans.len() > MAX_PATTERN_ALTERNATIVES {
                        return Err(MatchLowerError::TooManyPatternAlternatives {
                            limit: MAX_PATTERN_ALTERNATIVES,
                        });
                    }
                }
                Ok(plans)
            }
        }
    }

    fn compile_sequence(
        &mut self,
        patterns: &[Pattern],
        subject: &str,
    ) -> Result<Vec<PatternPlan>, MatchLowerError> {
        let star_index = patterns
            .iter()
            .position(|pattern| matches!(pattern, Pattern::MatchStar(_)));
        let length_condition = if star_index.is_some() {
            format!("len({subject}) >= {}", patterns.len().saturating_sub(1))
        } else {
            format!("len({subject}) == {}", patterns.len())
        };
        let mut plans = vec![PatternPlan {
            condition: format!(
                "(isinstance(({subject}), list) or isinstance(({subject}), tuple)) and ({length_condition})"
            ),
            bindings: Vec::new(),
        }];

        for (index, nested) in patterns.iter().enumerate() {
            if let Pattern::MatchStar(star) = nested {
                if let Some(name) = &star.name {
                    let suffix = patterns.len().saturating_sub(index.saturating_add(1));
                    let stop = if suffix == 0 {
                        String::new()
                    } else {
                        format!("-{suffix}")
                    };
                    for plan in &mut plans {
                        plan.bindings
                            .push(format!("{name} = list(({subject})[{index}:{stop}])"));
                    }
                }
                continue;
            }

            let item_name = self.fresh_name("item")?;
            let item_index = if let Some(star) = star_index {
                if index > star {
                    let from_end = patterns.len().saturating_sub(index);
                    format!("-{from_end}")
                } else {
                    index.to_string()
                }
            } else {
                index.to_string()
            };
            plans = combine_plan_sets(
                plans,
                vec![PatternPlan {
                    condition: format!(
                        "((({item_name} := ({subject})[{item_index}]) is {item_name}))"
                    ),
                    bindings: Vec::new(),
                }],
            )?;
            plans = combine_plan_sets(plans, self.compile(nested, &item_name)?)?;
        }
        Ok(plans)
    }

    fn compile_class(
        &mut self,
        class: &ruff_python_ast::PatternMatchClass,
        subject: &str,
    ) -> Result<Vec<PatternPlan>, MatchLowerError> {
        let class_source = self.source_expr(&class.cls)?;
        let class_name = self.fresh_name("class")?;
        let mut plans = vec![PatternPlan {
            condition: format!(
                "((({class_name} := ({class_source})) is {class_name})) and isinstance(({subject}), {class_name})"
            ),
            bindings: Vec::new(),
        }];

        let builtin_self_match = matches!(
            class_source.as_str(),
            "bool"
                | "bytes"
                | "bytearray"
                | "dict"
                | "float"
                | "frozenset"
                | "int"
                | "list"
                | "set"
                | "str"
                | "tuple"
        );
        for (index, nested) in class.arguments.patterns.iter().enumerate() {
            let value_name = self.fresh_name("attribute")?;
            let value_condition = if builtin_self_match && class.arguments.patterns.len() == 1 {
                format!("((({value_name} := ({subject})) is {value_name}))")
            } else {
                let match_args = self.fresh_name("match_args")?;
                let attr_name = self.fresh_name("attribute_name")?;
                format!(
                    concat!(
                        "hasattr({class_name}, '__match_args__') and ",
                        "((({match_args} := {class_name}.__match_args__) is {match_args})) and ",
                        "(len({match_args}) > {index}) and ",
                        "((({attr_name} := {match_args}[{index}]) is {attr_name})) and ",
                        "hasattr(({subject}), {attr_name}) and ",
                        "((({value_name} := getattr(({subject}), {attr_name})) is {value_name}))"
                    ),
                    class_name = class_name,
                    match_args = match_args,
                    index = index,
                    attr_name = attr_name,
                    subject = subject,
                    value_name = value_name
                )
            };
            plans = combine_plan_sets(
                plans,
                vec![PatternPlan {
                    condition: value_condition,
                    bindings: Vec::new(),
                }],
            )?;
            plans = combine_plan_sets(plans, self.compile(nested, &value_name)?)?;
        }

        for keyword in &class.arguments.keywords {
            let value_name = self.fresh_name("attribute")?;
            let attribute = keyword.attr.to_string();
            plans = combine_plan_sets(
                plans,
                vec![PatternPlan {
                    condition: format!(
                        concat!(
                            "hasattr(({subject}), {attribute:?}) and ",
                            "((({value_name} := getattr(({subject}), {attribute:?})) is {value_name}))"
                        ),
                        subject = subject,
                        attribute = attribute,
                        value_name = value_name
                    ),
                    bindings: Vec::new(),
                }],
            )?;
            plans = combine_plan_sets(plans, self.compile(&keyword.pattern, &value_name)?)?;
        }
        Ok(plans)
    }
}

#[allow(clippy::needless_pass_by_value)]
fn combine_plan_sets(
    left: Vec<PatternPlan>,
    right: Vec<PatternPlan>,
) -> Result<Vec<PatternPlan>, MatchLowerError> {
    if left.len().saturating_mul(right.len()) > MAX_PATTERN_ALTERNATIVES {
        return Err(MatchLowerError::TooManyPatternAlternatives {
            limit: MAX_PATTERN_ALTERNATIVES,
        });
    }
    let mut combined = Vec::with_capacity(left.len().saturating_mul(right.len()));
    for left_plan in left {
        for right_plan in &right {
            combined.push(left_plan.clone().combine(right_plan.clone()));
        }
    }
    Ok(combined)
}

pub(crate) fn lower_match(
    source: &str,
    statement: &StmtMatch,
) -> Result<MatchEdit, MatchLowerError> {
    let statement_start = to_offset(statement.start());
    let statement_end = to_offset(statement.end());
    let edit_start = line_start(source, statement_start)?;
    let edit_end = line_end_including_newline(source, statement_end)?;
    let outer_indent =
        source
            .get(edit_start..statement_start)
            .ok_or(MatchLowerError::InvalidSourceRange {
                start: edit_start,
                end: statement_start,
            })?;
    let indent_unit = infer_indent_unit(source, outer_indent, &statement.cases)?;
    let subject_source = source_slice(source, statement.subject.range())?;
    let mut compiler = PatternCompiler::new(source);
    let subject_name = compiler.fresh_name("subject")?;
    let matched_name = compiler.fresh_name("done")?;
    let mut output = String::new();

    push_line(
        &mut output,
        outer_indent,
        &format!("{subject_name} = ({subject_source})"),
    );
    push_line(
        &mut output,
        outer_indent,
        &format!("{matched_name} = False"),
    );

    for case in &statement.cases {
        let hit_name = compiler.fresh_name("case")?;
        let plans = compiler.compile(&case.pattern, &subject_name)?;
        push_line(
            &mut output,
            outer_indent,
            &format!("if not {matched_name}:"),
        );
        let level_one = nested_indent(outer_indent, &indent_unit, 1);
        let level_two = nested_indent(outer_indent, &indent_unit, 2);
        let level_three = nested_indent(outer_indent, &indent_unit, 3);
        push_line(&mut output, &level_one, &format!("{hit_name} = False"));
        for (index, plan) in plans.iter().enumerate() {
            let keyword = if index == 0 { "if" } else { "elif" };
            push_line(
                &mut output,
                &level_one,
                &format!("{keyword} {}:", plan.condition),
            );
            for binding in &plan.bindings {
                push_line(&mut output, &level_two, binding);
            }
            push_line(&mut output, &level_two, &format!("{hit_name} = True"));
        }
        push_line(&mut output, &level_one, &format!("if {hit_name}:"));
        let body_level = if let Some(guard) = case.guard.as_deref() {
            let guard_source = source_slice(source, guard.range())?;
            push_line(&mut output, &level_two, &format!("if {guard_source}:"));
            push_line(&mut output, &level_three, &format!("{matched_name} = True"));
            3
        } else {
            push_line(&mut output, &level_two, &format!("{matched_name} = True"));
            2
        };
        let body_indent = nested_indent(outer_indent, &indent_unit, body_level);
        output.push_str(&suite_source(source, &case.body, &body_indent)?);
    }

    Ok(MatchEdit {
        start: edit_start,
        end: edit_end,
        replacement: output,
    })
}

fn infer_indent_unit(
    source: &str,
    outer_indent: &str,
    cases: &[MatchCase],
) -> Result<String, MatchLowerError> {
    for case in cases {
        if let Some(first) = case.body.first() {
            let logical_start = logical_statement_start(first);
            let start = line_start(source, logical_start)?;
            let body_indent =
                source
                    .get(start..logical_start)
                    .ok_or(MatchLowerError::InvalidSourceRange {
                        start,
                        end: logical_start,
                    })?;
            if let Some(relative) = body_indent.strip_prefix(outer_indent)
                && !relative.is_empty()
            {
                let width = relative.len() / 2;
                if width > 0
                    && let Some(unit) = relative.get(..width)
                {
                    return Ok(unit.to_owned());
                }
            }
        }
    }
    Ok("    ".to_owned())
}

fn suite_source(source: &str, body: &[Stmt], new_indent: &str) -> Result<String, MatchLowerError> {
    let Some(first) = body.first() else {
        return Err(MatchLowerError::EmptyCaseBody);
    };
    let Some(last) = body.last() else {
        return Err(MatchLowerError::EmptyCaseBody);
    };
    let logical_start = logical_statement_start(first);
    let start = line_start(source, logical_start)?;
    let end = line_end_including_newline(source, to_offset(last.end()))?;
    let old_indent =
        source
            .get(start..logical_start)
            .ok_or(MatchLowerError::InvalidSourceRange {
                start,
                end: logical_start,
            })?;
    let block = source
        .get(start..end)
        .ok_or(MatchLowerError::InvalidSourceRange { start, end })?;
    let mut output = String::new();
    for line in block.split_inclusive('\n') {
        if line.trim().is_empty() {
            output.push_str(line);
        } else if let Some(dedented) = line.strip_prefix(old_indent) {
            output.push_str(new_indent);
            output.push_str(dedented);
        } else {
            output.push_str(new_indent);
            output.push_str(line);
        }
    }
    Ok(output)
}

fn nested_indent(base: &str, unit: &str, level: usize) -> String {
    let mut indent =
        String::with_capacity(base.len().saturating_add(unit.len().saturating_mul(level)));
    indent.push_str(base);
    for _ in 0..level {
        indent.push_str(unit);
    }
    indent
}

fn push_line(output: &mut String, indent: &str, content: &str) {
    output.push_str(indent);
    output.push_str(content);
    output.push('\n');
}

fn source_slice(source: &str, range: ruff_text_size::TextRange) -> Result<&str, MatchLowerError> {
    let start = to_offset(range.start());
    let end = to_offset(range.end());
    source
        .get(start..end)
        .ok_or(MatchLowerError::InvalidSourceRange { start, end })
}

fn logical_statement_start(statement: &Stmt) -> usize {
    match statement {
        Stmt::FunctionDef(function) => function.decorator_list.first().map_or_else(
            || to_offset(function.start()),
            |decorator| to_offset(decorator.start()),
        ),
        Stmt::ClassDef(class) => class.decorator_list.first().map_or_else(
            || to_offset(class.start()),
            |decorator| to_offset(decorator.start()),
        ),
        _ => to_offset(statement.start()),
    }
}

fn line_start(source: &str, offset: usize) -> Result<usize, MatchLowerError> {
    let prefix = source
        .get(..offset)
        .ok_or(MatchLowerError::InvalidSourceRange {
            start: 0,
            end: offset,
        })?;
    Ok(prefix
        .rfind('\n')
        .map_or(0, |index| index.saturating_add(1)))
}

fn line_end_including_newline(source: &str, offset: usize) -> Result<usize, MatchLowerError> {
    let suffix = source
        .get(offset..)
        .ok_or(MatchLowerError::InvalidSourceRange {
            start: offset,
            end: source.len(),
        })?;
    match suffix.find('\n') {
        Some(index) => offset
            .checked_add(index)
            .and_then(|end| end.checked_add(1))
            .ok_or(MatchLowerError::InvalidSourceRange {
                start: offset,
                end: source.len(),
            }),
        None => Ok(source.len()),
    }
}

fn to_offset(size: ruff_text_size::TextSize) -> usize {
    size.to_usize()
}
