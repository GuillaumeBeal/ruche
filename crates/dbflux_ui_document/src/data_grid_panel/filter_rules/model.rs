//! The rules of the filter rules panel and their translation to a
//! driver-neutral [`SemanticFilter`], kept free of GPUI so they test as plain
//! functions.
//!
//! The operators per column type mirror Sequel Ace's `ContentFilters.plist`.

use dbflux_core::{ColumnKind, ColumnMeta, SemanticFilter, Value, WhereOperator};

/// What a column holds, as far as the operators it is offered go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColumnClass {
    Text,
    Number,
    Date,
    Boolean,
}

impl ColumnClass {
    /// The class of `column`. A boolean is recognised by its type name, the
    /// way the grid's Filter menu recognises one; everything else follows the
    /// driver-reported [`ColumnKind`].
    pub(crate) fn of(column: &ColumnMeta) -> Self {
        if column.type_name.to_ascii_lowercase().contains("bool") {
            return ColumnClass::Boolean;
        }

        match column.kind {
            ColumnKind::Integer | ColumnKind::Float => ColumnClass::Number,
            ColumnKind::Timestamp => ColumnClass::Date,
            _ => ColumnClass::Text,
        }
    }
}

/// The operator of one rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RuleOperator {
    Equals,
    NotEquals,
    Contains,
    NotContains,
    StartsWith,
    NotStartsWith,
    EndsWith,
    NotEndsWith,
    Like,
    NotLike,
    In,
    Between,
    IsEmpty,
    IsNotEmpty,
    IsNull,
    IsNotNull,
    Greater,
    Less,
    GreaterOrEqual,
    LessOrEqual,
    After,
    Before,
    OnOrAfter,
    OnOrBefore,
    IsTrue,
    IsFalse,
}

const TEXT_OPERATORS: &[RuleOperator] = &[
    RuleOperator::Equals,
    RuleOperator::NotEquals,
    RuleOperator::Contains,
    RuleOperator::NotContains,
    RuleOperator::StartsWith,
    RuleOperator::NotStartsWith,
    RuleOperator::EndsWith,
    RuleOperator::NotEndsWith,
    RuleOperator::Like,
    RuleOperator::NotLike,
    RuleOperator::In,
    RuleOperator::Between,
    RuleOperator::IsEmpty,
    RuleOperator::IsNotEmpty,
    RuleOperator::IsNull,
    RuleOperator::IsNotNull,
];

const NUMBER_OPERATORS: &[RuleOperator] = &[
    RuleOperator::Equals,
    RuleOperator::NotEquals,
    RuleOperator::Greater,
    RuleOperator::Less,
    RuleOperator::GreaterOrEqual,
    RuleOperator::LessOrEqual,
    RuleOperator::In,
    RuleOperator::Between,
    RuleOperator::IsNull,
    RuleOperator::IsNotNull,
];

const DATE_OPERATORS: &[RuleOperator] = &[
    RuleOperator::Equals,
    RuleOperator::NotEquals,
    RuleOperator::After,
    RuleOperator::Before,
    RuleOperator::OnOrAfter,
    RuleOperator::OnOrBefore,
    RuleOperator::Between,
    RuleOperator::IsNull,
    RuleOperator::IsNotNull,
];

const BOOLEAN_OPERATORS: &[RuleOperator] = &[
    RuleOperator::IsTrue,
    RuleOperator::IsFalse,
    RuleOperator::IsNull,
    RuleOperator::IsNotNull,
];

/// Every operator offered for `class`, in menu order.
pub(crate) fn operators_for(class: ColumnClass) -> &'static [RuleOperator] {
    match class {
        ColumnClass::Text => TEXT_OPERATORS,
        ColumnClass::Number => NUMBER_OPERATORS,
        ColumnClass::Date => DATE_OPERATORS,
        ColumnClass::Boolean => BOOLEAN_OPERATORS,
    }
}

/// The operators offered for `class` that a connection declaring
/// `where_operators` can render.
pub(crate) fn available_operators(
    class: ColumnClass,
    where_operators: &[WhereOperator],
) -> Vec<RuleOperator> {
    operators_for(class)
        .iter()
        .copied()
        .filter(|operator| operator.supported_by(where_operators))
        .collect()
}

impl RuleOperator {
    /// How many values the operator takes: none for the null, empty and
    /// boolean tests, two for BETWEEN, one otherwise.
    pub(crate) fn value_count(self) -> usize {
        match self {
            RuleOperator::IsEmpty
            | RuleOperator::IsNotEmpty
            | RuleOperator::IsNull
            | RuleOperator::IsNotNull
            | RuleOperator::IsTrue
            | RuleOperator::IsFalse => 0,
            RuleOperator::Between => 2,
            _ => 1,
        }
    }

