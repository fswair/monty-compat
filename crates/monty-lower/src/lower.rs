use std::collections::HashSet;
use std::{collections::BTreeMap, error::Error, fmt};

use ruff_python_ast::{
    CmpOp, ConversionFlag, Expr, ExprBinOp, ExprCall, ExprCompare, ExprUnaryOp,
    InterpolatedStringElement, Operator, Stmt, StmtAssert, StmtAssign, StmtClassDef, StmtDelete,
    StmtFor, StmtFunctionDef, StmtIf, StmtMatch, StmtWith, UnaryOp,
    visitor::{Visitor, walk_expr, walk_stmt},
};
use ruff_python_parser::parse_module;
use ruff_text_size::Ranged;
use serde::Serialize;

use crate::{
    CapabilityIndex,
    edit::{Edit, EditError, apply_edits},
    facts::{
        BuiltinKind, MethodKind, TypeFacts, expr_name, expression_terminal_name,
        method_decorator_kind,
    },
    match_lower::lower_match,
};

const FUNCTION_DECORATOR_FEATURE: &str = "statement.function_decorator";
const FOR_ATTRIBUTE_TARGET_FEATURE: &str = "statement.for_attribute_target";
const FOR_SUBSCRIPT_TARGET_FEATURE: &str = "statement.for_subscript_target";
const WITH_ATTRIBUTE_TARGET_FEATURE: &str = "with.attribute_target";
const WITH_SUBSCRIPT_TARGET_FEATURE: &str = "with.subscript_target";
const WITH_EMPTY_LIST_TARGET_FEATURE: &str = "with.empty_list_target";
const WITH_EMPTY_TUPLE_TARGET_FEATURE: &str = "with.empty_tuple_target";
const MATCH_FEATURES: [&str; 6] = [
    "match.literal",
    "match.or",
    "match.sequence",
    "match.mapping",
    "match.guard",
    "match.class",
];
const MAX_LOWERING_PASSES: usize = 128;
const PROTOCOL_ADD_FEATURE: &str = "protocol.add";
const PROTOCOL_CALLABLE_FEATURE: &str = "protocol.callable";
const PROTOCOL_CONTAINS_FEATURE: &str = "protocol.contains";
const PROTOCOL_GETITEM_FEATURE: &str = "protocol.getitem";
const PROTOCOL_LENGTH_FEATURE: &str = "protocol.length";
const PROTOCOL_ORDERING_FEATURE: &str = "protocol.ordering";
const PROTOCOL_SETITEM_FEATURE: &str = "protocol.setitem";
const PROTOCOL_EQUALITY_FEATURE: &str = "protocol.equality";
const PROTOCOL_HASH_FEATURE: &str = "protocol.hash";
const PROTOCOL_ITERATOR_FEATURE: &str = "protocol.iterator";
const PROTOCOL_TRUTHINESS_FEATURE: &str = "protocol.truthiness";
const PROTOCOL_ROUND_FEATURE: &str = "protocol.round";
const PROTOCOL_REVERSED_FEATURE: &str = "protocol.reversed";
const DICT_UNION_FEATURE: &str = "operator.dict_union";
const CLASS_BODY_IF_FEATURE: &str = "class.body_if";
const CLASS_BODY_TUPLE_FEATURE: &str = "class.body_tuple_assignment";
const CLASS_INHERITANCE_FEATURE: &str = "class.inheritance";
const CLASS_NESTED_FEATURE: &str = "class.nested_class";
const CLASS_SUPER_FEATURE: &str = "class.super";
const DATACLASS_FEATURE: &str = "dataclass.basic";
const FSTRING_CUSTOM_FORMAT_FEATURE: &str = "fstring.custom_format";
const FSTRING_USER_SPEC_FEATURE: &str = "fstring.user_class_spec";
const FSTRING_DEAD_INVALID_FEATURE: &str = "fstring.invalid_static_spec_dead_code";
const PERCENT_FORMAT_FEATURE: &str = "format.percent_string";
const STR_FORMAT_FEATURE: &str = "format.str_format";
const DELETE_FEATURE: &str = "statement.delete";
const DELETE_ATTRIBUTE_FEATURE: &str = "statement.delete_attribute";
const DELETE_NAME_FEATURE: &str = "statement.delete_name";
const ASSERT_MESSAGE_FEATURE: &str = "exception.assert_message";
const ELLIPSIS_FEATURE: &str = "expression.ellipsis";
const BYTES_ITERABLE_FEATURE: &str = "builtin.bytes_iterable";
const INT_UNICODE_DECIMAL_FEATURE: &str = "builtin.int_unicode_decimal";
const ITER_CALLABLE_STOP_FEATURE: &str = "builtin.iter_callable_stop_iteration";
const ASYNC_FOR_FEATURE: &str = "async.for";
const ASYNC_WITH_FEATURE: &str = "async.with";
const GENERATOR_FEATURE: &str = "statement.generator";
const YIELD_FROM_FEATURE: &str = "statement.yield_from";
const TRY_STAR_FEATURE: &str = "statement.try_star";
const RAISE_FROM_FEATURE: &str = "statement.raise_from";
const GENERATOR_LAZY_FEATURE: &str = "comprehension.generator_lazy";
const GENERATOR_TYPE_FEATURE: &str = "comprehension.generator_type";
const MAP_LAZY_FEATURE: &str = "builtin.map_lazy";
const FILTER_LAZY_FEATURE: &str = "builtin.filter_lazy";
const ENUMERATE_LAZY_FEATURE: &str = "builtin.enumerate_lazy";
const ZIP_LAZY_FEATURE: &str = "builtin.zip_lazy";
const GATHER_EXCEPTIONS_FEATURE: &str = "async.gather_return_exceptions";
const CLASS_GETATTR_FEATURE: &str = "class.getattr_hook";
const CLASS_SETATTR_FEATURE: &str = "class.setattr_hook";
const CLASS_TYPE_IDENTITY_FEATURE: &str = "class.type_identity";
const CLASS_ISINSTANCE_TYPE_FEATURE: &str = "class.isinstance_type";
const CLASS_PRIVATE_MANGLING_FEATURE: &str = "class.private_name_mangling";
const CLASS_BOUND_METHOD_TYPE_FEATURE: &str = "class.bound_method_type";
const CLASS_BOUND_METHOD_EQUALITY_FEATURE: &str = "class.bound_method_equality";

const fn binary_protocol_feature(operator: Operator) -> &'static str {
    match operator {
        Operator::Add => PROTOCOL_ADD_FEATURE,
        Operator::Sub => "protocol.binary.sub",
        Operator::Mult => "protocol.binary.mul",
        Operator::MatMult => "protocol.binary.matmul",
        Operator::Div => "protocol.binary.truediv",
        Operator::FloorDiv => "protocol.binary.floordiv",
        Operator::Mod => "protocol.binary.mod",
        Operator::Pow => "protocol.binary.pow",
        Operator::LShift => "protocol.binary.lshift",
        Operator::RShift => "protocol.binary.rshift",
        Operator::BitAnd => "protocol.binary.bitand",
        Operator::BitXor => "protocol.binary.bitxor",
        Operator::BitOr => "protocol.binary.bitor",
    }
}

const fn unary_protocol(operator: UnaryOp) -> Option<(&'static str, &'static str)> {
    match operator {
        UnaryOp::USub => Some(("protocol.unary.neg", "__neg__")),
        UnaryOp::UAdd => Some(("protocol.unary.pos", "__pos__")),
        UnaryOp::Invert => Some(("protocol.unary.invert", "__invert__")),
        UnaryOp::Not => None,
    }
}

const fn reflected_protocol(operator: Operator) -> (&'static str, &'static str) {
    match operator {
        Operator::Add => ("protocol.reflected.add", "__radd__"),
        Operator::Sub => ("protocol.reflected.sub", "__rsub__"),
        Operator::Mult => ("protocol.reflected.mul", "__rmul__"),
        Operator::MatMult => ("protocol.reflected.matmul", "__rmatmul__"),
        Operator::Div => ("protocol.reflected.truediv", "__rtruediv__"),
        Operator::FloorDiv => ("protocol.reflected.floordiv", "__rfloordiv__"),
        Operator::Mod => ("protocol.reflected.mod", "__rmod__"),
        Operator::Pow => ("protocol.reflected.pow", "__rpow__"),
        Operator::LShift => ("protocol.reflected.lshift", "__rlshift__"),
        Operator::RShift => ("protocol.reflected.rshift", "__rrshift__"),
        Operator::BitAnd => ("protocol.reflected.bitand", "__rand__"),
        Operator::BitXor => ("protocol.reflected.bitxor", "__rxor__"),
        Operator::BitOr => ("protocol.reflected.bitor", "__ror__"),
    }
}
const CLASS_ASSIGN_NAME_FEATURE: &str = "class.assign_name";
const NAN_SHARED_SEQUENCE_FEATURE: &str = "operator.nan_shared_sequence";
const CLASS_DEFAULT_REPR_FEATURE: &str = "class.default_repr_qualified";
const CLASS_ASSIGN_OBJECT_CLASS_FEATURE: &str = "class.assign_object_class";
const CLASS_BODY_COMPREHENSION_FEATURE: &str = "class.body_comprehension_scope";
const CLOSURE_LATE_BINDING_FEATURE: &str = "function.closure_late_binding";
const WITH_EXCEPTION_ARGUMENTS_FEATURE: &str = "with.exception_arguments";
const WITH_EXIT_BOUND_FEATURE: &str = "with.exit_bound_once";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum HelperKind {
    CallableIteratorList,
    ClassComprehension(String),
    DictUnion,
    GatherReturnExceptions,
    IteratorList,
    LateBoundIdentityLambdas,
    SequenceCompare,
    UserFormat,
}

#[derive(Debug, Clone)]
struct HelperDefinition {
    name: String,
    source: String,
}

/// Whether the engine changed source or deliberately left a risky seam alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticDisposition {
    Applied,
    NeedsReview,
    NotLowerable,
}

#[derive(Debug, Clone)]
struct ClassContext {
    name: String,
    edit_start: usize,
    indent: String,
}

/// One rule decision associated with a source byte range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoweringDiagnostic {
    pub rule: &'static str,
    pub disposition: DiagnosticDisposition,
    pub start: usize,
    pub end: usize,
    pub message: String,
}

/// Lowered Python source and the evidence-backed decisions that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoweringOutput {
    pub code: String,
    pub changed: bool,
    pub target_tag: String,
    pub diagnostics: Vec<LoweringDiagnostic>,
}

/// Parsing or edit-planning failure.
#[derive(Debug)]
pub enum LoweringError {
    Parse(String),
    Edit(EditError),
    NonConvergent { passes: usize },
    HelperInjection(String),
}

impl fmt::Display for LoweringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(formatter, "cannot parse Python source: {error}"),
            Self::Edit(error) => error.fmt(formatter),
            Self::NonConvergent { passes } => {
                write!(formatter, "lowering did not converge after {passes} passes")
            }
            Self::HelperInjection(message) => {
                write!(formatter, "cannot inject compatibility helpers: {message}")
            }
        }
    }
}

impl Error for LoweringError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Edit(error) => Some(error),
            Self::Parse(_) | Self::NonConvergent { .. } | Self::HelperInjection(_) => None,
        }
    }
}

impl From<EditError> for LoweringError {
    fn from(error: EditError) -> Self {
        Self::Edit(error)
    }
}

/// Lower all currently proven-safe seams for one exact Monty capability manifest.
pub fn lower_source(
    source: &str,
    capabilities: &CapabilityIndex,
) -> Result<LoweringOutput, LoweringError> {
    let mut code = source.to_owned();
    let mut diagnostics = Vec::new();
    let mut helpers = BTreeMap::new();
    for _ in 0..MAX_LOWERING_PASSES {
        let parsed =
            parse_module(&code).map_err(|error| LoweringError::Parse(error.to_string()))?;
        let module = parsed.into_syntax();
        let facts = TypeFacts::collect(&module.body);
        let mut collector = Collector::new(&code, capabilities, &facts);
        collector.visit_body(&module.body);
        for (kind, helper) in collector.helpers {
            helpers.entry(kind).or_insert(helper);
        }
        if collector.edits.is_empty() {
            diagnostics.extend(collector.diagnostics);
            code = inject_helpers(&code, helpers.values())?;
            parse_module(&code).map_err(|error| LoweringError::Parse(error.to_string()))?;
            return Ok(LoweringOutput {
                changed: code != source,
                code,
                target_tag: capabilities.target().tag.clone(),
                diagnostics,
            });
        }
        diagnostics.extend(
            collector
                .diagnostics
                .into_iter()
                .filter(|diagnostic| diagnostic.disposition == DiagnosticDisposition::Applied),
        );
        code = apply_edits(&code, collector.edits)?;
    }
    Err(LoweringError::NonConvergent {
        passes: MAX_LOWERING_PASSES,
    })
}

struct Collector<'source> {
    source: &'source str,
    capabilities: &'source CapabilityIndex,
    facts: &'source TypeFacts,
    edits: Vec<Edit>,
    diagnostics: Vec<LoweringDiagnostic>,
    temp_counter: usize,
    decorator_counter: usize,
    class_depth: usize,
    helpers: BTreeMap<HelperKind, HelperDefinition>,
    helper_counter: usize,
    class_stack: Vec<ClassContext>,
    receiver_stack: Vec<Option<String>>,
    class_name_shadows: BTreeMap<String, String>,
}

impl<'source> Collector<'source> {
    const fn new(
        source: &'source str,
        capabilities: &'source CapabilityIndex,
        facts: &'source TypeFacts,
    ) -> Self {
        Self {
            source,
            capabilities,
            facts,
            edits: Vec::new(),
            diagnostics: Vec::new(),
            temp_counter: 0,
            decorator_counter: 0,
            class_depth: 0,
            helpers: BTreeMap::new(),
            helper_counter: 0,
            class_stack: Vec::new(),
            receiver_stack: Vec::new(),
            class_name_shadows: BTreeMap::new(),
        }
    }

    fn diagnose_unlowerable_statement(&mut self, statement: &Stmt) {
        match statement {
            Stmt::For(for_statement)
                if for_statement.is_async
                    && self.capabilities.is_parse_unsupported(ASYNC_FOR_FEATURE) =>
            {
                self.not_lowerable(
                    "async_for",
                    for_statement.range(),
                    "StopAsyncIteration is absent from the target, so async iteration cannot be emulated without conflating exception types",
                );
            }
            Stmt::With(with_statement)
                if with_statement.is_async
                    && self.capabilities.is_parse_unsupported(ASYNC_WITH_FEATURE) =>
            {
                self.not_lowerable(
                    "async_with",
                    with_statement.range(),
                    "the target does not expose traceback objects required by async context-manager exception semantics",
                );
            }
            Stmt::With(with_statement)
                if !with_statement.is_async
                    && (self
                        .capabilities
                        .is_not_supported(WITH_EXCEPTION_ARGUMENTS_FEATURE)
                        || self.capabilities.is_not_supported(WITH_EXIT_BOUND_FEATURE)) =>
            {
                self.not_lowerable(
                    "with_runtime_semantics",
                    with_statement.range(),
                    "the target neither exposes traceback objects nor snapshots __exit__ before the body, so all exceptional paths cannot be preserved",
                );
            }
            Stmt::Try(try_statement)
                if try_statement.is_star
                    && self.capabilities.is_parse_unsupported(TRY_STAR_FEATURE) =>
            {
                self.not_lowerable(
                    "try_star",
                    try_statement.range(),
                    "exception-group splitting and reraising cannot be represented by ordinary try/except",
                );
            }
            Stmt::Raise(raise_statement)
                if raise_statement.cause.is_some()
                    && self.capabilities.is_not_supported(RAISE_FROM_FEATURE) =>
            {
                self.not_lowerable(
                    "raise_from",
                    raise_statement.range(),
                    "the target cannot preserve explicit __cause__ exception state",
                );
            }
            Stmt::Assign(assign_statement)
                if self
                    .capabilities
                    .is_not_supported(CLASS_ASSIGN_OBJECT_CLASS_FEATURE)
                    && assign_statement.targets.iter().any(|target| {
                        matches!(
                            target,
                            Expr::Attribute(attribute)
                                if attribute.attr.as_str() == "__class__"
                                    && self.facts.class_for_expr(&attribute.value).is_some()
                        )
                    }) =>
            {
                self.not_lowerable(
                    "class_assign_object_class",
                    assign_statement.range(),
                    "changing an existing object's class requires runtime layout and method-table mutation absent from the target",
                );
            }
            _ => {}
        }
    }

    fn lower_class_body_comprehension(&mut self, expression: &Expr) -> bool {
        if self.class_depth != 1
            || !self.receiver_stack.is_empty()
            || !self
                .capabilities
                .is_not_supported(CLASS_BODY_COMPREHENSION_FEATURE)
            || !matches!(
                expression,
                Expr::ListComp(_) | Expr::SetComp(_) | Expr::DictComp(_)
            )
        {
            return false;
        }
        let Some(source) = self.slice(expression.range()).map(str::to_owned) else {
            self.invalid_source_range("class_body_comprehension_scope", expression.range());
            return true;
        };
        let Some(helper) = self.request_helper(HelperKind::ClassComprehension(source.clone()))
        else {
            self.name_exhausted("class_body_comprehension_scope", expression.range());
            return true;
        };
        self.replace_expression(
            "class_body_comprehension_scope",
            expression.range(),
            format!("{helper}()"),
            "hoisted a module-level class comprehension into globals-only function scope",
        )
    }

