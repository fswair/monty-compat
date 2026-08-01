use serde::Serialize;

/// The strongest guarantee the lowering engine can make for a discovered seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoweringAvailability {
    /// Every occurrence represented by the feature has a semantics-preserving rewrite.
    Automatic,
    /// A rewrite exists when conservative static preconditions can be proven.
    Contextual,
    /// Monty's supported surface cannot express the required observable semantics.
    NotLowerable,
}

/// Auditable coverage for one behavioral capability feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FeatureLowering {
    pub feature: &'static str,
    pub availability: LoweringAvailability,
}

use LoweringAvailability::{Automatic, Contextual, NotLowerable};

/// Coverage of every non-supported feature in the Monty v0.0.19 manifest.
///
/// `NotLowerable` is deliberate: emitting a diagnostic is preferable to
/// producing code with observably different Python semantics.
pub const LOWERING_COVERAGE: &[FeatureLowering] = &[
    FeatureLowering {
        feature: "async.for",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "async.gather_return_exceptions",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "async.with",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "builtin.bytes_iterable",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "builtin.enumerate_lazy",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "builtin.filter_lazy",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "builtin.int_unicode_decimal",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "builtin.iter_callable_stop_iteration",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "builtin.map_lazy",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "builtin.zip_lazy",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.assign_name",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.assign_object_class",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "class.body_comprehension_scope",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.body_if",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.body_tuple_assignment",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.bound_method_equality",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.bound_method_type",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.classmethod",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.default_repr_qualified",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.getattr_hook",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.inheritance",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.isinstance_type",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.nested_class",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.private_name_mangling",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.property",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.setattr_hook",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.staticmethod",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.super",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "class.type_identity",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "comprehension.generator_lazy",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "comprehension.generator_type",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "dataclass.basic",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "exception.assert_message",
        availability: Automatic,
    },
    FeatureLowering {
        feature: "exception.explicit_cause",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "expression.ellipsis",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "format.percent_string",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "format.str_format",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "fstring.custom_format",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "fstring.invalid_static_spec_dead_code",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "fstring.user_class_spec",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "function.closure_late_binding",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "match.class",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "match.guard",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "match.literal",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "match.mapping",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "match.or",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "match.sequence",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "operator.dict_union",
        availability: Automatic,
    },
    FeatureLowering {
        feature: "operator.nan_shared_sequence",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.add",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.binary.bitand",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.binary.bitor",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.binary.bitxor",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.binary.floordiv",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.binary.lshift",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.binary.matmul",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.binary.mod",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.binary.mul",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.binary.pow",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.binary.rshift",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.binary.sub",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.binary.truediv",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.callable",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.contains",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.conversion.float",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.conversion.index",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.conversion.int",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.equality",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.getitem",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.hash",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.iterator",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.length",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.ordering",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.reflected.add",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.reflected.bitand",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.reflected.bitor",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.reflected.bitxor",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.reflected.floordiv",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.reflected.lshift",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.reflected.matmul",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.reflected.mod",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.reflected.mul",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.reflected.pow",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.reflected.rshift",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.reflected.sub",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.reflected.truediv",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "protocol.reversed",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.round",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.setitem",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.truthiness",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.unary.invert",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.unary.neg",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "protocol.unary.pos",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "statement.delete",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "statement.delete_attribute",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "statement.delete_name",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "statement.for_attribute_target",
        availability: Automatic,
    },
    FeatureLowering {
        feature: "statement.for_subscript_target",
        availability: Automatic,
    },
    FeatureLowering {
        feature: "statement.function_decorator",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "statement.generator",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "statement.raise_from",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "statement.try_star",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "statement.yield_from",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "with.attribute_target",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "with.empty_list_target",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "with.empty_tuple_target",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "with.exception_arguments",
        availability: NotLowerable,
    },
    FeatureLowering {
        feature: "with.exit_bound_once",
        availability: Contextual,
    },
    FeatureLowering {
        feature: "with.subscript_target",
        availability: Contextual,
    },
];

/// Return the immutable, release-audited lowering coverage table.
#[must_use]
pub const fn lowering_coverage() -> &'static [FeatureLowering] {
    LOWERING_COVERAGE
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::CapabilityIndex;

    #[test]
    fn covers_every_non_supported_release_feature_exactly_once()
    -> Result<(), Box<dyn std::error::Error>> {
        let manifest =
            CapabilityIndex::from_json(include_str!("../../../manifests/monty-v0.0.19.json"))?;
        let expected: HashSet<_> = manifest
            .feature_statuses()
            .filter_map(|(feature, status)| (status != "supported").then_some(feature))
            .collect();
        let actual: HashSet<_> = LOWERING_COVERAGE
            .iter()
            .map(|entry| entry.feature)
            .collect();
        assert_eq!(
            actual.len(),
            LOWERING_COVERAGE.len(),
            "duplicate coverage entry"
        );
        assert_eq!(actual, expected);
        Ok(())
    }
}