    /// The semantic operators the rule renders through. The null tests render
    /// as `= NULL` / `<> NULL`, which the SQL renderer turns into `IS NULL` /
    /// `IS NOT NULL`.
    fn required_where_operators(self) -> &'static [WhereOperator] {
        match self {
            RuleOperator::Equals
            | RuleOperator::IsEmpty
            | RuleOperator::IsNull
            | RuleOperator::IsTrue
            | RuleOperator::IsFalse => &[WhereOperator::Eq],
            RuleOperator::NotEquals | RuleOperator::IsNotNull => &[WhereOperator::Ne],
            RuleOperator::IsNotEmpty => &[WhereOperator::Eq, WhereOperator::Not],
            RuleOperator::Contains
            | RuleOperator::StartsWith
            | RuleOperator::EndsWith
            | RuleOperator::Like => &[WhereOperator::Like],
            RuleOperator::NotContains
            | RuleOperator::NotStartsWith
            | RuleOperator::NotEndsWith
            | RuleOperator::NotLike => &[WhereOperator::Like, WhereOperator::Not],
            RuleOperator::In => &[WhereOperator::In],
            RuleOperator::Between => &[WhereOperator::Gte, WhereOperator::Lte, WhereOperator::And],
            RuleOperator::Greater | RuleOperator::After => &[WhereOperator::Gt],
            RuleOperator::Less | RuleOperator::Before => &[WhereOperator::Lt],
            RuleOperator::GreaterOrEqual | RuleOperator::OnOrAfter => &[WhereOperator::Gte],
            RuleOperator::LessOrEqual | RuleOperator::OnOrBefore => &[WhereOperator::Lte],
        }
    }

    /// Whether a connection declaring `where_operators` can render the rule.
    pub(crate) fn supported_by(self, where_operators: &[WhereOperator]) -> bool {
        self.required_where_operators()
            .iter()
            .all(|required| where_operators.contains(required))
    }

    /// Stable, locale-independent key of the operator, used for its label.
    pub(crate) fn key(self) -> &'static str {
        match self {
            RuleOperator::Equals => "equals",
            RuleOperator::NotEquals => "not_equals",
            RuleOperator::Contains => "contains",
            RuleOperator::NotContains => "not_contains",
            RuleOperator::StartsWith => "starts_with",
            RuleOperator::NotStartsWith => "not_starts_with",
            RuleOperator::EndsWith => "ends_with",
            RuleOperator::NotEndsWith => "not_ends_with",
            RuleOperator::Like => "like",
            RuleOperator::NotLike => "not_like",
            RuleOperator::In => "in",
            RuleOperator::Between => "between",
            RuleOperator::IsEmpty => "is_empty",
            RuleOperator::IsNotEmpty => "is_not_empty",
            RuleOperator::IsNull => "is_null",
            RuleOperator::IsNotNull => "is_not_null",
            RuleOperator::Greater => "greater",
            RuleOperator::Less => "less",
            RuleOperator::GreaterOrEqual => "greater_or_equal",
            RuleOperator::LessOrEqual => "less_or_equal",
            RuleOperator::After => "after",
            RuleOperator::Before => "before",
            RuleOperator::OnOrAfter => "on_or_after",
            RuleOperator::OnOrBefore => "on_or_before",
            RuleOperator::IsTrue => "is_true",
            RuleOperator::IsFalse => "is_false",
        }
    }

    /// The operator's label in the current locale.
    pub(crate) fn label(self) -> String {
        dbflux_i18n::translate(&format!(
            "document.data.grid.filter.rules.operators.{}",
            self.key()
        ))
    }
}

/// How the enabled rules combine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Combinator {
    /// AND: a row matches every rule.
    #[default]
    All,
    /// OR: a row matches at least one rule.
    Any,
}

impl Combinator {
    pub(crate) const ALL: [Combinator; 2] = [Combinator::All, Combinator::Any];

    pub(crate) fn index(self) -> usize {
        match self {
            Combinator::All => 0,
            Combinator::Any => 1,
        }
    }

    pub(crate) fn from_index(index: usize) -> Self {
        if index == 1 {
            Combinator::Any
        } else {
            Combinator::All
        }
    }

    pub(crate) fn label(self) -> String {
        match self {
            Combinator::All => dbflux_i18n::t!("document.data.grid.filter.rules.combinator.all"),
            Combinator::Any => dbflux_i18n::t!("document.data.grid.filter.rules.combinator.any"),
        }
    }
}

/// One rule as the panel holds it when the filter is applied.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FilterRule {
    pub(crate) enabled: bool,
    /// The column tested; `None` until the table has columns.
    pub(crate) column: Option<String>,
    /// The driver-reported kind of the column, which types the values.
    pub(crate) kind: ColumnKind,
    pub(crate) operator: RuleOperator,
    pub(crate) value: String,
    /// The upper bound of BETWEEN.
    pub(crate) second_value: String,
}