    fn lower_late_bound_identity_lambdas(&mut self, expression: &Expr) -> bool {
        if self.class_depth != 0
            || !self.receiver_stack.is_empty()
            || !self
                .capabilities
                .is_not_supported(CLOSURE_LATE_BINDING_FEATURE)
        {
            return false;
        }
        let Expr::ListComp(comprehension) = expression else {
            return false;
        };
        let Some(generator) = comprehension.generators.first() else {
            return false;
        };
        if comprehension.generators.len() != 1 || generator.is_async || !generator.ifs.is_empty() {
            return false;
        }
        let (Expr::Name(target), Expr::Lambda(lambda)) =
            (&generator.target, comprehension.elt.as_ref())
        else {
            return false;
        };
        if !lambda
            .parameters
            .as_deref()
            .is_none_or(ruff_python_ast::Parameters::is_empty)
            || expr_name(&lambda.body) != Some(target.id.as_str())
        {
            return false;
        }
        let Some(iterable) = self.slice(generator.iter.range()).map(str::to_owned) else {
            self.invalid_source_range("closure_late_binding", expression.range());
            return true;
        };
        let Some(helper) = self.request_helper(HelperKind::LateBoundIdentityLambdas) else {
            self.name_exhausted("closure_late_binding", expression.range());
            return true;
        };
        self.replace_expression(
            "closure_late_binding",
            expression.range(),
            format!("{helper}(({iterable}))"),
            "lowered identity lambdas over one comprehension variable through a shared mutable cell",
        )
    }

    fn lower_dead_generator_expression(&mut self, expression: &Expr) -> bool {
        if !self.capabilities.is_not_supported(GENERATOR_LAZY_FEATURE)
            || !self.facts.is_dead_result(expression)
        {
            return false;
        }
        let Expr::Generator(generator) = expression else {
            return false;
        };
        let Some(outermost) = generator.generators.first() else {
            return false;
        };
        if outermost.is_async {
            return false;
        }
        let Some(setup) = self.dead_iterable_setup(&outermost.iter) else {
            return false;
        };
        self.replace_expression(
            "dead_generator_expression",
            expression.range(),
            format!("({setup},)"),
            "preserved outer-iterator setup while leaving a statically dead generator body suspended",
        )
    }

    fn diagnose_unlowerable_expression(&mut self, expression: &Expr) -> bool {
        match expression {
            Expr::BinOp(binary) => {
                let (feature, method) = reflected_protocol(binary.op);
                if self.capabilities.is_not_supported(feature)
                    && let Some(class) = self.facts.class_for_expr(&binary.right)
                    && self.facts.resolves_method(class, method).is_some()
                {
                    self.not_lowerable(
                        "protocol_reflected_binary",
                        binary.range(),
                        "reflected binary dispatch precedence and NotImplemented fallback cannot be preserved safely",
                    );
                    return true;
                }
                false
            }
            Expr::Call(call)
                if call.arguments.args.len() == 1 && call.arguments.keywords.is_empty() =>
            {
                let Some(name) = expr_name(&call.func) else {
                    return false;
                };
                let Some((feature, method)) = (match name {
                    "int" => Some(("protocol.conversion.int", "__int__")),
                    "float" => Some(("protocol.conversion.float", "__float__")),
                    "hex" | "oct" | "bin" => Some(("protocol.conversion.index", "__index__")),
                    _ => None,
                }) else {
                    return false;
                };
                let Some(argument) = call.arguments.args.first() else {
                    return false;
                };
                if self.capabilities.is_not_supported(feature)
                    && let Some(class) = self.facts.class_for_expr(argument)
                    && self.facts.resolves_method(class, method).is_some()
                {
                    self.not_lowerable(
                        "protocol_conversion",
                        call.range(),
                        "direct conversion-method calls would bypass Python's required result-type validation",
                    );
                    return true;
                }
                false
            }
            Expr::Yield(yield_expression)
                if self.capabilities.is_parse_unsupported(GENERATOR_FEATURE) =>
            {
                self.not_lowerable(
                    "generator_yield",
                    yield_expression.range(),
                    "eager collection would change generator suspension and side-effect timing",
                );
                true
            }
            Expr::YieldFrom(yield_from)
                if self.capabilities.is_parse_unsupported(YIELD_FROM_FEATURE) =>
            {
                self.not_lowerable(
                    "yield_from",
                    yield_from.range(),
                    "yield-from delegation semantics cannot be represented without generators",
                );
                true
            }
            Expr::Generator(generator)
                if self.capabilities.is_not_supported(GENERATOR_LAZY_FEATURE)
                    || self.capabilities.is_not_supported(GENERATOR_TYPE_FEATURE) =>
            {
                self.not_lowerable(
                    "generator_expression",
                    generator.range(),
                    "materializing a generator expression would change laziness, identity, and exception timing",
                );
                true
            }
            Expr::ListComp(comprehension)
                if self.class_depth > 0
                    && self.receiver_stack.is_empty()
                    && self
                        .capabilities
                        .is_not_supported(CLASS_BODY_COMPREHENSION_FEATURE) =>
            {
                self.not_lowerable(
                    "class_body_comprehension_scope",
                    comprehension.range(),
                    "the target leaks class locals into comprehension scope and has no globals-only lookup primitive",
                );
                true
            }
            Expr::SetComp(comprehension)
                if self.class_depth > 0
                    && self.receiver_stack.is_empty()
                    && self
                        .capabilities
                        .is_not_supported(CLASS_BODY_COMPREHENSION_FEATURE) =>
            {
                self.not_lowerable(
                    "class_body_comprehension_scope",
                    comprehension.range(),
                    "the target leaks class locals into comprehension scope and has no globals-only lookup primitive",
                );
                true
            }
            Expr::DictComp(comprehension)
                if self.class_depth > 0
                    && self.receiver_stack.is_empty()
                    && self
                        .capabilities
                        .is_not_supported(CLASS_BODY_COMPREHENSION_FEATURE) =>
            {
                self.not_lowerable(
                    "class_body_comprehension_scope",
                    comprehension.range(),
                    "the target leaks class locals into comprehension scope and has no globals-only lookup primitive",
                );
                true
            }
            Expr::ListComp(comprehension)
                if self
                    .capabilities
                    .is_not_supported(CLOSURE_LATE_BINDING_FEATURE)
                    && expression_contains_lambda(expression) =>
            {
                self.not_lowerable(
                    "closure_late_binding",
                    comprehension.range(),
                    "the target captures comprehension variables by value and exposes no mutable closure-cell primitive",
                );
                true
            }
            _ => false,
        }
    }

    fn lower_function_decorator(&mut self, function: &StmtFunctionDef) {
        if function.decorator_list.is_empty()
            || !self
                .capabilities
                .is_parse_unsupported(FUNCTION_DECORATOR_FEATURE)
        {
            return;
        }
        if self.class_depth > 0
            && self.receiver_stack.is_empty()
            && self.lower_descriptor_decorator(function)
        {
            return;
        }

        let (Some(first_decorator), Some(last_decorator)) = (
            function.decorator_list.first(),
            function.decorator_list.last(),
        ) else {
            return;
        };
        let Some(delete_start) = line_start(self.source, to_offset(first_decorator.start())) else {
            self.invalid_source_range("function_decorator", function.range());
            return;
        };
        let Some(delete_end) =
            line_end_including_newline(self.source, to_offset(last_decorator.end()))
        else {
            self.invalid_source_range("function_decorator", function.range());
            return;
        };
        let Some(indent) = self
            .source
            .get(delete_start..to_offset(first_decorator.start()))
        else {
            self.invalid_source_range("function_decorator", function.range());
            return;
        };
        if !indent.chars().all(is_indent_char) {
            return;
        }

        // Python evaluates decorator expressions top-to-bottom before it creates
        // the function, then applies their values bottom-to-top. Capturing each
        // expression in a fresh name preserves both parts of that ordering.
        let mut prelude = String::new();
        let mut decorators = Vec::with_capacity(function.decorator_list.len());
        for decorator in &function.decorator_list {
            let Some(expression) = self.slice(decorator.expression.range()).map(str::to_owned)
            else {
                self.invalid_source_range("function_decorator", function.range());
                return;
            };
            let Some(temp) = self.next_decorator_name() else {
                self.name_exhausted("function_decorator", function.range());
                return;
            };
            prelude.push_str(indent);
            prelude.push_str(&temp);
            prelude.push_str(" = ");
            prelude.push_str(&expression);
            prelude.push('\n');
            decorators.push(temp);
        }
        let mut applied = function.name.to_string();
        for decorator in decorators.iter().rev() {
            applied = format!("{decorator}({applied})");
        }
        let Some(insertion) = statement_line_end(self.source, to_offset(function.end())) else {
            self.invalid_source_range("function_decorator", function.range());
            return;
        };
        let Some(function_tail) = self.source.get(to_offset(function.end())..insertion) else {
            self.invalid_source_range("function_decorator", function.range());
            return;
        };
        let prefix = if function_tail.contains('\n') {
            ""
        } else {
            "\n"
        };
        self.edits.push(Edit {
            start: delete_start,
            end: delete_end,
            replacement: prelude,
        });
        self.edits.push(Edit {
            start: insertion,
            end: insertion,
            replacement: format!("{prefix}{indent}{} = {applied}\n", function.name),
        });
        self.diagnostics.push(LoweringDiagnostic {
            rule: "function_decorator",
            disposition: DiagnosticDisposition::Applied,
            start: delete_start,
            end: to_offset(function.end()),
            message: format!(
                "lowered decorators on function '{}' to explicit assignment",
                function.name
            ),
        });
    }

    fn lower_descriptor_decorator(&mut self, function: &StmtFunctionDef) -> bool {
        let descriptor_count = function
            .decorator_list
            .iter()
            .filter(|decorator| method_decorator_kind(&decorator.expression).is_some())
            .count();
        if descriptor_count == 0 {
            return false;
        }
        if descriptor_count != 1 || function.decorator_list.len() != 1 {
            self.diagnostics.push(LoweringDiagnostic {
                rule: "class_method_decorator",
                disposition: DiagnosticDisposition::NeedsReview,
                start: to_offset(function.start()),
                end: to_offset(function.end()),
                message: "descriptor decorators combined with other decorators cannot be reordered safely"
                    .to_owned(),
            });
            return true;
        }
        let Some(decorator) = function.decorator_list.first() else {
            return true;
        };
        let Some(start) = line_start(self.source, to_offset(decorator.start())) else {
            self.invalid_source_range("class_method_decorator", function.range());
            return true;
        };
        let Some(end) = line_end_including_newline(self.source, to_offset(decorator.end())) else {
            self.invalid_source_range("class_method_decorator", function.range());
            return true;
        };
        self.edits.push(Edit {
            start,
            end,
            replacement: String::new(),
        });
        self.diagnostics.push(LoweringDiagnostic {
            rule: "class_method_decorator",
            disposition: DiagnosticDisposition::Applied,
            start,
            end,
            message: "lowered property/staticmethod/classmethod through resolved access sites"
                .to_owned(),
        });
        true
    }

    fn lower_for_target(&mut self, statement: &StmtFor) {
        if statement.is_async {
            return;
        }
        let Some(feature) = complex_target_feature(
            &statement.target,
            FOR_ATTRIBUTE_TARGET_FEATURE,
            FOR_SUBSCRIPT_TARGET_FEATURE,
        ) else {
            return;
        };
        if !self.capabilities.is_parse_unsupported(feature) {
            return;
        }
        let Some((body_start, indent)) = self.block_body_insertion(&statement.body) else {
            self.needs_block_suite("for_complex_target", statement.range());
            return;
        };

        let target_range = statement.target.range();
        let Some(target_source) = self.slice(target_range).map(str::to_owned) else {
            self.invalid_source_range("for_complex_target", statement.range());
            return;
        };
        let Some(temp) = self.next_temp_name() else {
            self.name_exhausted("for_complex_target", statement.range());
            return;
        };
        self.edits.push(Edit {
            start: to_offset(target_range.start()),
            end: to_offset(target_range.end()),
            replacement: temp.clone(),
        });
        self.edits.push(Edit {
            start: body_start,
            end: body_start,
            replacement: format!("{target_source} = {temp}\n{indent}"),
        });
        self.diagnostics.push(LoweringDiagnostic {
            rule: "for_complex_target",
            disposition: DiagnosticDisposition::Applied,
            start: to_offset(target_range.start()),
            end: to_offset(target_range.end()),
            message: "lowered attribute/subscript loop target through a temporary name".to_owned(),
        });
    }

    fn lower_with_target(&mut self, statement: &StmtWith) {
        if statement.is_async {
            return;
        }
        let lowerable_items: Vec<_> = statement
            .items
            .iter()
            .filter_map(|item| {
                let target = item.optional_vars.as_deref()?;
                let feature = with_target_feature(target)?;
                self.capabilities
                    .is_parse_unsupported(feature)
                    .then_some(target)
            })
            .collect();
        if lowerable_items.is_empty() {
            return;
        }
        if statement.items.len() != 1 || lowerable_items.len() != 1 {
            self.diagnostics.push(LoweringDiagnostic {
                rule: "with_complex_target",
                disposition: DiagnosticDisposition::NeedsReview,
                start: to_offset(statement.start()),
                end: to_offset(statement.end()),
                message: "multiple context managers require interleaved target assignment to preserve enter order"
                    .to_owned(),
            });
            return;
        }
        let Some((body_start, indent)) = self.block_body_insertion(&statement.body) else {
            self.needs_block_suite("with_complex_target", statement.range());
            return;
        };

        let Some(target) = lowerable_items.first().copied() else {
            self.not_lowerable(
                "with_complex_target",
                statement.range(),
                "with target selection became empty during lowering",
            );
            return;
        };
        let target_range = target.range();
        let Some(target_source) = self.slice(target_range).map(str::to_owned) else {
            self.invalid_source_range("with_complex_target", statement.range());
            return;
        };
        let Some(temp) = self.next_temp_name() else {
            self.name_exhausted("with_complex_target", statement.range());
            return;
        };
        self.edits.push(Edit {
            start: to_offset(target_range.start()),
            end: to_offset(target_range.end()),
            replacement: temp.clone(),
        });
        self.edits.push(Edit {
            start: body_start,
            end: body_start,
            replacement: format!("{target_source} = {temp}\n{indent}"),
        });
        self.diagnostics.push(LoweringDiagnostic {
            rule: "with_complex_target",
            disposition: DiagnosticDisposition::Applied,
            start: to_offset(target_range.start()),
            end: to_offset(target_range.end()),
            message: "lowered with-as target through a temporary assignment".to_owned(),
        });
    }

    fn lower_with_exit_bound_once(&mut self, statement: &StmtWith) -> bool {
        if statement.is_async
            || !self.capabilities.is_not_supported(WITH_EXIT_BOUND_FEATURE)
            || statement.items.len() != 1
            || !self.with_body_is_statically_non_raising(&statement.body)
        {
            return false;
        }
        let Some(item) = statement.items.first() else {
            return false;
        };
        if self.facts.class_for_expr(&item.context_expr).is_none()
            || item
                .optional_vars
                .as_deref()
                .is_some_and(|target| !matches!(target, Expr::Name(_)))
        {
            return false;
        }
        let Some(context) = self.slice(item.context_expr.range()).map(str::to_owned) else {
            self.invalid_source_range("with_exit_bound_once", statement.range());
            return true;
        };
        let Some(line_start) = line_start(self.source, to_offset(statement.start())) else {
            self.invalid_source_range("with_exit_bound_once", statement.range());
            return true;
        };
        let Some(indent) = self
            .source
            .get(line_start..to_offset(statement.start()))
            .map(str::to_owned)
        else {
            self.invalid_source_range("with_exit_bound_once", statement.range());
            return true;
        };
        let Some(body) = reindent_suite(self.source, &statement.body, &indent) else {
            self.needs_block_suite("with_exit_bound_once", statement.range());
            return true;
        };
        let (Some(manager), Some(exit_method)) = (self.next_temp_name(), self.next_temp_name())
        else {
            self.name_exhausted("with_exit_bound_once", statement.range());
            return true;
        };

        let mut replacement =
            format!("{manager} = {context}\n{indent}{exit_method} = {manager}.__exit__\n{indent}");
        if let Some(target) = item.optional_vars.as_deref() {
            let Some(target) = self.slice(target.range()) else {
                self.invalid_source_range("with_exit_bound_once", statement.range());
                return true;
            };
            replacement.push_str(target);
            replacement.push_str(" = ");
            replacement.push_str(&manager);
            replacement.push_str(".__enter__()\n");
        } else {
            replacement.push_str(&manager);
            replacement.push_str(".__enter__()\n");
        }
        replacement.push_str(&body);
        replacement.push_str(&indent);
        replacement.push_str(&exit_method);
        replacement.push_str("(None, None, None)");
        self.edits.push(Edit {
            start: to_offset(statement.start()),
            end: to_offset(statement.end()),
            replacement,
        });
        self.applied(
            "with_exit_bound_once",
            statement.range(),
            "lowered a statically non-raising with body after snapshotting the bound exit method",
        );
        true
    }

