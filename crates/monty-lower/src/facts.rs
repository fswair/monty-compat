use std::collections::{HashMap, HashSet};

use ruff_python_ast::{
    Expr, ExprContext, Stmt, StmtClassDef,
    visitor::{Visitor, walk_expr, walk_stmt},
};
use ruff_text_size::Ranged;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MethodKind {
    Instance,
    Property,
    Static,
    Class,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinKind {
    Dict,
    Float,
    List,
    Tuple,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ClassInfo {
    methods: HashMap<String, MethodKind>,
    bases: Vec<String>,
    members: Vec<ClassMember>,
    definition_start: usize,
    definition_end: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ClassMember {
    pub name: Option<String>,
    pub start: usize,
    pub end: usize,
}

impl ClassInfo {
    pub fn method_kind(&self, name: &str) -> Option<MethodKind> {
        self.methods.get(name).copied()
    }

    pub fn bases(&self) -> &[String] {
        &self.bases
    }

    pub fn members(&self) -> &[ClassMember] {
        &self.members
    }

    pub const fn definition_range(&self) -> (usize, usize) {
        (self.definition_start, self.definition_end)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BindingState {
    Instance(String),
    Builtin(BuiltinKind),
    Ambiguous,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TypeFacts {
    classes: HashMap<String, ClassInfo>,
    bindings: HashMap<String, BindingState>,
    function_names: HashSet<String>,
    dead_results: HashSet<usize>,
}

impl TypeFacts {
    pub fn collect(body: &[Stmt]) -> Self {
        let mut class_collector = ClassCollector::default();
        class_collector.visit_body(body);
        let bindings = {
            let mut binding_collector = BindingCollector {
                classes: &class_collector.classes,
                bindings: HashMap::new(),
            };
            binding_collector.visit_body(body);
            binding_collector.bindings
        };
        let mut usage_collector = UsageCollector::default();
        usage_collector.visit_body(body);
        let dead_results = if usage_collector.has_dynamic_namespace_access {
            HashSet::new()
        } else {
            let mut collector = DeadResultCollector {
                referenced_names: &usage_collector.referenced_names,
                results: HashSet::new(),
            };
            collector.visit_body(body);
            collector.results
        };
        Self {
            classes: class_collector.classes,
            bindings,
            function_names: usage_collector.function_names,
            dead_results,
        }
    }

    pub fn class_for_expr(&self, expression: &Expr) -> Option<&str> {
        match expression {
            Expr::Call(call) => expr_name(&call.func).and_then(|name| self.canonical_class(name)),
            Expr::Name(name) => match self.bindings.get(name.id.as_str()) {
                Some(BindingState::Instance(class)) => Some(class.as_str()),
                Some(BindingState::Builtin(_) | BindingState::Ambiguous) | None => None,
            },
            _ => None,
        }
    }

    pub fn builtin_for_expr(&self, expression: &Expr) -> Option<BuiltinKind> {
        match expression {
            Expr::Dict(_) => Some(BuiltinKind::Dict),
            Expr::List(_) => Some(BuiltinKind::List),
            Expr::Tuple(_) => Some(BuiltinKind::Tuple),
            Expr::Call(call) => match expr_name(&call.func) {
                Some("dict") => Some(BuiltinKind::Dict),
                Some("float") => Some(BuiltinKind::Float),
                Some("list") => Some(BuiltinKind::List),
                Some("tuple") => Some(BuiltinKind::Tuple),
                _ => None,
            },
            Expr::Name(name) => match self.bindings.get(name.id.as_str()) {
                Some(BindingState::Builtin(kind)) => Some(*kind),
                Some(BindingState::Instance(_) | BindingState::Ambiguous) | None => None,
            },
            _ => None,
        }
    }

    pub fn is_name_bound(&self, name: &str) -> bool {
        self.bindings.contains_key(name) || self.classes.contains_key(name)
    }

    pub fn is_function_name(&self, name: &str) -> bool {
        self.function_names.contains(name)
    }

    pub fn is_dead_result(&self, expression: &impl Ranged) -> bool {
        self.dead_results.contains(&expression.start().to_usize())
    }

    pub fn class(&self, name: &str) -> Option<&ClassInfo> {
        self.classes.get(name)
    }

    pub fn canonical_class(&self, name: &str) -> Option<&str> {
        self.classes
            .get_key_value(name)
            .map(|(canonical, _)| canonical.as_str())
    }

    pub fn resolves_method(&self, class: &str, method: &str) -> Option<(&str, MethodKind)> {
        let mut current = self.canonical_class(class)?;
        let mut visited = HashSet::new();
        while visited.insert(current.to_owned()) {
            let (canonical, info) = self.classes.get_key_value(current)?;
            if let Some(kind) = info.method_kind(method) {
                return Some((canonical.as_str(), kind));
            }
            let base = info.bases.first()?;
            current = self.canonical_class(base)?;
        }
        None
    }
}

#[derive(Default)]
struct UsageCollector {
    referenced_names: HashSet<String>,
    function_names: HashSet<String>,
    has_dynamic_namespace_access: bool,
}

impl<'ast> Visitor<'ast> for UsageCollector {
    fn visit_stmt(&mut self, statement: &'ast Stmt) {
        if let Stmt::FunctionDef(function) = statement {
            self.function_names.insert(function.name.to_string());
        }
        walk_stmt(self, statement);
    }

    fn visit_expr(&mut self, expression: &'ast Expr) {
        if let Expr::Name(name) = expression {
            if name.ctx != ExprContext::Store {
                self.referenced_names.insert(name.id.to_string());
            }
            if name.ctx == ExprContext::Load
                && matches!(
                    name.id.as_str(),
                    "eval" | "exec" | "globals" | "locals" | "vars"
                )
            {
                self.has_dynamic_namespace_access = true;
            }
        }
        walk_expr(self, expression);
    }
}

struct DeadResultCollector<'usage> {
    referenced_names: &'usage HashSet<String>,
    results: HashSet<usize>,
}

impl<'ast> Visitor<'ast> for DeadResultCollector<'_> {
    fn visit_stmt(&mut self, statement: &'ast Stmt) {
        match statement {
            Stmt::Assign(assign) if assign.targets.len() == 1 => {
                if let Some(name) = assign.targets.first().and_then(expr_name)
                    && !self.referenced_names.contains(name)
                {
                    self.results.insert(assign.value.start().to_usize());
                }
            }
            Stmt::AnnAssign(assign) => {
                if let (Some(name), Some(value)) =
                    (expr_name(&assign.target), assign.value.as_deref())
                    && !self.referenced_names.contains(name)
                {
                    self.results.insert(value.start().to_usize());
                }
            }
            Stmt::Expr(expression) => {
                self.results.insert(expression.value.start().to_usize());
            }
            _ => {}
        }
        walk_stmt(self, statement);
    }
}

#[derive(Default)]
struct ClassCollector {
    classes: HashMap<String, ClassInfo>,
}

impl<'ast> Visitor<'ast> for ClassCollector {
    fn visit_stmt(&mut self, statement: &'ast Stmt) {
        if let Stmt::ClassDef(class) = statement {
            self.collect_class(class);
        }
        walk_stmt(self, statement);
    }
}

impl ClassCollector {
    fn collect_class(&mut self, class: &StmtClassDef) {
        let mut info = ClassInfo {
            definition_start: class.start().to_usize(),
            definition_end: class.end().to_usize(),
            ..ClassInfo::default()
        };
        if let Some(arguments) = class.arguments.as_deref() {
            info.bases.extend(
                arguments
                    .args
                    .iter()
                    .filter_map(expr_name)
                    .map(str::to_owned),
            );
        }
        for statement in &class.body {
            let name = statement_binding_name(statement).map(str::to_owned);
            if !matches!(statement, Stmt::Pass(_)) {
                info.members.push(ClassMember {
                    name,
                    start: logical_statement_start(statement),
                    end: statement.end().to_usize(),
                });
            }
            if let Stmt::FunctionDef(function) = statement {
                let kind = function
                    .decorator_list
                    .iter()
                    .filter_map(|decorator| decorator_terminal_name(&decorator.expression))
                    .find_map(|name| match name {
                        "property" => Some(MethodKind::Property),
                        "staticmethod" => Some(MethodKind::Static),
                        "classmethod" => Some(MethodKind::Class),
                        _ => None,
                    });
                let kind = match kind {
                    Some(kind) => kind,
                    None => MethodKind::Instance,
                };
                info.methods.insert(function.name.to_string(), kind);
            }
        }
        self.classes.insert(class.name.to_string(), info);
    }
}

struct BindingCollector<'classes> {
    classes: &'classes HashMap<String, ClassInfo>,
    bindings: HashMap<String, BindingState>,
}

impl BindingCollector<'_> {
    fn record(&mut self, name: &str, state: BindingState) {
        match self.bindings.get(name) {
            None => {
                self.bindings.insert(name.to_owned(), state);
            }
            Some(existing) if existing == &state => {}
            Some(_) => {
                self.bindings
                    .insert(name.to_owned(), BindingState::Ambiguous);
            }
        }
    }

    fn record_target(&mut self, target: &Expr, value: Option<&Expr>) {
        match target {
            Expr::Name(name) => {
                let state = match value {
                    Some(expression) => binding_state(expression, self.classes),
                    None => BindingState::Ambiguous,
                };
                self.record(name.id.as_str(), state);
            }
            Expr::List(list) => {
                for element in &list.elts {
                    self.record_target(element, None);
                }
            }
            Expr::Tuple(tuple) => {
                for element in &tuple.elts {
                    self.record_target(element, None);
                }
            }
            Expr::Starred(starred) => self.record_target(&starred.value, None),
            _ => {}
        }
    }
}

impl<'ast> Visitor<'ast> for BindingCollector<'_> {
    fn visit_stmt(&mut self, statement: &'ast Stmt) {
        match statement {
            Stmt::Assign(assign) => {
                for target in &assign.targets {
                    self.record_target(target, Some(&assign.value));
                }
            }
            Stmt::AnnAssign(assign) => {
                self.record_target(&assign.target, assign.value.as_deref());
            }
            Stmt::For(statement) => self.record_target(&statement.target, None),
            Stmt::With(statement) => {
                for item in &statement.items {
                    if let Some(target) = item.optional_vars.as_deref() {
                        self.record_target(target, None);
                    }
                }
            }
            Stmt::FunctionDef(function) => {
                self.record(function.name.as_str(), BindingState::Ambiguous);
                for parameter in &function.parameters {
                    self.record(parameter.name().as_str(), BindingState::Ambiguous);
                }
            }
            _ => {}
        }
        walk_stmt(self, statement);
    }
}

fn instance_class<'a>(
    expression: &'a Expr,
    classes: &'a HashMap<String, ClassInfo>,
) -> Option<&'a str> {
    let Expr::Call(call) = expression else {
        return None;
    };
    expr_name(&call.func).filter(|name| classes.contains_key(*name))
}