/// The filter the enabled, complete rules describe, combined by
/// `combinator`. `None` when no rule is both enabled and complete: a disabled
/// rule, a rule without a column, and a rule missing a value it needs are
/// left out.
pub(crate) fn rules_to_semantic_filter(
    rules: &[FilterRule],
    combinator: Combinator,
) -> Option<SemanticFilter> {
    let mut filters: Vec<SemanticFilter> = rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter_map(rule_to_filter)
        .collect();

    if filters.len() <= 1 {
        return filters.pop();
    }

    Some(match combinator {
        Combinator::All => SemanticFilter::and(filters),
        Combinator::Any => SemanticFilter::or(filters),
    })
}

fn rule_to_filter(rule: &FilterRule) -> Option<SemanticFilter> {
    let column = rule.column.as_deref().filter(|name| !name.is_empty())?;
    let value = rule.value.as_str();
    let has_value = !value.trim().is_empty();

    if rule.operator.value_count() >= 1 && !has_value {
        return None;
    }

    let compare =
        |operator: WhereOperator, value: Value| SemanticFilter::compare(column, operator, value);
    let pattern = |pattern: String| compare(WhereOperator::Like, Value::Text(pattern));

    let filter = match rule.operator {
        RuleOperator::Equals => compare(WhereOperator::Eq, typed_value(rule.kind, value)),
        RuleOperator::NotEquals => compare(WhereOperator::Ne, typed_value(rule.kind, value)),
        RuleOperator::Greater | RuleOperator::After => {
            compare(WhereOperator::Gt, typed_value(rule.kind, value))
        }
        RuleOperator::Less | RuleOperator::Before => {
            compare(WhereOperator::Lt, typed_value(rule.kind, value))
        }
        RuleOperator::GreaterOrEqual | RuleOperator::OnOrAfter => {
            compare(WhereOperator::Gte, typed_value(rule.kind, value))
        }
        RuleOperator::LessOrEqual | RuleOperator::OnOrBefore => {
            compare(WhereOperator::Lte, typed_value(rule.kind, value))
        }
        RuleOperator::Contains => pattern(format!("%{value}%")),
        RuleOperator::NotContains => SemanticFilter::negate(pattern(format!("%{value}%"))),
        RuleOperator::StartsWith => pattern(format!("{value}%")),
        RuleOperator::NotStartsWith => SemanticFilter::negate(pattern(format!("{value}%"))),
        RuleOperator::EndsWith => pattern(format!("%{value}")),
        RuleOperator::NotEndsWith => SemanticFilter::negate(pattern(format!("%{value}"))),
        RuleOperator::Like => pattern(value.to_string()),
        RuleOperator::NotLike => SemanticFilter::negate(pattern(value.to_string())),
        RuleOperator::In => {
            let items: Vec<Value> = value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(|item| typed_value(rule.kind, item))
                .collect();

            if items.is_empty() {
                return None;
            }

            compare(WhereOperator::In, Value::Array(items))
        }
        RuleOperator::Between => {
            let upper = rule.second_value.as_str();
            if upper.trim().is_empty() {
                return None;
            }

            SemanticFilter::and(vec![
                compare(WhereOperator::Gte, typed_value(rule.kind, value)),
                compare(WhereOperator::Lte, typed_value(rule.kind, upper)),
            ])
        }
        RuleOperator::IsEmpty => compare(WhereOperator::Eq, Value::Text(String::new())),
        RuleOperator::IsNotEmpty => {
            SemanticFilter::negate(compare(WhereOperator::Eq, Value::Text(String::new())))
        }
        RuleOperator::IsNull => SemanticFilter::null(column),
        RuleOperator::IsNotNull => compare(WhereOperator::Ne, Value::Null),
        RuleOperator::IsTrue => compare(WhereOperator::Eq, Value::Bool(true)),
        RuleOperator::IsFalse => compare(WhereOperator::Eq, Value::Bool(false)),
    };

    Some(filter)
}

/// The literal a typed-in value stands for: a number for a numeric column
/// when the text reads as one, the text itself otherwise (dates included,
/// which the database parses).
pub(crate) fn typed_value(kind: ColumnKind, text: &str) -> Value {
    let trimmed = text.trim();

    match kind {
        ColumnKind::Integer => match trimmed.parse::<i64>() {
            Ok(number) => Value::Int(number),
            Err(_) => Value::Text(text.to_string()),
        },
        // A decimal column reports `Float`, so the validated text is kept as a
        // `Decimal`: an `f64` would round digits past ~15 significant ones.
        ColumnKind::Float => match trimmed.parse::<f64>() {
            Ok(number) if number.is_finite() => Value::Decimal(trimmed.to_string()),
            _ => Value::Text(text.to_string()),
        },
        _ => Value::Text(text.to_string()),
    }
}