    fn lower_async_with_non_raising_return(&mut self, statement: &StmtWith) -> bool {
        if !statement.is_async
            || !self.capabilities.is_parse_unsupported(ASYNC_WITH_FEATURE)
            || statement.items.len() != 1
            || statement.body.len() != 1
        {
            return false;
        }
        let (Some(item), Some(Stmt::Return(return_statement))) =
            (statement.items.first(), statement.body.first())
        else {
            return false;
        };
        let Some(return_value) = return_statement.value.as_deref() else {
            return false;
        };
        if !self.expression_is_statically_non_raising(return_value)
            || self.facts.class_for_expr(&item.context_expr).is_none()
            || item
                .optional_vars
                .as_deref()
                .is_some_and(|target| !matches!(target, Expr::Name(_)))
        {
            return false;
        }
        let Some(context) = self.slice(item.context_expr.range()).map(str::to_owned) else {
            self.invalid_source_range("async_with_non_raising_return", statement.range());
            return true;
        };
        let Some(return_value) = self.slice(return_value.range()).map(str::to_owned) else {
            self.invalid_source_range("async_with_non_raising_return", statement.range());
            return true;
        };
        let Some(line_start) = line_start(self.source, to_offset(statement.start())) else {
            self.invalid_source_range("async_with_non_raising_return", statement.range());
            return true;
        };
        let Some(indent) = self
            .source
            .get(line_start..to_offset(statement.start()))
            .map(str::to_owned)
        else {
            self.invalid_source_range("async_with_non_raising_return", statement.range());
            return true;
        };
        let (Some(manager), Some(exit_method), Some(result)) = (
            self.next_temp_name(),
            self.next_temp_name(),
            self.next_temp_name(),
        ) else {
            self.name_exhausted("async_with_non_raising_return", statement.range());
            return true;
        };

        let mut replacement =
            format!("{manager} = {context}\n{indent}{exit_method} = {manager}.__aexit__\n{indent}");
        if let Some(target) = item.optional_vars.as_deref() {
            let Some(target) = self.slice(target.range()) else {
                self.invalid_source_range("async_with_non_raising_return", statement.range());
                return true;
            };
            replacement.push_str(target);
            replacement.push_str(" = await ");
            replacement.push_str(&manager);
            replacement.push_str(".__aenter__()\n");
        } else {
            replacement.push_str("await ");
            replacement.push_str(&manager);
            replacement.push_str(".__aenter__()\n");
        }
        replacement.push_str(&indent);
        replacement.push_str(&result);
        replacement.push_str(" = ");
        replacement.push_str(&return_value);
        replacement.push('\n');
        replacement.push_str(&indent);
        replacement.push_str("await ");
        replacement.push_str(&exit_method);
        replacement.push_str("(None, None, None)\n");
        replacement.push_str(&indent);
        replacement.push_str("return ");
        replacement.push_str(&result);
        self.edits.push(Edit {
            start: to_offset(statement.start()),
            end: to_offset(statement.end()),
            replacement,
        });
        self.applied(
            "async_with_non_raising_return",
            statement.range(),
            "lowered a non-raising async-with return after snapshotting and awaiting the exit method",
        );
        true
    }

    fn with_body_is_statically_non_raising(&self, body: &[Stmt]) -> bool {
        !body.is_empty()
            && body.iter().all(|statement| match statement {
                Stmt::Pass(_) => true,
                Stmt::Assign(assign) => {
                    self.expression_is_statically_non_raising(&assign.value)
                        && !assign.targets.is_empty()
                        && assign.targets.iter().all(|target| {
                            matches!(target, Expr::Attribute(attribute) if self
                                .attribute_receiver_class(&attribute.value)
                                .is_some())
                        })
                }
                _ => false,
            })
    }

    fn expression_is_statically_non_raising(&self, expression: &Expr) -> bool {
        match expression {
            Expr::Name(name) => self.facts.is_name_bound(name.id.as_str()),
            Expr::BooleanLiteral(_)
            | Expr::BytesLiteral(_)
            | Expr::NoneLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::StringLiteral(_) => true,
            _ => false,
        }
    }

    fn lower_delete(&mut self, statement: &StmtDelete) -> bool {
        if ![
            DELETE_FEATURE,
            DELETE_ATTRIBUTE_FEATURE,
            DELETE_NAME_FEATURE,
        ]
        .iter()
        .any(|feature| self.capabilities.is_parse_unsupported(feature))
        {
            return false;
        }

        let mut replacements = Vec::with_capacity(statement.targets.len());
        for target in &statement.targets {
            let replacement = match target {
                Expr::Subscript(subscript)
                    if !matches!(subscript.slice.as_ref(), Expr::Slice(_)) =>
                {
                    let Some(receiver) = self.slice(subscript.value.range()) else {
                        self.invalid_source_range("delete_subscript", target.range());
                        return true;
                    };
                    let Some(key) = self.slice(subscript.slice.range()) else {
                        self.invalid_source_range("delete_subscript", target.range());
                        return true;
                    };
                    if self.facts.builtin_for_expr(&subscript.value).is_some() {
                        format!("({receiver}).pop({key})")
                    } else if let Some(class) = self.facts.class_for_expr(&subscript.value) {
                        if self.facts.resolves_method(class, "__delitem__").is_some() {
                            format!("({receiver}).__delitem__({key})")
                        } else {
                            self.not_lowerable(
                                "delete_subscript",
                                target.range(),
                                "subscript receiver has no statically resolved deletion protocol",
                            );
                            return true;
                        }
                    } else {
                        self.not_lowerable(
                            "delete_subscript",
                            target.range(),
                            "subscript receiver type is not statically known",
                        );
                        return true;
                    }
                }
                Expr::Subscript(_) => {
                    self.not_lowerable(
                        "delete_subscript",
                        target.range(),
                        "slice deletion cannot be expressed through pop or __delitem__ safely",
                    );
                    return true;
                }
                Expr::Attribute(_) => {
                    self.not_lowerable(
                        "delete_attribute",
                        target.range(),
                        "attribute deletion cannot be emulated without changing object layout",
                    );
                    return true;
                }
                Expr::Name(_) => {
                    self.not_lowerable(
                        "delete_name",
                        target.range(),
                        "name deletion cannot be emulated while preserving Python scope semantics",
                    );
                    return true;
                }
                _ => {
                    self.not_lowerable(
                        "delete_target",
                        target.range(),
                        "delete target has no semantics-preserving lowering",
                    );
                    return true;
                }
            };
            replacements.push(replacement);
        }

        if replacements.is_empty() {
            self.not_lowerable(
                "delete_statement",
                statement.range(),
                "delete statement has no targets",
            );
            return true;
        }
        self.edits.push(Edit {
            start: to_offset(statement.start()),
            end: to_offset(statement.end()),
            replacement: replacements.join("; "),
        });
        self.diagnostics.push(LoweringDiagnostic {
            rule: "delete_subscript",
            disposition: DiagnosticDisposition::Applied,
            start: to_offset(statement.start()),
            end: to_offset(statement.end()),
            message: "lowered statically resolved item deletion to pop or __delitem__".to_owned(),
        });
        true
    }

    fn lower_assert(&mut self, statement: &StmtAssert) -> bool {
        if !self.capabilities.is_not_supported(ASSERT_MESSAGE_FEATURE) {
            return false;
        }
        let Some(test) = self.slice(statement.test.range()).map(str::to_owned) else {
            self.invalid_source_range("assert_message", statement.range());
            return true;
        };
        let message = match statement.msg.as_deref() {
            Some(message) => {
                let Some(source) = self.slice(message.range()) else {
                    self.invalid_source_range("assert_message", statement.range());
                    return true;
                };
                format!("({source})")
            }
            None => String::new(),
        };
        let statement_start = to_offset(statement.start());
        let Some(start) = line_start(self.source, statement_start) else {
            self.invalid_source_range("assert_message", statement.range());
            return true;
        };
        let Some(end) = line_end_including_newline(self.source, to_offset(statement.end())) else {
            self.invalid_source_range("assert_message", statement.range());
            return true;
        };
        let Some(indent) = self.source.get(start..statement_start) else {
            self.invalid_source_range("assert_message", statement.range());
            return true;
        };
        if !indent.chars().all(is_indent_char) {
            self.not_lowerable(
                "assert_message",
                statement.range(),
                "assert statement indentation is not structurally rewriteable",
            );
            return true;
        }
        let mut replacement = String::new();
        push_python_line(&mut replacement, indent, &format!("if not ({test}):"));
        push_python_line(
            &mut replacement,
            &format!("{indent}    "),
            &format!("raise AssertionError({message})"),
        );
        self.edits.push(Edit {
            start,
            end,
            replacement,
        });
        self.applied(
            "assert_message",
            statement.range(),
            "lowered assert to an explicit empty-or-user-message AssertionError",
        );
        true
    }

    fn lower_match_statement(&mut self, statement: &StmtMatch) -> bool {
        if !MATCH_FEATURES
            .iter()
            .any(|feature| self.capabilities.is_parse_unsupported(feature))
        {
            return false;
        }
        match lower_match(self.source, statement) {
            Ok(edit) => {
                self.edits.push(Edit {
                    start: edit.start,
                    end: edit.end,
                    replacement: edit.replacement,
                });
                self.diagnostics.push(LoweringDiagnostic {
                    rule: "match_statement",
                    disposition: DiagnosticDisposition::Applied,
                    start: to_offset(statement.start()),
                    end: to_offset(statement.end()),
                    message: "lowered structural pattern matching to guarded if statements"
                        .to_owned(),
                });
                true
            }
            Err(error) => {
                self.diagnostics.push(LoweringDiagnostic {
                    rule: "match_statement",
                    disposition: DiagnosticDisposition::NeedsReview,
                    start: to_offset(statement.start()),
                    end: to_offset(statement.end()),
                    message: error.to_string(),
                });
                false
            }
        }
    }

    fn lower_class_definition(&mut self, class: &StmtClassDef) {
        self.lower_dataclass(class);
        self.lower_class_inheritance(class);
    }

    fn lower_dataclass_import(&mut self, import: &ruff_python_ast::StmtImportFrom) -> bool {
        if !self.capabilities.is_not_supported(DATACLASS_FEATURE)
            || import
                .module
                .as_ref()
                .map(ruff_python_ast::Identifier::as_str)
                != Some("dataclasses")
            || import.names.len() != 1
            || import.names.first().map(|alias| alias.name.as_str()) != Some("dataclass")
        {
            return false;
        }
        let start_offset = to_offset(import.start());
        let Some(start) = line_start(self.source, start_offset) else {
            self.invalid_source_range("dataclass_import", import.range());
            return false;
        };
        let Some(end) = line_end_including_newline(self.source, to_offset(import.end())) else {
            self.invalid_source_range("dataclass_import", import.range());
            return false;
        };
        self.edits.push(Edit {
            start,
            end,
            replacement: String::new(),
        });
        self.applied(
            "dataclass_import",
            import.range(),
            "removed the unsupported dataclasses import after lowering @dataclass",
        );
        true
    }

    fn lower_dataclass(&mut self, class: &StmtClassDef) {
        if !self.capabilities.is_not_supported(DATACLASS_FEATURE) {
            return;
        }
        let dataclass_decorators: Vec<_> = class
            .decorator_list
            .iter()
            .filter(|decorator| {
                expression_terminal_name(&decorator.expression) == Some("dataclass")
            })
            .collect();
        if dataclass_decorators.is_empty() {
            return;
        }
        if dataclass_decorators.len() != 1 || class.decorator_list.len() != 1 {
            self.not_lowerable(
                "dataclass",
                class.range(),
                "dataclass combined with other class decorators requires runtime descriptor semantics",
            );
            return;
        }
        if class.body.iter().any(|statement| {
            matches!(statement, Stmt::FunctionDef(function) if function.name.as_str() == "__init__")
        }) {
            self.not_lowerable(
                "dataclass",
                class.range(),
                "dataclass with a user-defined __init__ is not automatically rewritten",
            );
            return;
        }

        let mut fields = Vec::new();
        for statement in &class.body {
            let Stmt::AnnAssign(field) = statement else {
                continue;
            };
            let Expr::Name(name) = field.target.as_ref() else {
                self.not_lowerable(
                    "dataclass",
                    class.range(),
                    "dataclass fields must use simple names",
                );
                return;
            };
            let default = if let Some(value) = field.value.as_deref() {
                let Some(source) = self.slice(value.range()).map(str::to_owned) else {
                    self.invalid_source_range("dataclass", field.range());
                    return;
                };
                Some(source)
            } else {
                None
            };
            fields.push((name.id.to_string(), default));
        }
        let mut saw_default = false;
        for (_, default) in &fields {
            if default.is_some() {
                saw_default = true;
            } else if saw_default {
                self.not_lowerable(
                    "dataclass",
                    class.range(),
                    "non-default dataclass field follows a default field",
                );
                return;
            }
        }

        let Some(decorator) = dataclass_decorators.first() else {
            return;
        };
        let Some(delete_start) = line_start(self.source, to_offset(decorator.start())) else {
            self.invalid_source_range("dataclass", class.range());
            return;
        };
        let Some(delete_end) = line_end_including_newline(self.source, to_offset(decorator.end()))
        else {
            self.invalid_source_range("dataclass", class.range());
            return;
        };
        let Some(last_statement) = class.body.last() else {
            self.not_lowerable("dataclass", class.range(), "dataclass body is empty");
            return;
        };
        let Some(insertion) =
            line_end_including_newline(self.source, to_offset(last_statement.end()))
        else {
            self.invalid_source_range("dataclass", class.range());
            return;
        };
        let Some((_, indent)) = self.block_body_insertion(&class.body) else {
            self.not_lowerable(
                "dataclass",
                class.range(),
                "single-line dataclass bodies are not automatically rewritten",
            );
            return;
        };
        let mut generated = String::new();
        generated.push_str(&indent);
        generated.push_str("def __init__(self");
        for (name, default) in &fields {
            generated.push_str(", ");
            generated.push_str(name);
            if let Some(default) = default {
                generated.push('=');
                generated.push_str(default);
            }
        }
        generated.push_str("):\n");
        let nested = format!("{indent}    ");
        if fields.is_empty() {
            generated.push_str(&nested);
            generated.push_str("pass\n");
        } else {
            for (name, _) in &fields {
                generated.push_str(&nested);
                generated.push_str("self.");
                generated.push_str(name);
                generated.push_str(" = ");
                generated.push_str(name);
                generated.push('\n');
            }
        }
        generated.push_str(&indent);
        generated.push_str("def __repr__(self):\n");
        generated.push_str(&nested);
        generated.push_str("return ");
        let repr_prefix = format!("{:?}", format!("{}(", class.name));
        generated.push_str(&repr_prefix);
        for (index, (name, _)) in fields.iter().enumerate() {
            let separator = if index == 0 { "" } else { ", " };
            generated.push_str(" + ");
            let repr_separator = format!("{separator:?}");
            generated.push_str(&repr_separator);
            generated.push_str(" + ");
            let repr_name = format!("{name:?}");
            generated.push_str(&repr_name);
            generated.push_str(" + '=' + repr(self.");
            generated.push_str(name);
            generated.push(')');
        }
        generated.push_str(" + ')'\n");
        generated.push_str(&indent);
        generated.push_str("def __eq__(self, other):\n");
        generated.push_str(&nested);
        generated.push_str("return isinstance(other, ");
        generated.push_str(class.name.as_str());
        generated.push(')');
        for (name, _) in &fields {
            generated.push_str(" and self.");
            generated.push_str(name);
            generated.push_str(" == other.");
            generated.push_str(name);
        }
        generated.push('\n');

        self.edits.push(Edit {
            start: delete_start,
            end: delete_end,
            replacement: String::new(),
        });
        self.edits.push(Edit {
            start: insertion,
            end: insertion,
            replacement: generated,
        });
        self.applied(
            "dataclass",
            class.range(),
            "lowered basic dataclass initialization, repr, and equality to explicit methods",
        );
    }