fn binding_state(expression: &Expr, classes: &HashMap<String, ClassInfo>) -> BindingState {
    if let Some(class) = instance_class(expression, classes) {
        return BindingState::Instance(class.to_owned());
    }
    match expression {
        Expr::Dict(_) => BindingState::Builtin(BuiltinKind::Dict),
        Expr::List(_) => BindingState::Builtin(BuiltinKind::List),
        Expr::Tuple(_) => BindingState::Builtin(BuiltinKind::Tuple),
        Expr::Call(call) => match expr_name(&call.func) {
            Some("dict") => BindingState::Builtin(BuiltinKind::Dict),
            Some("float") => BindingState::Builtin(BuiltinKind::Float),
            Some("list") => BindingState::Builtin(BuiltinKind::List),
            Some("tuple") => BindingState::Builtin(BuiltinKind::Tuple),
            _ => BindingState::Ambiguous,
        },
        _ => BindingState::Ambiguous,
    }
}

pub(crate) fn expr_name(expression: &Expr) -> Option<&str> {
    match expression {
        Expr::Name(name) => Some(name.id.as_str()),
        _ => None,
    }
}

fn decorator_terminal_name(expression: &Expr) -> Option<&str> {
    match expression {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(attribute) => Some(attribute.attr.as_str()),
        Expr::Call(call) => decorator_terminal_name(&call.func),
        _ => None,
    }
}