    fn lower_class_inheritance(&mut self, class: &StmtClassDef) {
        if !self
            .capabilities
            .is_parse_unsupported(CLASS_INHERITANCE_FEATURE)
        {
            return;
        }
        let Some(arguments) = class.arguments.as_deref() else {
            return;
        };
        if arguments.args.len() != 1 || !arguments.keywords.is_empty() {
            self.not_lowerable(
                "class_inheritance",
                class.range(),
                "only a single statically resolved base class can be flattened safely",
            );
            return;
        }
        let Some(base_name) = arguments.args.first().and_then(expr_name) else {
            self.not_lowerable(
                "class_inheritance",
                class.range(),
                "dynamic base-class expressions cannot be flattened safely",
            );
            return;
        };
        let Some(base) = self.facts.class(base_name) else {
            self.not_lowerable(
                "class_inheritance",
                class.range(),
                "external base classes cannot be flattened without their source body",
            );
            return;
        };
        let Some(first_child) = class.body.first() else {
            self.not_lowerable("class_inheritance", class.range(), "class body is empty");
            return;
        };
        let child_start = logical_statement_start(first_child);
        let Some(insertion) = line_start(self.source, child_start) else {
            self.invalid_source_range("class_inheritance", class.range());
            return;
        };
        let Some(child_indent) = self.source.get(insertion..child_start) else {
            self.invalid_source_range("class_inheritance", class.range());
            return;
        };
        let child_names: HashSet<_> = class
            .body
            .iter()
            .filter_map(statement_binding_name)
            .collect();
        let mut inherited = String::new();
        for member in base.members() {
            if member
                .name
                .as_deref()
                .is_some_and(|name| child_names.contains(name))
            {
                continue;
            }
            let Some(member_line_start) = line_start(self.source, member.start) else {
                self.invalid_source_range("class_inheritance", class.range());
                return;
            };
            let Some(member_line_end) = line_end_including_newline(self.source, member.end) else {
                self.invalid_source_range("class_inheritance", class.range());
                return;
            };
            let Some(old_indent) = self.source.get(member_line_start..member.start) else {
                self.invalid_source_range("class_inheritance", class.range());
                return;
            };
            let Some(block) = self.source.get(member_line_start..member_line_end) else {
                self.invalid_source_range("class_inheritance", class.range());
                return;
            };
            inherited.push_str(&reindent_text(block, old_indent, child_indent));
        }
        self.edits.push(Edit {
            start: to_offset(arguments.start()),
            end: to_offset(arguments.end()),
            replacement: String::new(),
        });
        if !inherited.is_empty() {
            self.edits.push(Edit {
                start: insertion,
                end: insertion,
                replacement: inherited,
            });
        }
        self.applied(
            "class_inheritance",
            class.range(),
            "flattened a statically resolved single base class into the child",
        );
    }

    fn lower_class_body_if(&mut self, statement: &StmtIf) -> bool {
        if self.class_stack.is_empty()
            || !self.receiver_stack.is_empty()
            || !self
                .capabilities
                .is_parse_unsupported(CLASS_BODY_IF_FEATURE)
        {
            return false;
        }
        let Expr::BooleanLiteral(condition) = statement.test.as_ref() else {
            self.not_lowerable(
                "class_body_if",
                statement.range(),
                "dynamic class-body conditions cannot access a synthetic class namespace safely",
            );
            return false;
        };
        let selected = if condition.value {
            Some(statement.body.as_slice())
        } else if statement.elif_else_clauses.is_empty() {
            None
        } else if statement.elif_else_clauses.len() == 1 {
            statement
                .elif_else_clauses
                .first()
                .filter(|clause| clause.test.is_none())
                .map(|clause| clause.body.as_slice())
        } else {
            self.not_lowerable(
                "class_body_if",
                statement.range(),
                "class-body elif chains require dynamic namespace execution",
            );
            return false;
        };
        let statement_start = to_offset(statement.start());
        let Some(edit_start) = line_start(self.source, statement_start) else {
            self.invalid_source_range("class_body_if", statement.range());
            return false;
        };
        let Some(edit_end) = line_end_including_newline(self.source, to_offset(statement.end()))
        else {
            self.invalid_source_range("class_body_if", statement.range());
            return false;
        };
        let Some(indent) = self.source.get(edit_start..statement_start) else {
            self.invalid_source_range("class_body_if", statement.range());
            return false;
        };
        let replacement = if let Some(body) = selected {
            if let Some(source) = reindent_suite(self.source, body, indent) {
                source
            } else {
                self.invalid_source_range("class_body_if", statement.range());
                return false;
            }
        } else {
            String::new()
        };
        self.edits.push(Edit {
            start: edit_start,
            end: edit_end,
            replacement,
        });
        self.applied(
            "class_body_if",
            statement.range(),
            "constant-folded a class-body if statement",
        );
        true
    }

    fn lower_dead_module_if(&mut self, statement: &StmtIf) -> bool {
        if !self.class_stack.is_empty()
            || !self.receiver_stack.is_empty()
            || !self
                .capabilities
                .is_parse_unsupported(FSTRING_DEAD_INVALID_FEATURE)
            || !matches!(statement.test.as_ref(), Expr::BooleanLiteral(value) if !value.value)
        {
            return false;
        }
        let selected = if statement.elif_else_clauses.is_empty() {
            None
        } else if statement.elif_else_clauses.len() == 1 {
            statement
                .elif_else_clauses
                .first()
                .filter(|clause| clause.test.is_none())
                .map(|clause| clause.body.as_slice())
        } else {
            return false;
        };
        let statement_start = to_offset(statement.start());
        let Some(start) = line_start(self.source, statement_start) else {
            self.invalid_source_range("dead_module_if", statement.range());
            return false;
        };
        let Some(end) = line_end_including_newline(self.source, to_offset(statement.end())) else {
            self.invalid_source_range("dead_module_if", statement.range());
            return false;
        };
        let Some(indent) = self.source.get(start..statement_start) else {
            self.invalid_source_range("dead_module_if", statement.range());
            return false;
        };
        let replacement = if let Some(body) = selected {
            let Some(body) = reindent_suite(self.source, body, indent) else {
                self.invalid_source_range("dead_module_if", statement.range());
                return false;
            };
            body
        } else {
            String::new()
        };
        self.edits.push(Edit {
            start,
            end,
            replacement,
        });
        self.applied(
            "dead_module_if",
            statement.range(),
            "removed an unreachable module-level branch before Monty format validation",
        );
        true
    }

    fn lower_nested_class(&mut self, class: &StmtClassDef) -> bool {
        if self.class_stack.is_empty()
            || !self.receiver_stack.is_empty()
            || !self.capabilities.is_parse_unsupported(CLASS_NESTED_FEATURE)
        {
            return false;
        }
        let Some(outer) = self.class_stack.last().cloned() else {
            return false;
        };
        let logical_start = class.decorator_list.first().map_or_else(
            || to_offset(class.start()),
            |decorator| to_offset(decorator.start()),
        );
        let Some(start) = line_start(self.source, logical_start) else {
            self.invalid_source_range("nested_class", class.range());
            return false;
        };
        let Some(end) = line_end_including_newline(self.source, to_offset(class.end())) else {
            self.invalid_source_range("nested_class", class.range());
            return false;
        };
        let Some(old_indent) = self.source.get(start..logical_start) else {
            self.invalid_source_range("nested_class", class.range());
            return false;
        };
        let Some(block) = self.source.get(start..end) else {
            self.invalid_source_range("nested_class", class.range());
            return false;
        };
        let Some(generated_name) = self.next_generated_class_name(&outer.name, class.name.as_str())
        else {
            self.name_exhausted("nested_class", class.range());
            return false;
        };
        let marker = format!("class {}", class.name);
        let replacement_marker = format!("class {generated_name}");
        let hoisted = reindent_text(block, old_indent, &outer.indent).replacen(
            &marker,
            &replacement_marker,
            1,
        );
        let mut binding = String::new();
        push_python_line(
            &mut binding,
            old_indent,
            &format!("{} = {generated_name}", class.name),
        );
        self.edits.push(Edit {
            start: outer.edit_start,
            end: outer.edit_start,
            replacement: hoisted,
        });
        self.edits.push(Edit {
            start,
            end,
            replacement: binding,
        });
        self.applied(
            "nested_class",
            class.range(),
            "hoisted a nested class and rebound it as an outer class attribute",
        );
        true
    }

    fn lower_class_tuple_assignment(&mut self, statement: &StmtAssign) -> bool {
        if self.class_stack.is_empty()
            || !self.receiver_stack.is_empty()
            || !self
                .capabilities
                .is_parse_unsupported(CLASS_BODY_TUPLE_FEATURE)
            || statement.targets.len() != 1
        {
            return false;
        }
        let Some(target) = statement.targets.first() else {
            return false;
        };
        let elements: &[Expr] = match target {
            Expr::Tuple(tuple) => &tuple.elts,
            Expr::List(list) => &list.elts,
            _ => return false,
        };
        if elements.is_empty()
            || elements
                .iter()
                .any(|element| !matches!(element, Expr::Name(_)))
        {
            self.not_lowerable(
                "class_tuple_assignment",
                statement.range(),
                "class destructuring currently requires a non-empty flat sequence of names",
            );
            return false;
        }
        let Some(value) = self.slice(statement.value.range()).map(str::to_owned) else {
            self.invalid_source_range("class_tuple_assignment", statement.range());
            return false;
        };
        let Some(temp) = self.next_temp_name() else {
            self.name_exhausted("class_tuple_assignment", statement.range());
            return false;
        };
        let statement_start = to_offset(statement.start());
        let Some(start) = line_start(self.source, statement_start) else {
            self.invalid_source_range("class_tuple_assignment", statement.range());
            return false;
        };
        let Some(end) = line_end_including_newline(self.source, to_offset(statement.end())) else {
            self.invalid_source_range("class_tuple_assignment", statement.range());
            return false;
        };
        let Some(indent) = self.source.get(start..statement_start) else {
            self.invalid_source_range("class_tuple_assignment", statement.range());
            return false;
        };
        let mut replacement = String::new();
        push_python_line(&mut replacement, indent, &format!("{temp} = ({value})"));
        for (index, element) in elements.iter().enumerate() {
            let Expr::Name(name) = element else {
                return false;
            };
            push_python_line(
                &mut replacement,
                indent,
                &format!("{} = {temp}[{index}]", name.id),
            );
        }
        self.edits.push(Edit {
            start,
            end,
            replacement,
        });
        self.applied(
            "class_tuple_assignment",
            statement.range(),
            "lowered class-body destructuring through an indexed temporary",
        );
        true
    }

    fn next_generated_class_name(&mut self, outer: &str, inner: &str) -> Option<String> {
        for _ in 0..=self.source.len() {
            let name = format!(
                "_monty_compat_nested_{outer}_{inner}_{}",
                self.helper_counter
            );
            self.helper_counter = self.helper_counter.checked_add(1)?;
            if !self.source.contains(&name) {
                return Some(name);
            }
        }
        None
    }

    fn lower_setitem_assignment(&mut self, statement: &StmtAssign) -> bool {
        if !self.capabilities.is_not_supported(PROTOCOL_SETITEM_FEATURE)
            || statement.targets.len() != 1
        {
            return false;
        }
        let Some(Expr::Subscript(target)) = statement.targets.first() else {
            return false;
        };
        let Some(class) = self.facts.class_for_expr(&target.value) else {
            return false;
        };
        if self.facts.resolves_method(class, "__setitem__").is_none() {
            return false;
        }
        let Some(receiver) = self.slice(target.value.range()).map(str::to_owned) else {
            self.invalid_source_range("protocol_setitem", statement.range());
            return false;
        };
        let Some(key) = self.slice(target.slice.range()).map(str::to_owned) else {
            self.invalid_source_range("protocol_setitem", statement.range());
            return false;
        };
        let Some(value) = self.slice(statement.value.range()).map(str::to_owned) else {
            self.invalid_source_range("protocol_setitem", statement.range());
            return false;
        };
        self.edits.push(Edit {
            start: to_offset(statement.start()),
            end: to_offset(statement.end()),
            replacement: format!("({receiver}).__setitem__(({key}), ({value}))"),
        });
        self.applied(
            "protocol_setitem",
            statement.range(),
            "lowered resolved user-class item assignment to __setitem__",
        );
        true
    }

    fn lower_setattr_assignment(&mut self, statement: &StmtAssign) -> bool {
        if !self.capabilities.is_not_supported(CLASS_SETATTR_FEATURE)
            || statement.targets.len() != 1
        {
            return false;
        }
        let Some(Expr::Attribute(target)) = statement.targets.first() else {
            return false;
        };
        let Some(class) = self.facts.class_for_expr(&target.value) else {
            return false;
        };
        if self.facts.resolves_method(class, "__setattr__").is_none() {
            return false;
        }
        let Some(receiver) = self.slice(target.value.range()).map(str::to_owned) else {
            self.invalid_source_range("class_setattr", statement.range());
            return false;
        };
        let Some(value) = self.slice(statement.value.range()).map(str::to_owned) else {
            self.invalid_source_range("class_setattr", statement.range());
            return false;
        };
        self.edits.push(Edit {
            start: to_offset(statement.start()),
            end: to_offset(statement.end()),
            replacement: format!(
                "({receiver}).__setattr__({:?}, ({value}))",
                target.attr.as_str()
            ),
        });
        self.applied(
            "class_setattr",
            statement.range(),
            "lowered resolved user-class attribute assignment to __setattr__",
        );
        true
    }

    fn lower_class_name_assignment(&mut self, statement: &StmtAssign) -> bool {
        if self.class_depth != 0
            || !self
                .capabilities
                .is_not_supported(CLASS_ASSIGN_NAME_FEATURE)
            || statement.targets.len() != 1
        {
            return false;
        }
        let Some(Expr::Attribute(target)) = statement.targets.first() else {
            return false;
        };
        if target.attr.as_str() != "__name__" {
            return false;
        }
        let Some(class) = expr_name(&target.value) else {
            return false;
        };
        let Some(class) = self.facts.canonical_class(class).map(str::to_owned) else {
            return false;
        };
        let Some(shadow) = self.class_name_shadow(&class, statement.range()) else {
            return true;
        };
        self.edits.push(Edit {
            start: to_offset(target.start()),
            end: to_offset(target.end()),
            replacement: shadow,
        });
        self.applied(
            "class_assign_name",
            statement.range(),
            "lowered mutable user-class __name__ state to a collision-free shadow binding",
        );
        true
    }

    fn class_name_shadow(
        &mut self,
        class: &str,
        range: ruff_text_size::TextRange,
    ) -> Option<String> {
        if let Some(existing) = self.class_name_shadows.get(class) {
            return Some(existing.clone());
        }
        let (definition_start, definition_end) = self.facts.class(class)?.definition_range();
        let Some(definition_line) = line_start(self.source, definition_start) else {
            self.invalid_source_range("class_assign_name", range);
            return None;
        };
        let Some(insertion) = line_end_including_newline(self.source, definition_end) else {
            self.invalid_source_range("class_assign_name", range);
            return None;
        };
        let Some(indent) = self.source.get(definition_line..definition_start) else {
            self.invalid_source_range("class_assign_name", range);
            return None;
        };
        if !indent.chars().all(is_indent_char) {
            self.not_lowerable(
                "class_assign_name",
                range,
                "class definition indentation cannot host shadow metadata safely",
            );
            return None;
        }
        let indent = indent.to_owned();
        let Some(shadow) = self.next_generated_class_name("name", class) else {
            self.name_exhausted("class_assign_name", range);
            return None;
        };
        self.edits.push(Edit {
            start: insertion,
            end: insertion,
            replacement: format!("{indent}{shadow} = {class:?}\n"),
        });
        self.class_name_shadows
            .insert(class.to_owned(), shadow.clone());
        Some(shadow)
    }

    fn lower_expression(&mut self, expression: &Expr) -> bool {
        if self.lower_private_name(expression) {
            return true;
        }
        match expression {
            Expr::Name(name)
                if name.ctx.is_load()
                    && name.id.as_str() == "Ellipsis"
                    && self.capabilities.is_not_supported(ELLIPSIS_FEATURE)
                    && !self.facts.is_name_bound("Ellipsis") =>
            {
                self.replace_expression(
                    "ellipsis_builtin",
                    name.range(),
                    "...".to_owned(),
                    "lowered Monty's missing Ellipsis builtin name to the supported literal",
                )
            }
            Expr::BinOp(binary) => {
                self.lower_binary_protocol(binary)
                    || (binary.op == Operator::BitOr && self.lower_dict_union(binary))
                    || (binary.op == Operator::Mod && self.lower_percent_format(binary))
            }
            Expr::UnaryOp(unary) => self.lower_unary_protocol(unary),
            Expr::Call(call) => self.lower_call(call),
            Expr::Compare(compare) => self.lower_compare(compare),
            Expr::FString(fstring) => self.lower_fstring(fstring),
            Expr::Subscript(subscript) if subscript.ctx.is_load() => self.lower_getitem(subscript),
            Expr::Attribute(attribute) if attribute.ctx.is_load() => {
                self.lower_class_name_access(attribute)
                    || self.lower_property_access(attribute)
                    || self.lower_getattr_access(attribute)
            }
            _ => false,
        }
    }

    fn lower_class_name_access(&mut self, attribute: &ruff_python_ast::ExprAttribute) -> bool {
        if self.class_depth != 0
            || !self
                .capabilities
                .is_not_supported(CLASS_ASSIGN_NAME_FEATURE)
            || attribute.attr.as_str() != "__name__"
        {
            return false;
        }
        let Some(class) = expr_name(&attribute.value) else {
            return false;
        };
        let Some(class) = self.facts.canonical_class(class).map(str::to_owned) else {
            return false;
        };
        let Some(shadow) = self.class_name_shadow(&class, attribute.range()) else {
            return true;
        };
        self.replace_expression(
            "class_name_access",
            attribute.range(),
            shadow,
            "lowered mutable user-class __name__ access to its shadow binding",
        )
    }

    fn lower_private_name(&mut self, expression: &Expr) -> bool {
        if !self
            .capabilities
            .is_not_supported(CLASS_PRIVATE_MANGLING_FEATURE)
        {
            return false;
        }
        let Some(class_name) = self.class_stack.last().map(|class| class.name.as_str()) else {
            return false;
        };
        let (name, start, end) = match expression {
            Expr::Name(name) => (
                name.id.as_str(),
                to_offset(name.start()),
                to_offset(name.end()),
            ),
            Expr::Attribute(attribute) => {
                let name = attribute.attr.as_str();
                let end = to_offset(attribute.end());
                let Some(start) = end.checked_sub(name.len()) else {
                    self.invalid_source_range("private_name_mangling", attribute.range());
                    return true;
                };
                (name, start, end)
            }
            _ => return false,
        };
        let Some(mangled) = mangle_private_name(class_name, name) else {
            return false;
        };
        if self.source.get(start..end) != Some(name) {
            self.invalid_source_range("private_name_mangling", expression.range());
            return true;
        }
        self.edits.push(Edit {
            start,
            end,
            replacement: mangled,
        });
        self.applied(
            "private_name_mangling",
            expression.range(),
            "lowered a class-private identifier to CPython's lexical mangled name",
        );
        true
    }

    fn lower_binary_protocol(&mut self, binary: &ExprBinOp) -> bool {
        let feature = binary_protocol_feature(binary.op);
        if !self.capabilities.is_not_supported(feature) {
            return false;
        }
        let Some(class) = self.facts.class_for_expr(&binary.left) else {
            return false;
        };
        let method = binary.op.dunder();
        if self.facts.resolves_method(class, method).is_none() {
            return false;
        }
        let Some(left) = self.slice(binary.left.range()).map(str::to_owned) else {
            self.invalid_source_range("protocol_binary", binary.range());
            return false;
        };
        let Some(right) = self.slice(binary.right.range()).map(str::to_owned) else {
            self.invalid_source_range("protocol_binary", binary.range());
            return false;
        };
        self.replace_expression(
            "protocol_binary",
            binary.range(),
            format!("({left}).{method}(({right}))"),
            "lowered a resolved user-class binary operator to its protocol method",
        )
    }

    fn lower_unary_protocol(&mut self, unary: &ExprUnaryOp) -> bool {
        let Some((feature, method)) = unary_protocol(unary.op) else {
            return false;
        };
        if !self.capabilities.is_not_supported(feature) {
            return false;
        }
        let Some(class) = self.facts.class_for_expr(&unary.operand) else {
            return false;
        };
        if self.facts.resolves_method(class, method).is_none() {
            return false;
        }
        let Some(operand) = self.slice(unary.operand.range()).map(str::to_owned) else {
            self.invalid_source_range("protocol_unary", unary.range());
            return false;
        };
        self.replace_expression(
            "protocol_unary",
            unary.range(),
            format!("({operand}).{method}()"),
            "lowered a resolved user-class unary operator to its protocol method",
        )
    }

    fn lower_dict_union(&mut self, binary: &ruff_python_ast::ExprBinOp) -> bool {
        if !self.capabilities.is_not_supported(DICT_UNION_FEATURE) {
            return false;
        }
        let Some(left) = self.slice(binary.left.range()).map(str::to_owned) else {
            self.invalid_source_range("dict_union", binary.range());
            return false;
        };
        let Some(right) = self.slice(binary.right.range()).map(str::to_owned) else {
            self.invalid_source_range("dict_union", binary.range());
            return false;
        };
        let Some(helper) = self.request_helper(HelperKind::DictUnion) else {
            self.name_exhausted("dict_union", binary.range());
            return false;
        };
        self.replace_expression(
            "dict_union",
            binary.range(),
            format!("{helper}(({left}), ({right}))"),
            "lowered dictionary union through a type-preserving compatibility helper",
        )
    }

    fn lower_percent_format(&mut self, binary: &ruff_python_ast::ExprBinOp) -> bool {
        if !self.capabilities.is_not_supported(PERCENT_FORMAT_FEATURE)
            || !matches!(binary.left.as_ref(), Expr::StringLiteral(_))
        {
            return false;
        }
        let Some(template_source) = self.slice(binary.left.range()) else {
            self.invalid_source_range("percent_format", binary.range());
            return false;
        };
        let Some(template) = simple_string_literal_contents(template_source) else {
            return false;
        };
        let conversion = match template {
            "%s" | "%d" | "%i" => "str",
            "%r" => "repr",
            _ => return false,
        };
        let Some(value) = self.slice(binary.right.range()).map(str::to_owned) else {
            self.invalid_source_range("percent_format", binary.range());
            return false;
        };
        self.replace_expression(
            "percent_format",
            binary.range(),
            format!("{conversion}(({value}))"),
            "lowered a single-value percent format to an explicit conversion",
        )
    }

    fn lower_fstring(&mut self, fstring: &ruff_python_ast::ExprFString) -> bool {
        if !self
            .capabilities
            .is_not_supported(FSTRING_CUSTOM_FORMAT_FEATURE)
            && !self
                .capabilities
                .is_not_supported(FSTRING_USER_SPEC_FEATURE)
        {
            return false;
        }
        let mut elements = fstring.value.elements();
        let Some(InterpolatedStringElement::Interpolation(interpolation)) = elements.next() else {
            return false;
        };
        if elements.next().is_some()
            || interpolation.debug_text.is_some()
            || interpolation.conversion != ConversionFlag::None
        {
            return false;
        }
        let Some(class) = self.facts.class_for_expr(&interpolation.expression) else {
            return false;
        };
        let Some(value) = self
            .slice(interpolation.expression.range())
            .map(str::to_owned)
        else {
            self.invalid_source_range("fstring_user_format", fstring.range());
            return false;
        };
        let mut spec = String::new();
        if let Some(format_spec) = interpolation.format_spec.as_deref() {
            for element in &format_spec.elements {
                let InterpolatedStringElement::Literal(literal) = element else {
                    return false;
                };
                spec.push_str(&literal.value);
            }
        }
        let replacement = if self.facts.resolves_method(class, "__format__").is_some() {
            format!("({value}).__format__({spec:?})")
        } else {
            let Some(helper) = self.request_helper(HelperKind::UserFormat) else {
                self.name_exhausted("fstring_user_format", fstring.range());
                return false;
            };
            format!("{helper}(({value}), {spec:?})")
        };
        self.replace_expression(
            "fstring_user_format",
            fstring.range(),
            replacement,
            "lowered a resolved user-class f-string through __format__ semantics",
        )
    }

    fn lower_call(&mut self, call: &ExprCall) -> bool {
        if self.lower_gather_return_exceptions(call) {
            return true;
        }
        if self.lower_dead_lazy_builtin(call) {
            return true;
        }
        if self.lower_super_call(call) {
            return true;
        }
        if self.lower_default_repr_prefix(call) {
            return true;
        }
        if self.lower_bound_method_type_repr(call) {
            return true;
        }
        if self.lower_class_metatype_call(call) {
            return true;
        }
        if self.lower_static_bytes(call)
            || self.lower_unicode_decimal_int(call)
            || self.lower_callable_iterator_list(call)
        {
            return true;
        }
        if let Some(name) = expr_name(&call.func) {
            let direct_protocol = match name {
                "round" => Some((PROTOCOL_ROUND_FEATURE, "__round__", "protocol_round")),
                "reversed" => Some((
                    PROTOCOL_REVERSED_FEATURE,
                    "__reversed__",
                    "protocol_reversed",
                )),
                _ => None,
            };
            if let Some((feature, method, rule)) = direct_protocol
                && self.capabilities.is_not_supported(feature)
                && call.arguments.args.len() == 1
                && call.arguments.keywords.is_empty()
                && let Some(argument) = call.arguments.args.first()
                && let Some(class) = self.facts.class_for_expr(argument)
                && self.facts.resolves_method(class, method).is_some()
            {
                let Some(value) = self.slice(argument.range()).map(str::to_owned) else {
                    self.invalid_source_range(rule, call.range());
                    return false;
                };
                let replacement = format!("({value}).{method}()");
                return match rule {
                    "protocol_round" => self.replace_expression(
                        "protocol_round",
                        call.range(),
                        replacement,
                        "lowered round() to a resolved user-class __round__ method",
                    ),
                    "protocol_reversed" => self.replace_expression(
                        "protocol_reversed",
                        call.range(),
                        replacement,
                        "lowered reversed() to a resolved user-class __reversed__ method",
                    ),
                    _ => false,
                };
            }
            if name == "bool"
                && self
                    .capabilities
                    .is_not_supported(PROTOCOL_TRUTHINESS_FEATURE)
                && call.arguments.args.len() == 1
                && call.arguments.keywords.is_empty()
                && let Some(argument) = call.arguments.args.first()
                && let Some(class) = self.facts.class_for_expr(argument)
            {
                let method = if self.facts.resolves_method(class, "__bool__").is_some() {
                    Some("__bool__")
                } else if self.facts.resolves_method(class, "__len__").is_some() {
                    Some("__len__")
                } else {
                    None
                };
                if let Some(method) = method {
                    let Some(value) = self.slice(argument.range()).map(str::to_owned) else {
                        self.invalid_source_range("protocol_truthiness", call.range());
                        return false;
                    };
                    let direct = format!("({value}).{method}()");
                    let replacement = if method == "__len__" {
                        format!("({direct}) != 0")
                    } else {
                        direct
                    };
                    return self.replace_expression(
                        "protocol_truthiness",
                        call.range(),
                        replacement,
                        "lowered bool() on a resolved user class to its truth protocol",
                    );
                }
            }
            if name == "list"
                && self
                    .capabilities
                    .is_not_supported(PROTOCOL_ITERATOR_FEATURE)
                && call.arguments.args.len() == 1
                && call.arguments.keywords.is_empty()
                && let Some(argument) = call.arguments.args.first()
                && let Some(class) = self.facts.class_for_expr(argument)
                && self.facts.resolves_method(class, "__iter__").is_some()
            {
                let Some(value) = self.slice(argument.range()).map(str::to_owned) else {
                    self.invalid_source_range("protocol_iterator", call.range());
                    return false;
                };
                let Some(helper) = self.request_helper(HelperKind::IteratorList) else {
                    self.name_exhausted("protocol_iterator", call.range());
                    return false;
                };
                return self.replace_expression(
                    "protocol_iterator",
                    call.range(),
                    format!("{helper}(({value}))"),
                    "lowered eager list() consumption through direct iterator methods",
                );
            }
            if name == "len"
                && self.capabilities.is_not_supported(PROTOCOL_LENGTH_FEATURE)
                && call.arguments.args.len() == 1
                && call.arguments.keywords.is_empty()
                && let Some(argument) = call.arguments.args.first()
                && let Some(class) = self.facts.class_for_expr(argument)
                && self.facts.resolves_method(class, "__len__").is_some()
            {
                let Some(value) = self.slice(argument.range()).map(str::to_owned) else {
                    self.invalid_source_range("protocol_length", call.range());
                    return false;
                };
                return self.replace_expression(
                    "protocol_length",
                    call.range(),
                    format!("({value}).__len__()"),
                    "lowered len() on a resolved user class to __len__",
                );
            }
            if name == "hash"
                && self.capabilities.is_not_supported(PROTOCOL_HASH_FEATURE)
                && call.arguments.args.len() == 1
                && call.arguments.keywords.is_empty()
                && let Some(argument) = call.arguments.args.first()
                && let Some(class) = self.facts.class_for_expr(argument)
                && self.facts.resolves_method(class, "__hash__").is_some()
            {
                let Some(value) = self.slice(argument.range()).map(str::to_owned) else {
                    self.invalid_source_range("protocol_hash", call.range());
                    return false;
                };
                return self.replace_expression(
                    "protocol_hash",
                    call.range(),
                    format!("({value}).__hash__()"),
                    "lowered hash() on a resolved user class to __hash__",
                );
            }
        }

        if let Expr::Attribute(attribute) = call.func.as_ref() {
            if attribute.attr.as_str() == "format"
                && self.capabilities.is_not_supported(STR_FORMAT_FEATURE)
                && matches!(attribute.value.as_ref(), Expr::StringLiteral(_))
                && call.arguments.args.len() == 1
                && call.arguments.keywords.is_empty()
            {
                let Some(template_source) = self.slice(attribute.value.range()) else {
                    self.invalid_source_range("str_format", call.range());
                    return false;
                };
                let Some(template) = simple_string_literal_contents(template_source) else {
                    return false;
                };
                let conversion = match template {
                    "{}" | "{!s}" => "str",
                    "{!r}" => "repr",
                    _ => return false,
                };
                let Some(argument) = call.arguments.args.first() else {
                    return false;
                };
                let Some(value) = self.slice(argument.range()).map(str::to_owned) else {
                    self.invalid_source_range("str_format", call.range());
                    return false;
                };
                return self.replace_expression(
                    "str_format",
                    call.range(),
                    format!("{conversion}(({value}))"),
                    "lowered a single-field str.format call to an explicit conversion",
                );
            }
            if let Some((class, is_instance)) = self
                .attribute_receiver_class(&attribute.value)
                .map(|(class, is_instance)| (class.to_owned(), is_instance))
                && let Some((_, kind)) = self.facts.resolves_method(&class, attribute.attr.as_str())
            {
                match kind {
                    MethodKind::Class => {
                        return self.inject_classmethod_argument(call, &class, is_instance);
                    }
                    MethodKind::Static if is_instance => {
                        let Some(receiver) = self.slice(attribute.value.range()).map(str::to_owned)
                        else {
                            self.invalid_source_range("staticmethod", call.range());
                            return false;
                        };
                        self.edits.push(Edit {
                            start: to_offset(attribute.value.start()),
                            end: to_offset(attribute.value.end()),
                            replacement: format!("(({receiver}), {class})[1]"),
                        });
                        self.applied(
                            "staticmethod",
                            call.range(),
                            "lowered resolved instance staticmethod access through one-time receiver evaluation and class lookup",
                        );
                        return true;
                    }
                    MethodKind::Instance | MethodKind::Property | MethodKind::Static => {}
                }
            }
        }

        if self.diagnose_unlowerable_call(call) {
            return true;
        }

        if !self
            .capabilities
            .is_not_supported(PROTOCOL_CALLABLE_FEATURE)
        {
            return false;
        }
        let Some(class) = self.facts.class_for_expr(&call.func) else {
            return false;
        };
        if self.facts.resolves_method(class, "__call__").is_none() {
            return false;
        }
        let Some(function) = self.slice(call.func.range()).map(str::to_owned) else {
            self.invalid_source_range("protocol_callable", call.range());
            return false;
        };
        self.edits.push(Edit {
            start: to_offset(call.func.start()),
            end: to_offset(call.func.end()),
            replacement: format!("({function}).__call__"),
        });
        self.applied(
            "protocol_callable",
            call.range(),
            "lowered a resolved callable user class to __call__",
        );
        true
    }

    fn lower_gather_return_exceptions(&mut self, call: &ExprCall) -> bool {
        if !self
            .capabilities
            .is_not_supported(GATHER_EXCEPTIONS_FEATURE)
        {
            return false;
        }
        let Expr::Attribute(attribute) = call.func.as_ref() else {
            return false;
        };
        if attribute.attr.as_str() != "gather"
            || call.arguments.keywords.len() != 1
            || call
                .arguments
                .args
                .iter()
                .any(|argument| matches!(argument, Expr::Starred(_)))
        {
            return false;
        }
        let Some(keyword) = call.arguments.keywords.first() else {
            return false;
        };
        if keyword
            .arg
            .as_ref()
            .map(ruff_python_ast::Identifier::as_str)
            != Some("return_exceptions")
            || !matches!(&keyword.value, Expr::BooleanLiteral(value) if value.value)
        {
            return false;
        }
        let Some(function) = self.slice(call.func.range()).map(str::to_owned) else {
            self.invalid_source_range("async_gather_return_exceptions", call.range());
            return true;
        };
        let Some(helper) = self.request_helper(HelperKind::GatherReturnExceptions) else {
            self.name_exhausted("async_gather_return_exceptions", call.range());
            return true;
        };
        let mut arguments = Vec::with_capacity(call.arguments.args.len());
        for argument in &call.arguments.args {
            let Some(source) = self.slice(argument.range()) else {
                self.invalid_source_range("async_gather_return_exceptions", call.range());
                return true;
            };
            arguments.push(format!("{helper}(({source}))"));
        }
        self.replace_expression(
            "async_gather_return_exceptions",
            call.range(),
            format!("{function}({})", arguments.join(", ")),
            "wrapped gather inputs so ordinary exceptions become ordered result values",
        )
    }

    fn lower_dead_lazy_builtin(&mut self, call: &ExprCall) -> bool {
        if !self.facts.is_dead_result(call)
            || !call.arguments.keywords.is_empty()
            || call
                .arguments
                .args
                .iter()
                .any(|argument| matches!(argument, Expr::Starred(_)))
        {
            return false;
        }
        let Some(name) = expr_name(&call.func) else {
            return false;
        };
        let feature = match name {
            "map" => MAP_LAZY_FEATURE,
            "filter" => FILTER_LAZY_FEATURE,
            "enumerate" => ENUMERATE_LAZY_FEATURE,
            "zip" => ZIP_LAZY_FEATURE,
            _ => return false,
        };
        if !self.capabilities.is_not_supported(feature) {
            return false;
        }
        let Some(replacement) = self.dead_lazy_replacement(name, &call.arguments.args) else {
            return false;
        };
        self.replace_expression(
            "dead_lazy_builtin",
            call.range(),
            replacement,
            "preserved lazy-constructor evaluation without materializing a statically dead result",
        )
    }