pub(crate) fn expression_terminal_name(expression: &Expr) -> Option<&str> {
    decorator_terminal_name(expression)
}

pub(crate) fn method_decorator_kind(expression: &Expr) -> Option<MethodKind> {
    match decorator_terminal_name(expression) {
        Some("property") => Some(MethodKind::Property),
        Some("staticmethod") => Some(MethodKind::Static),
        Some("classmethod") => Some(MethodKind::Class),
        _ => None,
    }
}

fn statement_binding_name(statement: &Stmt) -> Option<&str> {
    match statement {
        Stmt::FunctionDef(function) => Some(function.name.as_str()),
        Stmt::ClassDef(class) => Some(class.name.as_str()),
        Stmt::Assign(assign) if assign.targets.len() == 1 => {
            assign.targets.first().and_then(expr_name)
        }
        Stmt::AnnAssign(assign) => expr_name(&assign.target),
        _ => None,
    }
}

fn logical_statement_start(statement: &Stmt) -> usize {
    match statement {
        Stmt::FunctionDef(function) => function.decorator_list.first().map_or_else(
            || function.start().to_usize(),
            |decorator| decorator.start().to_usize(),
        ),
        Stmt::ClassDef(class) => class.decorator_list.first().map_or_else(
            || class.start().to_usize(),
            |decorator| decorator.start().to_usize(),
        ),
        _ => statement.start().to_usize(),
    }
}