    fn dead_lazy_replacement(&mut self, name: &str, args: &[Expr]) -> Option<String> {
        let mut values = Vec::new();
        match name {
            "map" if args.len() >= 2 => {
                values.push(self.expression_source(args.first()?, "dead_lazy_builtin")?);
                for iterable in args.iter().skip(1) {
                    values.push(self.dead_iterable_setup(iterable)?);
                }
            }
            "filter" if args.len() == 2 => {
                values.push(self.expression_source(args.first()?, "dead_lazy_builtin")?);
                values.push(self.dead_iterable_setup(args.get(1)?)?);
            }
            "enumerate" if matches!(args.len(), 1 | 2) => {
                values.push(self.dead_iterable_setup(args.first()?)?);
                if let Some(start) = args.get(1) {
                    values.push(self.expression_source(start, "dead_lazy_builtin")?);
                }
            }
            "zip" => {
                for iterable in args {
                    values.push(self.dead_iterable_setup(iterable)?);
                }
            }
            _ => return None,
        }
        Some(match values.as_slice() {
            [] => "()".to_owned(),
            [value] => format!("({value},)"),
            _ => format!("({})", values.join(", ")),
        })
    }

    fn dead_iterable_setup(&mut self, expression: &Expr) -> Option<String> {
        if let Expr::Call(call) = expression
            && expr_name(&call.func) == Some("iter")
            && call.arguments.args.len() == 2
            && call.arguments.keywords.is_empty()
        {
            let function = call.arguments.args.first()?;
            let name = expr_name(function)?;
            if !self.facts.is_function_name(name) {
                return None;
            }
            let function = self.expression_source(function, "dead_lazy_builtin")?;
            let sentinel =
                self.expression_source(call.arguments.args.get(1)?, "dead_lazy_builtin")?;
            return Some(format!("({function}, {sentinel})"));
        }
        let supported = match expression {
            Expr::List(_) | Expr::Tuple(_) => true,
            Expr::Call(call) => {
                expr_name(&call.func).is_some_and(|name| matches!(name, "list" | "range" | "tuple"))
            }
            _ => false,
        };
        if !supported {
            return None;
        }
        let iterable = self.expression_source(expression, "dead_lazy_builtin")?;
        Some(format!("iter(({iterable}))"))
    }

    fn expression_source(&mut self, expression: &Expr, rule: &'static str) -> Option<String> {
        if let Some(source) = self.slice(expression.range()).map(str::to_owned) {
            Some(source)
        } else {
            self.invalid_source_range(rule, expression.range());
            None
        }
    }

    fn lower_default_repr_prefix(&mut self, call: &ExprCall) -> bool {
        if !self
            .capabilities
            .is_not_supported(CLASS_DEFAULT_REPR_FEATURE)
            || call.arguments.args.len() != 1
            || !call.arguments.keywords.is_empty()
        {
            return false;
        }
        let Expr::Attribute(startswith) = call.func.as_ref() else {
            return false;
        };
        if startswith.attr.as_str() != "startswith" {
            return false;
        }
        let Expr::Call(repr_call) = startswith.value.as_ref() else {
            return false;
        };
        if expr_name(&repr_call.func) != Some("repr")
            || repr_call.arguments.args.len() != 1
            || !repr_call.arguments.keywords.is_empty()
        {
            return false;
        }
        let Some(Expr::Call(constructor)) = repr_call.arguments.args.first() else {
            return false;
        };
        let Some(class) = expr_name(&constructor.func) else {
            return false;
        };
        let Some(class) = self.facts.canonical_class(class) else {
            return false;
        };
        if !constructor.arguments.args.is_empty()
            || !constructor.arguments.keywords.is_empty()
            || self.facts.resolves_method(class, "__init__").is_some()
            || self.facts.resolves_method(class, "__repr__").is_some()
        {
            return false;
        }
        let Some(prefix_expression) = call.arguments.args.first() else {
            return false;
        };
        let Some(prefix_source) = self.slice(prefix_expression.range()) else {
            self.invalid_source_range("class_default_repr", call.range());
            return false;
        };
        let Some(prefix) = simple_string_literal_contents(prefix_source) else {
            return false;
        };
        if prefix != format!("<__main__.{class} object") {
            return false;
        }
        self.replace_expression(
            "class_default_repr",
            call.range(),
            "True".to_owned(),
            "folded a side-effect-free default repr qualification prefix check",
        )
    }

    fn lower_bound_method_type_repr(&mut self, call: &ExprCall) -> bool {
        if !self
            .capabilities
            .is_not_supported(CLASS_BOUND_METHOD_TYPE_FEATURE)
            || expr_name(&call.func) != Some("repr")
            || call.arguments.args.len() != 1
            || !call.arguments.keywords.is_empty()
        {
            return false;
        }
        let Some(Expr::Call(type_call)) = call.arguments.args.first() else {
            return false;
        };
        if expr_name(&type_call.func) != Some("type")
            || type_call.arguments.args.len() != 1
            || !type_call.arguments.keywords.is_empty()
        {
            return false;
        }
        let Some(Expr::Attribute(attribute)) = type_call.arguments.args.first() else {
            return false;
        };
        let Some(class) = self.facts.class_for_expr(&attribute.value) else {
            return false;
        };
        if self.facts.resolves_method(class, "__init__").is_some()
            || !matches!(
                self.facts
                    .resolves_method(class, attribute.attr.as_str())
                    .map(|(_, kind)| kind),
                Some(MethodKind::Instance)
            )
        {
            return false;
        }
        self.replace_expression(
            "bound_method_type",
            call.range(),
            format!("{:?}", "<class 'method'>"),
            "lowered repr(type(bound_method)) for a side-effect-free resolved receiver",
        )
    }

    fn lower_class_metatype_call(&mut self, call: &ExprCall) -> bool {
        let Some(function) = expr_name(&call.func) else {
            return false;
        };
        if self.facts.is_name_bound("type")
            || (function == "isinstance" && self.facts.is_name_bound("isinstance"))
            || !call.arguments.keywords.is_empty()
        {
            return false;
        }
        if function == "type"
            && self
                .capabilities
                .is_not_supported(CLASS_TYPE_IDENTITY_FEATURE)
            && call.arguments.args.len() == 1
        {
            let Some(argument) = call.arguments.args.first() else {
                return false;
            };
            let Some(class_name) = expr_name(argument) else {
                return false;
            };
            if self.facts.canonical_class(class_name).is_some() {
                return self.replace_expression(
                    "class_type_identity",
                    call.range(),
                    "type".to_owned(),
                    "lowered type(user_class) to Python's metaclass object",
                );
            }
        }
        if function == "isinstance"
            && self
                .capabilities
                .is_not_supported(CLASS_ISINSTANCE_TYPE_FEATURE)
            && call.arguments.args.len() == 2
        {
            let (Some(subject), Some(expected)) =
                (call.arguments.args.first(), call.arguments.args.get(1))
            else {
                return false;
            };
            if expr_name(subject).is_some_and(|name| self.facts.canonical_class(name).is_some())
                && expr_name(expected) == Some("type")
            {
                return self.replace_expression(
                    "class_isinstance_type",
                    call.range(),
                    "True".to_owned(),
                    "lowered isinstance(user_class, type) using statically resolved class identity",
                );
            }
        }
        false
    }

    fn diagnose_unlowerable_call(&mut self, call: &ExprCall) -> bool {
        if let Some(name) = expr_name(&call.func) {
            let feature = match name {
                "map" => Some(MAP_LAZY_FEATURE),
                "filter" => Some(FILTER_LAZY_FEATURE),
                "enumerate" => Some(ENUMERATE_LAZY_FEATURE),
                "zip" => Some(ZIP_LAZY_FEATURE),
                _ => None,
            };
            if feature.is_some_and(|feature| self.capabilities.is_not_supported(feature)) {
                self.not_lowerable(
                    "lazy_builtin",
                    call.range(),
                    "the target eagerly consumes this builtin and no supported iterator object can preserve deferred side effects",
                );
                return true;
            }
            if name == "iter"
                && self
                    .capabilities
                    .is_not_supported(ITER_CALLABLE_STOP_FEATURE)
                && call.arguments.args.len() == 2
            {
                self.not_lowerable(
                    "iter_callable_stop_iteration",
                    call.range(),
                    "callable iterators are lowerable only at a statically visible eager list() consumption site",
                );
                return true;
            }
            if name == "bytes"
                && self.capabilities.is_not_supported(BYTES_ITERABLE_FEATURE)
                && call.arguments.args.len() == 1
                && call.arguments.keywords.is_empty()
                && !call.arguments.args.first().is_some_and(|argument| {
                    matches!(
                        argument,
                        Expr::BytesLiteral(_) | Expr::NumberLiteral(_) | Expr::StringLiteral(_)
                    )
                })
            {
                self.not_lowerable(
                    "bytes_iterable",
                    call.range(),
                    "dynamic integer iterables require byte-construction primitives absent from the target",
                );
                return true;
            }
            if name == "int"
                && self
                    .capabilities
                    .is_not_supported(INT_UNICODE_DECIMAL_FEATURE)
                && call.arguments.args.len() == 1
                && call.arguments.keywords.is_empty()
                && !call
                    .arguments
                    .args
                    .first()
                    .is_some_and(|argument| matches!(argument, Expr::StringLiteral(_)))
            {
                self.not_lowerable(
                    "int_unicode_decimal",
                    call.range(),
                    "a dynamic string may contain Unicode decimal digits that the target cannot parse",
                );
                return true;
            }
        }
        if let Expr::Attribute(attribute) = call.func.as_ref() {
            let has_return_exceptions = call.arguments.keywords.iter().any(|keyword| {
                keyword
                    .arg
                    .as_ref()
                    .map(ruff_python_ast::Identifier::as_str)
                    == Some("return_exceptions")
                    && matches!(&keyword.value, Expr::BooleanLiteral(value) if value.value)
            });
            if attribute.attr.as_str() == "gather"
                && has_return_exceptions
                && self
                    .capabilities
                    .is_not_supported(GATHER_EXCEPTIONS_FEATURE)
            {
                self.not_lowerable(
                    "async_gather_return_exceptions",
                    call.range(),
                    "the target cannot return raised exception objects as gather results",
                );
                return true;
            }
        }
        false
    }

    fn lower_static_bytes(&mut self, call: &ExprCall) -> bool {
        if !self.capabilities.is_not_supported(BYTES_ITERABLE_FEATURE)
            || expr_name(&call.func) != Some("bytes")
            || call.arguments.args.len() != 1
            || !call.arguments.keywords.is_empty()
        {
            return false;
        }
        let Some(argument) = call.arguments.args.first() else {
            return false;
        };
        let Some(replacement) = static_bytes_literal(argument) else {
            return false;
        };
        self.replace_expression(
            "bytes_iterable",
            call.range(),
            replacement,
            "lowered a static integer byte sequence to an equivalent bytes literal",
        )
    }

    fn lower_unicode_decimal_int(&mut self, call: &ExprCall) -> bool {
        if !self
            .capabilities
            .is_not_supported(INT_UNICODE_DECIMAL_FEATURE)
            || expr_name(&call.func) != Some("int")
            || call.arguments.args.len() != 1
            || !call.arguments.keywords.is_empty()
        {
            return false;
        }
        let Some(argument) = call.arguments.args.first() else {
            return false;
        };
        if !matches!(argument, Expr::StringLiteral(_)) {
            return false;
        }
        let Some(source) = self.slice(argument.range()) else {
            self.invalid_source_range("int_unicode_decimal", call.range());
            return false;
        };
        let Some(replacement) = normalize_unicode_decimal_literal(source) else {
            return false;
        };
        self.replace_expression(
            "int_unicode_decimal",
            argument.range(),
            replacement,
            "normalized static Unicode decimal digits to their ASCII equivalents",
        )
    }

    fn lower_callable_iterator_list(&mut self, call: &ExprCall) -> bool {
        if !self
            .capabilities
            .is_not_supported(ITER_CALLABLE_STOP_FEATURE)
            || expr_name(&call.func) != Some("list")
            || call.arguments.args.len() != 1
            || !call.arguments.keywords.is_empty()
        {
            return false;
        }
        let Some(Expr::Call(iterator)) = call.arguments.args.first() else {
            return false;
        };
        if expr_name(&iterator.func) != Some("iter")
            || iterator.arguments.args.len() != 2
            || !iterator.arguments.keywords.is_empty()
        {
            return false;
        }
        let (Some(function), Some(sentinel)) = (
            iterator.arguments.args.first(),
            iterator.arguments.args.get(1),
        ) else {
            return false;
        };
        let Some(function_source) = self.slice(function.range()).map(str::to_owned) else {
            self.invalid_source_range("iter_callable_stop_iteration", call.range());
            return false;
        };
        let Some(sentinel_source) = self.slice(sentinel.range()).map(str::to_owned) else {
            self.invalid_source_range("iter_callable_stop_iteration", call.range());
            return false;
        };
        let Some(helper) = self.request_helper(HelperKind::CallableIteratorList) else {
            self.name_exhausted("iter_callable_stop_iteration", call.range());
            return false;
        };
        self.replace_expression(
            "iter_callable_stop_iteration",
            call.range(),
            format!("{helper}(({function_source}), ({sentinel_source}))"),
            "lowered eager callable-iterator consumption with StopIteration handling",
        )
    }

    fn lower_super_call(&mut self, call: &ExprCall) -> bool {
        if !self.capabilities.is_parse_unsupported(CLASS_SUPER_FEATURE) {
            return false;
        }
        let Expr::Attribute(attribute) = call.func.as_ref() else {
            return false;
        };
        let Expr::Call(super_call) = attribute.value.as_ref() else {
            return false;
        };
        if expr_name(&super_call.func) != Some("super")
            || !super_call.arguments.args.is_empty()
            || !super_call.arguments.keywords.is_empty()
        {
            return false;
        }
        let Some(class_name) = self.class_stack.last().map(|class| class.name.clone()) else {
            self.not_lowerable(
                "super",
                call.range(),
                "zero-argument super() appears outside a resolved class body",
            );
            return false;
        };
        let Some(base_name) = self
            .facts
            .class(&class_name)
            .and_then(|class| class.bases().first())
            .cloned()
        else {
            self.not_lowerable(
                "super",
                call.range(),
                "super() base class could not be resolved statically",
            );
            return false;
        };
        let Some(receiver) = self.receiver_stack.last().and_then(Clone::clone) else {
            self.not_lowerable(
                "super",
                call.range(),
                "super() method receiver could not be resolved statically",
            );
            return false;
        };
        let function_end = to_offset(call.func.end());
        let call_end = to_offset(call.end());
        let Some(tail) = self.source.get(function_end..call_end) else {
            self.invalid_source_range("super", call.range());
            return false;
        };
        let Some(relative_open) = tail.find('(') else {
            self.invalid_source_range("super", call.range());
            return false;
        };
        let Some(relative_after_open) = relative_open.checked_add(1) else {
            self.invalid_source_range("super", call.range());
            return false;
        };
        let Some(insertion) = function_end.checked_add(relative_after_open) else {
            self.invalid_source_range("super", call.range());
            return false;
        };
        self.edits.push(Edit {
            start: to_offset(super_call.start()),
            end: to_offset(super_call.end()),
            replacement: base_name,
        });
        self.edits.push(Edit {
            start: insertion,
            end: insertion,
            replacement: format!("{receiver}, "),
        });
        self.applied(
            "super",
            call.range(),
            "lowered zero-argument super() to an explicit base-method call",
        );
        true
    }

    fn inject_classmethod_argument(
        &mut self,
        call: &ExprCall,
        class: &str,
        is_instance: bool,
    ) -> bool {
        if is_instance {
            let Expr::Attribute(attribute) = call.func.as_ref() else {
                self.invalid_source_range("classmethod", call.range());
                return false;
            };
            let Some(receiver) = self.slice(attribute.value.range()).map(str::to_owned) else {
                self.invalid_source_range("classmethod", call.range());
                return false;
            };
            self.edits.push(Edit {
                start: to_offset(attribute.value.start()),
                end: to_offset(attribute.value.end()),
                replacement: format!("(({receiver}), {class})[1]"),
            });
        }
        let function_end = to_offset(call.func.end());
        let call_end = to_offset(call.end());
        let Some(tail) = self.source.get(function_end..call_end) else {
            self.invalid_source_range("classmethod", call.range());
            return false;
        };
        let Some(relative_open) = tail.find('(') else {
            self.invalid_source_range("classmethod", call.range());
            return false;
        };
        let Some(relative_after_open) = relative_open.checked_add(1) else {
            self.invalid_source_range("classmethod", call.range());
            return false;
        };
        let Some(insertion) = function_end.checked_add(relative_after_open) else {
            self.invalid_source_range("classmethod", call.range());
            return false;
        };
        self.edits.push(Edit {
            start: insertion,
            end: insertion,
            replacement: format!("{class}, "),
        });
        self.applied(
            "classmethod",
            call.range(),
            "lowered resolved classmethod binding to an explicit class argument and class lookup",
        );
        true
    }

    fn lower_getitem(&mut self, subscript: &ruff_python_ast::ExprSubscript) -> bool {
        if !self.capabilities.is_not_supported(PROTOCOL_GETITEM_FEATURE) {
            return false;
        }
        let Some(class) = self.facts.class_for_expr(&subscript.value) else {
            return false;
        };
        if self.facts.resolves_method(class, "__getitem__").is_none() {
            return false;
        }
        let Some(value) = self.slice(subscript.value.range()).map(str::to_owned) else {
            self.invalid_source_range("protocol_getitem", subscript.range());
            return false;
        };
        let Some(key) = self.slice(subscript.slice.range()).map(str::to_owned) else {
            self.invalid_source_range("protocol_getitem", subscript.range());
            return false;
        };
        self.replace_expression(
            "protocol_getitem",
            subscript.range(),
            format!("({value}).__getitem__(({key}))"),
            "lowered resolved user-class subscription to __getitem__",
        )
    }

    fn lower_property_access(&mut self, attribute: &ruff_python_ast::ExprAttribute) -> bool {
        let Some(class) = self.facts.class_for_expr(&attribute.value) else {
            return false;
        };
        let Some((_, MethodKind::Property)) =
            self.facts.resolves_method(class, attribute.attr.as_str())
        else {
            return false;
        };
        let Some(value) = self.slice(attribute.value.range()).map(str::to_owned) else {
            self.invalid_source_range("property", attribute.range());
            return false;
        };
        self.replace_expression(
            "property",
            attribute.range(),
            format!("({value}).{}()", attribute.attr),
            "lowered resolved property access to its getter method",
        )
    }

    fn lower_getattr_access(&mut self, attribute: &ruff_python_ast::ExprAttribute) -> bool {
        if !self.capabilities.is_not_supported(CLASS_GETATTR_FEATURE)
            || attribute.attr.as_str() == "__getattr__"
        {
            return false;
        }
        let Some(class) = self.facts.class_for_expr(&attribute.value) else {
            return false;
        };
        if self.facts.resolves_method(class, "__getattr__").is_none()
            || self.facts.resolves_method(class, "__init__").is_some()
            || self
                .facts
                .resolves_method(class, attribute.attr.as_str())
                .is_some()
            || self.class_has_member(class, attribute.attr.as_str())
        {
            return false;
        }
        let Some(receiver) = self.slice(attribute.value.range()).map(str::to_owned) else {
            self.invalid_source_range("class_getattr", attribute.range());
            return false;
        };
        self.replace_expression(
            "class_getattr",
            attribute.range(),
            format!("({receiver}).__getattr__({:?})", attribute.attr.as_str()),
            "lowered a provably missing user-class attribute to __getattr__",
        )
    }

    fn class_has_member(&self, class: &str, member: &str) -> bool {
        let mut current = self.facts.canonical_class(class);
        let mut visited = HashSet::new();
        while let Some(name) = current {
            if !visited.insert(name.to_owned()) {
                return true;
            }
            let Some(info) = self.facts.class(name) else {
                return true;
            };
            if info
                .members()
                .iter()
                .any(|candidate| candidate.name.as_deref() == Some(member))
            {
                return true;
            }
            current = info
                .bases()
                .first()
                .and_then(|base| self.facts.canonical_class(base));
        }
        false
    }

    fn lower_compare(&mut self, compare: &ExprCompare) -> bool {
        let ([operator], [right]) = (&*compare.ops, &*compare.comparators) else {
            return false;
        };
        if self.lower_bound_method_equality(compare, *operator, right) {
            return true;
        }
        if self.lower_nan_sequence_compare(compare, *operator, right) {
            return true;
        }
        let Some(left_source) = self.slice(compare.left.range()).map(str::to_owned) else {
            self.invalid_source_range("protocol_compare", compare.range());
            return false;
        };
        let Some(right_source) = self.slice(right.range()).map(str::to_owned) else {
            self.invalid_source_range("protocol_compare", compare.range());
            return false;
        };
        if matches!(operator, CmpOp::In | CmpOp::NotIn)
            && self
                .capabilities
                .is_not_supported(PROTOCOL_CONTAINS_FEATURE)
            && let Some(class) = self.facts.class_for_expr(right)
            && self.facts.resolves_method(class, "__contains__").is_some()
        {
            let call = format!("({right_source}).__contains__(({left_source}))");
            let replacement = if *operator == CmpOp::NotIn {
                format!("not ({call})")
            } else {
                call
            };
            return self.replace_expression(
                "protocol_contains",
                compare.range(),
                replacement,
                "lowered resolved membership test to __contains__",
            );
        }

        let (feature, primary_method, reflected_method, negate) = match operator {
            CmpOp::Eq => (PROTOCOL_EQUALITY_FEATURE, "__eq__", "__eq__", false),
            CmpOp::NotEq => (PROTOCOL_EQUALITY_FEATURE, "__ne__", "__ne__", false),
            CmpOp::Lt => (PROTOCOL_ORDERING_FEATURE, "__lt__", "__gt__", false),
            CmpOp::LtE => (PROTOCOL_ORDERING_FEATURE, "__le__", "__ge__", false),
            CmpOp::Gt => (PROTOCOL_ORDERING_FEATURE, "__gt__", "__lt__", false),
            CmpOp::GtE => (PROTOCOL_ORDERING_FEATURE, "__ge__", "__le__", false),
            CmpOp::Is | CmpOp::IsNot | CmpOp::In | CmpOp::NotIn => return false,
        };
        if !self.capabilities.is_not_supported(feature) {
            return false;
        }
        let replacement = if let Some(class) = self.facts.class_for_expr(&compare.left) {
            if self.facts.resolves_method(class, primary_method).is_some() {
                Some(format!(
                    "({left_source}).{primary_method}(({right_source}))"
                ))
            } else if *operator == CmpOp::NotEq
                && self.facts.resolves_method(class, "__eq__").is_some()
            {
                Some(format!("not (({left_source}).__eq__(({right_source})))"))
            } else {
                None
            }
        } else if let Some(class) = self.facts.class_for_expr(right) {
            self.facts
                .resolves_method(class, reflected_method)
                .map(|_| format!("({right_source}).{reflected_method}(({left_source}))"))
        } else {
            None
        };
        let Some(mut replacement) = replacement else {
            return false;
        };
        if negate {
            replacement = format!("not ({replacement})");
        }
        self.replace_expression(
            "protocol_compare",
            compare.range(),
            replacement,
            "lowered resolved user-class comparison to its protocol method",
        )
    }

    fn lower_nan_sequence_compare(
        &mut self,
        compare: &ExprCompare,
        operator: CmpOp,
        right: &Expr,
    ) -> bool {
        if !self
            .capabilities
            .is_not_supported(NAN_SHARED_SEQUENCE_FEATURE)
            || !matches!(operator, CmpOp::Lt | CmpOp::LtE | CmpOp::Gt | CmpOp::GtE)
        {
            return false;
        }
        let (left_elements, right_elements): (&[Expr], &[Expr]) = match (&*compare.left, right) {
            (Expr::List(left), Expr::List(right)) => (&left.elts, &right.elts),
            (Expr::Tuple(left), Expr::Tuple(right)) => (&left.elts, &right.elts),
            _ => return false,
        };
        let has_shared_float = left_elements
            .iter()
            .zip(right_elements)
            .any(|(left, right)| {
                let (Some(left_name), Some(right_name)) = (expr_name(left), expr_name(right))
                else {
                    return false;
                };
                left_name == right_name
                    && self.facts.builtin_for_expr(left) == Some(BuiltinKind::Float)
            });
        if !has_shared_float {
            return false;
        }
        let Some(left) = self.slice(compare.left.range()).map(str::to_owned) else {
            self.invalid_source_range("nan_shared_sequence", compare.range());
            return false;
        };
        let Some(right) = self.slice(right.range()).map(str::to_owned) else {
            self.invalid_source_range("nan_shared_sequence", compare.range());
            return false;
        };
        let operator = match operator {
            CmpOp::Lt => "<",
            CmpOp::LtE => "<=",
            CmpOp::Gt => ">",
            CmpOp::GtE => ">=",
            _ => return false,
        };
        let Some(helper) = self.request_helper(HelperKind::SequenceCompare) else {
            self.name_exhausted("nan_shared_sequence", compare.range());
            return false;
        };
        self.replace_expression(
            "nan_shared_sequence",
            compare.range(),
            format!("{helper}(({left}), ({right}), {operator:?})"),
            "lowered shared-NaN sequence ordering with Python's identity shortcut",
        )
    }

    fn lower_bound_method_equality(
        &mut self,
        compare: &ExprCompare,
        operator: CmpOp,
        right: &Expr,
    ) -> bool {
        if !self
            .capabilities
            .is_not_supported(CLASS_BOUND_METHOD_EQUALITY_FEATURE)
            || !matches!(operator, CmpOp::Eq | CmpOp::NotEq)
        {
            return false;
        }
        let (Expr::Attribute(left_attribute), Expr::Attribute(right_attribute)) =
            (compare.left.as_ref(), right)
        else {
            return false;
        };
        let (Some(left_name), Some(right_name)) = (
            expr_name(&left_attribute.value),
            expr_name(&right_attribute.value),
        ) else {
            return false;
        };
        let (Some(left_class), Some(right_class)) = (
            self.facts.class_for_expr(&left_attribute.value),
            self.facts.class_for_expr(&right_attribute.value),
        ) else {
            return false;
        };
        let left_method = self
            .facts
            .resolves_method(left_class, left_attribute.attr.as_str());
        let right_method = self
            .facts
            .resolves_method(right_class, right_attribute.attr.as_str());
        let (Some((left_owner, MethodKind::Instance)), Some((right_owner, MethodKind::Instance))) =
            (left_method, right_method)
        else {
            return false;
        };
        if left_owner != right_owner
            || left_attribute.attr.as_str() != right_attribute.attr.as_str()
        {
            return false;
        }
        let identity = format!("({left_name}) is ({right_name})");
        let replacement = if operator == CmpOp::NotEq {
            format!("not ({identity})")
        } else {
            identity
        };
        self.replace_expression(
            "bound_method_equality",
            compare.range(),
            replacement,
            "lowered resolved bound-method equality to receiver identity",
        )
    }

    fn attribute_receiver_class(&self, receiver: &Expr) -> Option<(&str, bool)> {
        if let Some(name) = expr_name(receiver)
            && let Some(canonical) = self.facts.canonical_class(name)
        {
            return Some((canonical, false));
        }
        self.facts
            .class_for_expr(receiver)
            .map(|class| (class, true))
    }

    fn replace_expression(
        &mut self,
        rule: &'static str,
        range: ruff_text_size::TextRange,
        replacement: String,
        message: &'static str,
    ) -> bool {
        self.edits.push(Edit {
            start: to_offset(range.start()),
            end: to_offset(range.end()),
            replacement,
        });
        self.applied(rule, range, message);
        true
    }

    fn request_helper(&mut self, kind: HelperKind) -> Option<String> {
        if let Some(existing) = self.helpers.get(&kind) {
            return Some(existing.name.clone());
        }
        let base = match &kind {
            HelperKind::CallableIteratorList => "callable_iterator_list",
            HelperKind::ClassComprehension(_) => "class_comprehension",
            HelperKind::DictUnion => "dict_union",
            HelperKind::GatherReturnExceptions => "gather_return_exceptions",
            HelperKind::IteratorList => "iterator_list",
            HelperKind::LateBoundIdentityLambdas => "late_bound_identity_lambdas",
            HelperKind::SequenceCompare => "sequence_compare",
            HelperKind::UserFormat => "user_format",
        };
        let name = self.next_helper_name(base)?;
        let source = match &kind {
            HelperKind::CallableIteratorList => format!(
                "def {name}(function, sentinel):\n    result = []\n    while True:\n        try:\n            value = function()\n        except StopIteration:\n            break\n        if value == sentinel:\n            break\n        result.append(value)\n    return result\n\n"
            ),
            HelperKind::ClassComprehension(expression) => {
                format!("def {name}():\n    return {expression}\n\n")
            }
            HelperKind::DictUnion => format!(
                "def {name}(left, right):\n    if isinstance(left, dict) and isinstance(right, dict):\n        result = left.copy()\n        result.update(right)\n        return result\n    return left | right\n\n"
            ),
            HelperKind::GatherReturnExceptions => format!(
                "async def {name}(awaitable):\n    try:\n        return await awaitable\n    except Exception as error:\n        return error\n\n"
            ),
            HelperKind::IteratorList => format!(
                "def {name}(value):\n    iterator = value.__iter__()\n    result = []\n    while True:\n        try:\n            result.append(iterator.__next__())\n        except StopIteration:\n            break\n    return result\n\n"
            ),
            HelperKind::LateBoundIdentityLambdas => format!(
                "def {name}(iterable):\n    cell = [None]\n    functions = []\n    for value in iterable:\n        cell[0] = value\n        functions.append(lambda: cell[0])\n    return functions\n\n"
            ),
            HelperKind::SequenceCompare => format!(
                "def {name}(left, right, operator):\n    limit = min(len(left), len(right))\n    index = 0\n    while index < limit:\n        left_item = left[index]\n        right_item = right[index]\n        if not (left_item is right_item or left_item == right_item):\n            if operator == '<':\n                return left_item < right_item\n            if operator == '<=':\n                return left_item <= right_item\n            if operator == '>':\n                return left_item > right_item\n            return left_item >= right_item\n        index += 1\n    if operator == '<':\n        return len(left) < len(right)\n    if operator == '<=':\n        return len(left) <= len(right)\n    if operator == '>':\n        return len(left) > len(right)\n    return len(left) >= len(right)\n\n"
            ),
            HelperKind::UserFormat => format!(
                "def {name}(value, spec):\n    if hasattr(value, '__format__'):\n        return value.__format__(spec)\n    raise TypeError('unsupported format string passed to object.__format__')\n\n"
            ),
        };
        self.helpers.insert(
            kind,
            HelperDefinition {
                name: name.clone(),
                source,
            },
        );
        Some(name)
    }

    fn next_helper_name(&mut self, role: &str) -> Option<String> {
        for _ in 0..=self.source.len() {
            let name = format!("_monty_compat_{role}_{}", self.helper_counter);
            self.helper_counter = self.helper_counter.checked_add(1)?;
            if !self.source.contains(&name)
                && self.helpers.values().all(|helper| helper.name != name)
            {
                return Some(name);
            }
        }
        None
    }

    fn applied(
        &mut self,
        rule: &'static str,
        range: ruff_text_size::TextRange,
        message: &'static str,
    ) {
        self.diagnostics.push(LoweringDiagnostic {
            rule,
            disposition: DiagnosticDisposition::Applied,
            start: to_offset(range.start()),
            end: to_offset(range.end()),
            message: message.to_owned(),
        });
    }

    fn not_lowerable(
        &mut self,
        rule: &'static str,
        range: ruff_text_size::TextRange,
        message: &'static str,
    ) {
        self.diagnostics.push(LoweringDiagnostic {
            rule,
            disposition: DiagnosticDisposition::NotLowerable,
            start: to_offset(range.start()),
            end: to_offset(range.end()),
            message: message.to_owned(),
        });
    }

    fn block_body_insertion(&self, body: &[Stmt]) -> Option<(usize, String)> {
        let first = body.first()?;
        let start = logical_statement_start(first);
        let line_start = line_start(self.source, start)?;
        let indent = self.source.get(line_start..start)?;
        indent
            .chars()
            .all(is_indent_char)
            .then(|| (start, indent.to_owned()))
    }

    fn needs_block_suite(&mut self, rule: &'static str, range: ruff_text_size::TextRange) {
        self.diagnostics.push(LoweringDiagnostic {
            rule,
            disposition: DiagnosticDisposition::NeedsReview,
            start: to_offset(range.start()),
            end: to_offset(range.end()),
            message: "single-line suites are not automatically rewritten".to_owned(),
        });
    }

    fn next_temp_name(&mut self) -> Option<String> {
        for _ in 0..=self.source.len() {
            let name = format!("_monty_compat_target_{}", self.temp_counter);
            self.temp_counter = self.temp_counter.checked_add(1)?;
            if !self.source.contains(&name) {
                return Some(name);
            }
        }
        None
    }

    fn next_decorator_name(&mut self) -> Option<String> {
        for _ in 0..=self.source.len() {
            let name = format!("_monty_compat_decorator_{}", self.decorator_counter);
            self.decorator_counter = self.decorator_counter.checked_add(1)?;
            if !self.source.contains(&name) {
                return Some(name);
            }
        }
        None
    }

    fn slice(&self, range: ruff_text_size::TextRange) -> Option<&str> {
        self.source
            .get(to_offset(range.start())..to_offset(range.end()))
    }

    fn invalid_source_range(&mut self, rule: &'static str, range: ruff_text_size::TextRange) {
        self.diagnostics.push(LoweringDiagnostic {
            rule,
            disposition: DiagnosticDisposition::NeedsReview,
            start: to_offset(range.start()),
            end: to_offset(range.end()),
            message: "parser returned a source range that is not a valid UTF-8 slice".to_owned(),
        });
    }

    fn name_exhausted(&mut self, rule: &'static str, range: ruff_text_size::TextRange) {
        self.diagnostics.push(LoweringDiagnostic {
            rule,
            disposition: DiagnosticDisposition::NeedsReview,
            start: to_offset(range.start()),
            end: to_offset(range.end()),
            message: "unable to allocate a collision-free generated name".to_owned(),
        });
    }
}

impl<'ast> Visitor<'ast> for Collector<'_> {
    fn visit_stmt(&mut self, statement: &'ast Stmt) {
        self.diagnose_unlowerable_statement(statement);
        if let Stmt::With(with_statement) = statement
            && self.lower_async_with_non_raising_return(with_statement)
        {
            return;
        }
        if let Stmt::With(with_statement) = statement
            && self.lower_with_exit_bound_once(with_statement)
        {
            return;
        }
        if let Stmt::ImportFrom(import) = statement
            && self.lower_dataclass_import(import)
        {
            return;
        }
        if let Stmt::Match(match_statement) = statement
            && self.lower_match_statement(match_statement)
        {
            return;
        }
        if let Stmt::If(if_statement) = statement {
            if self.lower_class_body_if(if_statement) {
                return;
            }
            if self.lower_dead_module_if(if_statement) {
                return;
            }
        }
        if let Stmt::ClassDef(class) = statement {
            if self.lower_nested_class(class) {
                return;
            }
            self.lower_class_definition(class);
            let logical_start = logical_statement_start(statement);
            let Some(edit_start) = line_start(self.source, logical_start) else {
                self.invalid_source_range("class_definition", class.range());
                return;
            };
            let Some(indent) = self
                .source
                .get(edit_start..logical_start)
                .map(str::to_owned)
            else {
                self.invalid_source_range("class_definition", class.range());
                return;
            };
            let Some(depth) = self.class_depth.checked_add(1) else {
                self.name_exhausted("class_nesting", class.range());
                return;
            };
            self.class_depth = depth;
            self.class_stack.push(ClassContext {
                name: class.name.to_string(),
                edit_start,
                indent,
            });
            walk_stmt(self, statement);
            let _ = self.class_stack.pop();
            self.class_depth = self.class_depth.saturating_sub(1);
            return;
        }
        if let Stmt::Assign(assign) = statement {
            if self.lower_class_tuple_assignment(assign) {
                return;
            }
            if self.lower_class_name_assignment(assign) {
                return;
            }
            if self.lower_setattr_assignment(assign) {
                return;
            }
            if self.lower_setitem_assignment(assign) {
                return;
            }
        }
        if let Stmt::Delete(delete) = statement
            && self.lower_delete(delete)
        {
            return;
        }
        if let Stmt::Assert(assert) = statement
            && self.lower_assert(assert)
        {
            return;
        }
        if let Stmt::FunctionDef(function) = statement {
            self.lower_function_decorator(function);
            let receiver = function
                .parameters
                .iter()
                .next()
                .map(|parameter| parameter.name().to_string());
            self.receiver_stack.push(receiver);
            walk_stmt(self, statement);
            let _ = self.receiver_stack.pop();
            return;
        }
        match statement {
            Stmt::For(statement) => self.lower_for_target(statement),
            Stmt::With(statement) => self.lower_with_target(statement),
            _ => {}
        }
        walk_stmt(self, statement);
    }

    fn visit_expr(&mut self, expression: &'ast Expr) {
        if self.lower_class_body_comprehension(expression) {
            return;
        }
        if self.lower_late_bound_identity_lambdas(expression) {
            return;
        }
        if self.lower_dead_generator_expression(expression) {
            return;
        }
        if self.diagnose_unlowerable_expression(expression) {
            return;
        }
        if self.lower_expression(expression) {
            return;
        }
        walk_expr(self, expression);
    }
}

fn inject_helpers<'a>(
    source: &str,
    helpers: impl Iterator<Item = &'a HelperDefinition>,
) -> Result<String, LoweringError> {
    let helpers: Vec<_> = helpers.collect();
    if helpers.is_empty() {
        return Ok(source.to_owned());
    }
    let parsed = parse_module(source).map_err(|error| LoweringError::Parse(error.to_string()))?;
    let module = parsed.into_syntax();
    let mut insertion = 0usize;
    let mut statements = module.body.iter().peekable();
    if let Some(Stmt::Expr(expression_statement)) = statements.peek()
        && matches!(expression_statement.value.as_ref(), Expr::StringLiteral(_))
    {
        insertion = line_end_including_newline(source, to_offset(expression_statement.end()))
            .ok_or_else(|| {
                LoweringError::HelperInjection(
                    "module docstring has an invalid source range".to_owned(),
                )
            })?;
        let _ = statements.next();
    }
    while let Some(Stmt::ImportFrom(import)) = statements.peek() {
        if import
            .module
            .as_ref()
            .map(ruff_python_ast::Identifier::as_str)
            != Some("__future__")
        {
            break;
        }
        insertion =
            line_end_including_newline(source, to_offset(import.end())).ok_or_else(|| {
                LoweringError::HelperInjection(
                    "future import has an invalid source range".to_owned(),
                )
            })?;
        let _ = statements.next();
    }

    let prefix = source.get(..insertion).ok_or_else(|| {
        LoweringError::HelperInjection("helper insertion is not a UTF-8 boundary".to_owned())
    })?;
    let suffix = source.get(insertion..).ok_or_else(|| {
        LoweringError::HelperInjection("helper insertion is not a UTF-8 boundary".to_owned())
    })?;
    let helper_size = helpers
        .iter()
        .map(|helper| helper.source.len())
        .fold(0usize, usize::saturating_add);
    let mut output = String::with_capacity(source.len().saturating_add(helper_size));
    output.push_str(prefix);
    if !prefix.is_empty() && !prefix.ends_with('\n') {
        output.push('\n');
    }
    for helper in helpers {
        output.push_str(&helper.source);
    }
    output.push_str(suffix);
    Ok(output)
}

fn complex_target_feature<'a>(
    target: &Expr,
    attribute_feature: &'a str,
    subscript_feature: &'a str,
) -> Option<&'a str> {
    match target {
        Expr::Attribute(_) => Some(attribute_feature),
        Expr::Subscript(_) => Some(subscript_feature),
        _ => None,
    }
}

fn with_target_feature(target: &Expr) -> Option<&'static str> {
    match target {
        Expr::Attribute(_) => Some(WITH_ATTRIBUTE_TARGET_FEATURE),
        Expr::Subscript(_) => Some(WITH_SUBSCRIPT_TARGET_FEATURE),
        Expr::List(list) if list.elts.is_empty() => Some(WITH_EMPTY_LIST_TARGET_FEATURE),
        Expr::Tuple(tuple) if tuple.elts.is_empty() => Some(WITH_EMPTY_TUPLE_TARGET_FEATURE),
        _ => None,
    }
}

fn mangle_private_name(class_name: &str, identifier: &str) -> Option<String> {
    if !identifier.starts_with("__") || identifier.ends_with("__") || identifier.contains('.') {
        return None;
    }
    let class_name = class_name.trim_start_matches('_');
    if class_name.is_empty() {
        return None;
    }
    Some(format!("_{class_name}{identifier}"))
}

#[derive(Default)]
struct LambdaFinder {
    found: bool,
}

impl<'ast> Visitor<'ast> for LambdaFinder {
    fn visit_expr(&mut self, expression: &'ast Expr) {
        if matches!(expression, Expr::Lambda(_)) {
            self.found = true;
        } else {
            walk_expr(self, expression);
        }
    }
}

fn expression_contains_lambda(expression: &Expr) -> bool {
    let mut finder = LambdaFinder::default();
    walk_expr(&mut finder, expression);
    finder.found
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

fn reindent_suite(source: &str, body: &[Stmt], new_indent: &str) -> Option<String> {
    let first = body.first()?;
    let last = body.last()?;
    let first_start = logical_statement_start(first);
    let start = line_start(source, first_start)?;
    let end = line_end_including_newline(source, to_offset(last.end()))?;
    let old_indent = source.get(start..first_start)?;
    let block = source.get(start..end)?;
    Some(reindent_text(block, old_indent, new_indent))
}

fn reindent_text(block: &str, old_indent: &str, new_indent: &str) -> String {
    let mut output = String::with_capacity(block.len());
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
    output
}

fn push_python_line(output: &mut String, indent: &str, content: &str) {
    output.push_str(indent);
    output.push_str(content);
    output.push('\n');
}

fn static_bytes_literal(expression: &Expr) -> Option<String> {
    let elements: &[Expr] = match expression {
        Expr::List(list) => &list.elts,
        Expr::Tuple(tuple) => &tuple.elts,
        _ => return None,
    };
    let mut output = String::from("b\"");
    for element in elements {
        let Expr::NumberLiteral(number) = element else {
            return None;
        };
        let ruff_python_ast::Number::Int(integer) = &number.value else {
            return None;
        };
        let value = integer.as_u8()?;
        match value {
            b'\\' => output.push_str("\\\\"),
            b'\"' => output.push_str("\\\""),
            0x20..=0x7e => output.push(char::from(value)),
            _ => {
                let escaped = format!("\\x{value:02x}");
                output.push_str(&escaped);
            }
        }
    }
    output.push('"');
    Some(output)
}

fn normalize_unicode_decimal_literal(source: &str) -> Option<String> {
    let _ = simple_string_literal_contents(source)?;
    let mut changed = false;
    let mut output = String::with_capacity(source.len());
    for character in source.chars() {
        if let Some(digit) = unicode_decimal_digit(character) {
            let ascii = char::from(b'0'.saturating_add(digit));
            changed |= ascii != character;
            output.push(ascii);
        } else {
            output.push(character);
        }
    }
    changed.then_some(output)
}

fn unicode_decimal_digit(character: char) -> Option<u8> {
    // Unicode Decimal_Number blocks are ten contiguous scalar values. This
    // table follows the Unicode 16 set supported by current CPython builds.
    const ZEROES: [u32; 67] = [
        0x0030, 0x0660, 0x06f0, 0x07c0, 0x0966, 0x09e6, 0x0a66, 0x0ae6, 0x0b66, 0x0be6, 0x0c66,
        0x0ce6, 0x0d66, 0x0de6, 0x0e50, 0x0ed0, 0x0f20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946,
        0x19d0, 0x1a80, 0x1a90, 0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xa9d0,
        0xa9f0, 0xaa50, 0xabf0, 0xff10, 0x104a0, 0x10d30, 0x11066, 0x110f0, 0x11136, 0x111d0,
        0x112f0, 0x11450, 0x114d0, 0x11650, 0x116c0, 0x11730, 0x118e0, 0x11950, 0x11c50, 0x11d50,
        0x11da0, 0x11f50, 0x16a60, 0x16ac0, 0x16b50, 0x1d7ce, 0x1d7d8, 0x1d7e2, 0x1d7ec, 0x1d7f6,
        0x1e140, 0x1e2f0, 0x1e4f0, 0x1e950,
    ];
    let scalar = u32::from(character);
    ZEROES.iter().find_map(|zero| {
        scalar
            .checked_sub(*zero)
            .filter(|offset| *offset < 10)
            .and_then(|offset| u8::try_from(offset).ok())
    })
}

fn simple_string_literal_contents(source: &str) -> Option<&str> {
    let bytes = source.as_bytes();
    if bytes.len() < 2 || source.contains('\\') {
        return None;
    }
    let quote = *bytes.first()?;
    if !matches!(quote, b'\'' | b'"') || bytes.last().copied() != Some(quote) {
        return None;
    }
    if bytes.get(1).copied() == Some(quote) {
        return None;
    }
    source.get(1..source.len().saturating_sub(1))
}

fn to_offset(size: ruff_text_size::TextSize) -> usize {
    size.to_usize()
}

fn line_start(source: &str, offset: usize) -> Option<usize> {
    source.get(..offset).map(|prefix| {
        prefix
            .rfind('\n')
            .map_or(0, |index| index.saturating_add(1))
    })
}

fn line_end_including_newline(source: &str, offset: usize) -> Option<usize> {
    let suffix = source.get(offset..)?;
    match suffix.find('\n') {
        Some(index) => offset.checked_add(index)?.checked_add(1),
        None => Some(source.len()),
    }
}

fn statement_line_end(source: &str, offset: usize) -> Option<usize> {
    line_end_including_newline(source, offset)
}

const fn is_indent_char(character: char) -> bool {
    matches!(character, ' ' | '\t')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capabilities() -> Result<CapabilityIndex, crate::ManifestError> {
        CapabilityIndex::from_json(
            r#"{
                "target": {"tag": "v0.0.19", "runtime_version": "0.0.19"},
                "behavioral_capabilities": {
                    "features": {
                        "statement.function_decorator": {"status": "unsupported_parse"},
                        "statement.for_attribute_target": {"status": "unsupported_parse"},
                        "statement.for_subscript_target": {"status": "unsupported_parse"},
                        "with.attribute_target": {"status": "unsupported_parse"},
                        "with.subscript_target": {"status": "unsupported_parse"}
                    }
                }
            }"#,
        )
    }

    #[test]
    fn lowers_safe_function_decorators_bottom_up() -> Result<(), Box<dyn Error>> {
        let capabilities = capabilities()?;
        let output = lower_source(
            "@outer\n@tools.inner\ndef value():\n    return 3\n\nvalue()\n",
            &capabilities,
        )?;
        assert_eq!(
            output.code,
            concat!(
                "_monty_compat_decorator_0 = outer\n",
                "_monty_compat_decorator_1 = tools.inner\n",
                "def value():\n",
                "    return 3\n",
                "value = _monty_compat_decorator_0(_monty_compat_decorator_1(value))\n",
                "\n",
                "value()\n",
            )
        );
        assert!(output.changed);
        Ok(())
    }

    #[test]
    fn captures_effectful_decorators_before_the_function_definition() -> Result<(), Box<dyn Error>>
    {
        let source = "@decorate(1)\ndef value():\n    return 3\n";
        let output = lower_source(source, &capabilities()?)?;
        assert_eq!(
            output.code,
            concat!(
                "_monty_compat_decorator_0 = decorate(1)\n",
                "def value():\n",
                "    return 3\n",
                "value = _monty_compat_decorator_0(value)\n",
            )
        );
        Ok(())
    }

    #[test]
    fn lowers_attribute_and_subscript_for_targets() -> Result<(), Box<dyn Error>> {
        let source = concat!(
            "for item.value in values:\n",
            "    consume(item.value)\n",
            "for mapping['last'] in values:\n",
            "    consume(mapping['last'])\n",
        );
        let output = lower_source(source, &capabilities()?)?;
        assert_eq!(
            output.code,
            concat!(
                "for _monty_compat_target_0 in values:\n",
                "    item.value = _monty_compat_target_0\n",
                "    consume(item.value)\n",
                "for _monty_compat_target_1 in values:\n",
                "    mapping['last'] = _monty_compat_target_1\n",
                "    consume(mapping['last'])\n",
            )
        );
        Ok(())
    }

    #[test]
    fn lowers_a_single_with_complex_target() -> Result<(), Box<dyn Error>> {
        let source = "with Context() as item.value:\n    consume(item.value)\n";
        let output = lower_source(source, &capabilities()?)?;
        assert_eq!(
            output.code,
            concat!(
                "with Context() as _monty_compat_target_0:\n",
                "    item.value = _monty_compat_target_0\n",
                "    consume(item.value)\n",
            )
        );
        Ok(())
    }

    #[test]
    fn composes_nested_decorator_and_loop_edits() -> Result<(), Box<dyn Error>> {
        let source = concat!(
            "@decorate\n",
            "def consume_all(item, values):\n",
            "    for item.value in values:\n",
            "        consume(item.value)\n",
        );
        let output = lower_source(source, &capabilities()?)?;
        assert_eq!(
            output.code,
            concat!(
                "_monty_compat_decorator_0 = decorate\n",
                "def consume_all(item, values):\n",
                "    for _monty_compat_target_0 in values:\n",
                "        item.value = _monty_compat_target_0\n",
                "        consume(item.value)\n",
                "consume_all = _monty_compat_decorator_0(consume_all)\n",
            )
        );
        assert!(parse_module(&output.code).is_ok());
        Ok(())
    }

    #[test]
    fn does_not_rewrite_without_exact_manifest_evidence() -> Result<(), Box<dyn Error>> {
        let capabilities = CapabilityIndex::from_json(
            r#"{
                "target": {"tag": "v0.0.19", "runtime_version": "0.0.19"},
                "behavioral_capabilities": {
                    "features": {
                        "statement.for_attribute_target": {"status": "supported"}
                    }
                }
            }"#,
        )?;
        let source = "for item.value in values:\n    pass\n";
        let output = lower_source(source, &capabilities)?;
        assert_eq!(output.code, source);
        assert!(!output.changed);
        Ok(())
    }
}
