use super::model::{
    ColumnClass, Combinator, FilterRule, RuleOperator, available_operators, operators_for,
    rules_to_semantic_filter, typed_value,
};
use super::{FilterRulesAction, RuleControl};
use crate::data_grid_panel::DataGridPanel;
use crate::data_grid_panel::context_menu::toolbar::ToolbarAction;
use crate::data_grid_panel::table_views::tests::open_table;
use dbflux_core::{
    ColumnKind, ColumnMeta, DefaultSqlDialect, QueryCapabilities, QueryResult, TableViewFeatures,
    Value, WhereOperator, render_semantic_filter_sql,
};
use gpui::{Entity, TestAppContext, VisualTestContext};

fn rule(column: &str, kind: ColumnKind, operator: RuleOperator, value: &str) -> FilterRule {
    FilterRule {
        enabled: true,
        column: Some(column.to_string()),
        kind,
        operator,
        value: value.to_string(),
        second_value: String::new(),
    }
}

fn between(column: &str, kind: ColumnKind, low: &str, high: &str) -> FilterRule {
    FilterRule {
        second_value: high.to_string(),
        ..rule(column, kind, RuleOperator::Between, low)
    }
}

fn sql(rules: &[FilterRule], combinator: Combinator) -> Option<String> {
    rules_to_semantic_filter(rules, combinator).map(|filter| {
        render_semantic_filter_sql(&filter, &DefaultSqlDialect).expect("the filter renders")
    })
}

/// The SQL of one rule on its own.
fn one(rule: FilterRule) -> String {
    sql(&[rule], Combinator::All).expect("a complete rule makes a filter")
}

fn text(operator: RuleOperator, value: &str) -> String {
    one(rule("name", ColumnKind::Text, operator, value))
}

#[test]
fn text_operators_render_their_sql() {
    let cases = [
        (RuleOperator::Equals, "Bob", r#""name" = 'Bob'"#),
        (RuleOperator::NotEquals, "Bob", r#""name" <> 'Bob'"#),
        (RuleOperator::Contains, "Bob", r#""name" LIKE '%Bob%'"#),
        (
            RuleOperator::NotContains,
            "Bob",
            r#"NOT ("name" LIKE '%Bob%')"#,
        ),
        (RuleOperator::StartsWith, "Bob", r#""name" LIKE 'Bob%'"#),
        (
            RuleOperator::NotStartsWith,
            "Bob",
            r#"NOT ("name" LIKE 'Bob%')"#,
        ),
        (RuleOperator::EndsWith, "Bob", r#""name" LIKE '%Bob'"#),
        (
            RuleOperator::NotEndsWith,
            "Bob",
            r#"NOT ("name" LIKE '%Bob')"#,
        ),
        (RuleOperator::Like, "B_b%", r#""name" LIKE 'B_b%'"#),
        (RuleOperator::NotLike, "B_b%", r#"NOT ("name" LIKE 'B_b%')"#),
        (
            RuleOperator::In,
            " a, b ,c ",
            r#""name" IN ('a', 'b', 'c')"#,
        ),
        (RuleOperator::IsEmpty, "", r#""name" = ''"#),
        (RuleOperator::IsNotEmpty, "", r#"NOT ("name" = '')"#),
        (RuleOperator::IsNull, "", r#""name" IS NULL"#),
        (RuleOperator::IsNotNull, "", r#""name" IS NOT NULL"#),
    ];

    for (operator, value, expected) in cases {
        assert_eq!(text(operator, value), expected, "{operator:?}");
    }

    assert_eq!(
        one(between("name", ColumnKind::Text, "a", "m")),
        r#"("name" >= 'a') AND ("name" <= 'm')"#
    );
}

#[test]
fn number_operators_render_unquoted_numbers() {
    let integer = |operator, value| one(rule("age", ColumnKind::Integer, operator, value));

    assert_eq!(integer(RuleOperator::Equals, "18"), r#""age" = 18"#);
    assert_eq!(integer(RuleOperator::NotEquals, "18"), r#""age" <> 18"#);
    assert_eq!(integer(RuleOperator::Greater, " 18 "), r#""age" > 18"#);
    assert_eq!(integer(RuleOperator::Less, "18"), r#""age" < 18"#);
    assert_eq!(
        integer(RuleOperator::GreaterOrEqual, "18"),
        r#""age" >= 18"#
    );
    assert_eq!(integer(RuleOperator::LessOrEqual, "18"), r#""age" <= 18"#);
    assert_eq!(integer(RuleOperator::In, "1, 2,3"), r#""age" IN (1, 2, 3)"#);
    assert_eq!(integer(RuleOperator::IsNull, ""), r#""age" IS NULL"#);
    assert_eq!(
        one(between("age", ColumnKind::Integer, "1", "5")),
        r#"("age" >= 1) AND ("age" <= 5)"#
    );

    assert_eq!(
        one(rule(
            "price",
            ColumnKind::Float,
            RuleOperator::Equals,
            "9.5"
        )),
        r#""price" = 9.5"#
    );
    assert_eq!(
        integer(RuleOperator::Equals, "abc"),
        r#""age" = 'abc'"#,
        "a value that is not a number stays text"
    );
}

#[test]
fn in_types_each_value_by_the_column() {
    assert_eq!(
        one(rule("age", ColumnKind::Integer, RuleOperator::In, "1, x")),
        r#""age" IN (1, 'x')"#
    );
    assert_eq!(
        one(rule("code", ColumnKind::Text, RuleOperator::In, "1, 2")),
        r#""code" IN ('1', '2')"#
    );
}

#[test]
fn date_operators_compare_text_literals() {
    let date = |operator| {
        one(rule(
            "created_at",
            ColumnKind::Timestamp,
            operator,
            "2024-01-01",
        ))
    };

    assert_eq!(date(RuleOperator::Equals), r#""created_at" = '2024-01-01'"#);
    assert_eq!(
        date(RuleOperator::NotEquals),
        r#""created_at" <> '2024-01-01'"#
    );
    assert_eq!(date(RuleOperator::After), r#""created_at" > '2024-01-01'"#);
    assert_eq!(date(RuleOperator::Before), r#""created_at" < '2024-01-01'"#);
    assert_eq!(
        date(RuleOperator::OnOrAfter),
        r#""created_at" >= '2024-01-01'"#
    );
    assert_eq!(
        date(RuleOperator::OnOrBefore),
        r#""created_at" <= '2024-01-01'"#
    );
    assert_eq!(
        one(between(
            "created_at",
            ColumnKind::Timestamp,
            "2024-01-01",
            "2024-12-31"
        )),
        r#"("created_at" >= '2024-01-01') AND ("created_at" <= '2024-12-31')"#
    );
}

#[test]
fn boolean_operators_compare_with_true_and_false() {
    let boolean = |operator| one(rule("active", ColumnKind::Unknown, operator, ""));

    assert_eq!(boolean(RuleOperator::IsTrue), r#""active" = TRUE"#);
    assert_eq!(boolean(RuleOperator::IsFalse), r#""active" = FALSE"#);
    assert_eq!(boolean(RuleOperator::IsNotNull), r#""active" IS NOT NULL"#);
}

#[test]
fn rules_combine_with_and_or_or() {
    let rules = [
        rule("name", ColumnKind::Text, RuleOperator::Equals, "Bob"),
        rule("age", ColumnKind::Integer, RuleOperator::Greater, "18"),
    ];

    assert_eq!(
        sql(&rules, Combinator::All).as_deref(),
        Some(r#"("name" = 'Bob') AND ("age" > 18)"#)
    );
    assert_eq!(
        sql(&rules, Combinator::Any).as_deref(),
        Some(r#"("name" = 'Bob') OR ("age" > 18)"#)
    );
}

#[test]
fn disabled_and_incomplete_rules_are_left_out() {
    let disabled = FilterRule {
        enabled: false,
        ..rule("name", ColumnKind::Text, RuleOperator::Equals, "Bob")
    };
    let no_column = FilterRule {
        column: None,
        ..rule("name", ColumnKind::Text, RuleOperator::Equals, "Bob")
    };
    let empty_value = rule("name", ColumnKind::Text, RuleOperator::Contains, "");
    let blank_value = rule("name", ColumnKind::Text, RuleOperator::Equals, "   ");
    let half_between = between("age", ColumnKind::Integer, "1", " ");
    let empty_in = rule("age", ColumnKind::Integer, RuleOperator::In, " , ,");
    let kept = rule("age", ColumnKind::Integer, RuleOperator::Greater, "18");

    let incomplete = [
        disabled,
        no_column,
        empty_value,
        blank_value,
        half_between,
        empty_in,
    ];

    assert_eq!(sql(&incomplete, Combinator::All), None);

    let mut with_one_complete = incomplete.to_vec();
    with_one_complete.push(kept);
    assert_eq!(
        sql(&with_one_complete, Combinator::Any).as_deref(),
        Some(r#""age" > 18"#),
        "a single remaining rule is not wrapped in a group"
    );
}

#[test]
fn identifiers_are_quoted_and_quotes_in_values_escaped() {
    assert_eq!(
        one(rule(
            "we\"ird col",
            ColumnKind::Text,
            RuleOperator::Equals,
            "x"
        )),
        r#""we""ird col" = 'x'"#
    );
    assert_eq!(
        text(RuleOperator::Equals, "O'Brien"),
        r#""name" = 'O''Brien'"#
    );
    assert_eq!(
        text(RuleOperator::Contains, "O'Brien"),
        r#""name" LIKE '%O''Brien%'"#
    );
    assert_eq!(
        text(RuleOperator::In, "O'Brien, x"),
        r#""name" IN ('O''Brien', 'x')"#
    );
}

#[test]
fn typed_values_follow_the_column_kind() {
    assert_eq!(typed_value(ColumnKind::Integer, " 42 "), Value::Int(42));
    assert_eq!(
        typed_value(ColumnKind::Float, " 1.5 "),
        Value::Decimal("1.5".to_string())
    );
    assert_eq!(
        typed_value(ColumnKind::Float, "12345678901234567.89"),
        Value::Decimal("12345678901234567.89".to_string())
    );
    assert_eq!(
        typed_value(ColumnKind::Float, "NaN"),
        Value::Text("NaN".to_string()),
        "a float literal that is not finite stays text"
    );
    assert_eq!(
        typed_value(ColumnKind::Timestamp, "2024-01-01"),
        Value::Text("2024-01-01".to_string())
    );
    assert_eq!(
        typed_value(ColumnKind::Text, "42"),
        Value::Text("42".to_string())
    );
}

fn column(type_name: &str, kind: ColumnKind) -> ColumnMeta {
    ColumnMeta {
        name: "c".to_string(),
        type_name: type_name.to_string(),
        kind,
        nullable: true,
        is_primary_key: false,
    }
}

#[test]
fn columns_are_classed_by_kind_and_boolean_type_name() {
    assert_eq!(
        ColumnClass::of(&column("BOOLEAN", ColumnKind::Unknown)),
        ColumnClass::Boolean
    );
    assert_eq!(
        ColumnClass::of(&column("int4", ColumnKind::Integer)),
        ColumnClass::Number
    );
    assert_eq!(
        ColumnClass::of(&column("float8", ColumnKind::Float)),
        ColumnClass::Number
    );
    assert_eq!(
        ColumnClass::of(&column("timestamptz", ColumnKind::Timestamp)),
        ColumnClass::Date
    );
    assert_eq!(
        ColumnClass::of(&column("varchar", ColumnKind::Text)),
        ColumnClass::Text
    );
    assert_eq!(
        ColumnClass::of(&column("geometry", ColumnKind::Unknown)),
        ColumnClass::Text
    );
}

#[test]
fn each_column_class_offers_its_operators() {
    use RuleOperator::*;

    assert_eq!(
        operators_for(ColumnClass::Text),
        &[
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
        ]
    );
    assert_eq!(
        operators_for(ColumnClass::Number),
        &[
            Equals,
            NotEquals,
            Greater,
            Less,
            GreaterOrEqual,
            LessOrEqual,
            In,
            Between,
            IsNull,
            IsNotNull,
        ]
    );
    assert_eq!(
        operators_for(ColumnClass::Date),
        &[
            Equals, NotEquals, After, Before, OnOrAfter, OnOrBefore, Between, IsNull, IsNotNull,
        ]
    );
    assert_eq!(
        operators_for(ColumnClass::Boolean),
        &[IsTrue, IsFalse, IsNull, IsNotNull]
    );
}

#[test]
fn operators_a_connection_cannot_render_are_not_offered() {
    let defaults = QueryCapabilities::default().where_operators;
    assert_eq!(
        available_operators(ColumnClass::Text, &defaults),
        operators_for(ColumnClass::Text),
        "the core defaults render every text operator"
    );

    let without_not_and_like: Vec<WhereOperator> = defaults
        .iter()
        .copied()
        .filter(|operator| !matches!(operator, WhereOperator::Not | WhereOperator::Like))
        .collect();
    let offered = available_operators(ColumnClass::Text, &without_not_and_like);

    for missing in [
        RuleOperator::Contains,
        RuleOperator::NotContains,
        RuleOperator::Like,
        RuleOperator::NotLike,
        RuleOperator::IsNotEmpty,
    ] {
        assert!(!offered.contains(&missing), "{missing:?} is offered");
    }
    assert!(offered.contains(&RuleOperator::Equals));
    assert!(offered.contains(&RuleOperator::IsEmpty));
}

#[test]
fn value_counts_match_the_inputs_drawn() {
    assert_eq!(RuleOperator::IsNull.value_count(), 0);
    assert_eq!(RuleOperator::IsTrue.value_count(), 0);
    assert_eq!(RuleOperator::IsNotEmpty.value_count(), 0);
    assert_eq!(RuleOperator::Contains.value_count(), 1);
    assert_eq!(RuleOperator::In.value_count(), 1);
    assert_eq!(RuleOperator::Between.value_count(), 2);
}

#[test]
fn every_rules_label_resolves_in_each_locale() {
    let mut keys: Vec<String> = [
        ColumnClass::Text,
        ColumnClass::Number,
        ColumnClass::Date,
        ColumnClass::Boolean,
    ]
    .iter()
    .flat_map(|class| operators_for(*class))
    .map(|operator| {
        format!(
            "document.data.grid.filter.rules.operators.{}",
            operator.key()
        )
    })
    .collect();

    keys.extend(
        [
            "toggle",
            "apply",
            "apply_failed",
            "column",
            "operator",
            "value",
            "and",
            "remove",
            "add_after",
            "rule_name",
            "combinator.all",
            "combinator.any",
            "menu.show",
            "menu.hide",
            "menu.add_rule",
            "menu.combinator",
            "menu.rule",
            "menu.enable",
            "menu.disable",
            "menu.column",
            "menu.operator",
            "menu.value",
            "menu.second_value",
            "menu.remove",
            "menu.add_after",
        ]
        .map(|key| format!("document.data.grid.filter.rules.{key}")),
    );

    let locales: Vec<&str> = dbflux_i18n::Language::available()
        .iter()
        .map(|language| language.locale_code())
        .collect();
    assert!(locales.len() >= 5, "{locales:?}");

    for key in &keys {
        for locale in &locales {
            let label = dbflux_i18n::translate_in(locale, key);
            assert!(!label.is_empty(), "empty {locale} label for {key}");
            assert!(
                !label.ends_with(key.as_str()),
                "{locale} label missing for {key}: {label}"
            );
        }
    }

    // A catalog without the keys would fall back to English.
    let apply = "document.data.grid.filter.rules.apply";
    let english = dbflux_i18n::translate_in("en", apply);
    for locale in locales.iter().filter(|locale| **locale != "en") {
        assert_ne!(
            dbflux_i18n::translate_in(locale, apply),
            english,
            "{locale} translates the panel"
        );
    }

    assert_eq!(
        dbflux_i18n::translate_in("en", "document.data.grid.filter.rules.operators.contains"),
        "contains"
    );
    assert_eq!(
        dbflux_i18n::translate_in("es", "document.data.grid.filter.rules.operators.contains"),
        "contiene"
    );
}

#[test]
fn rules_menu_entry_ids_are_stable() {
    assert_eq!(FilterRulesAction::Toggle.id(), "filter-rules");
    assert_eq!(FilterRulesAction::Apply.id(), "filter-rules-apply");
    assert_eq!(
        FilterRulesAction::Rule {
            index: 1,
            control: RuleControl::SecondValue
        }
        .id(),
        "filter-rule-2-second-value"
    );
    assert_eq!(
        Combinator::from_index(Combinator::Any.index()),
        Combinator::Any
    );
}

// The panel on a table browse.

fn run(
    panel: &Entity<DataGridPanel>,
    window: &mut VisualTestContext,
    update: impl FnOnce(&mut DataGridPanel, &mut gpui::Window, &mut gpui::Context<DataGridPanel>),
) {
    window.update(|window, app| panel.update(app, |panel, cx| update(panel, window, cx)));
    window.run_until_parked();
}

/// Points rule `index` at `column` with `operator` and types `value`.
fn set_rule(
    panel: &mut DataGridPanel,
    index: usize,
    column: &str,
    operator: RuleOperator,
    value: &str,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<DataGridPanel>,
) {
    let column_index = panel
        .result
        .columns
        .iter()
        .position(|meta| meta.name == column)
        .expect("the table has the column");
    let row_id = panel.filter_rules.rows[index].id;
    panel.set_filter_rule_column(row_id, column_index, cx);

    let operator_index = panel.filter_rules.rows[index]
        .operators
        .iter()
        .position(|candidate| *candidate == operator)
        .expect("the column offers the operator");
    panel.set_filter_rule_operator(row_id, operator_index, cx);

    panel.filter_rules.rows[index]
        .value_input
        .clone()
        .update(cx, |input, cx| input.set_value(value, window, cx));
}

fn filter_text(panel: &Entity<DataGridPanel>, window: &mut VisualTestContext) -> String {
    window.update(|_, app| {
        panel
            .read(app)
            .filter_bar
            .filter_input
            .read(app)
            .value()
            .to_string()
    })
}

fn last_browse_filter(
    calls: &std::sync::Arc<std::sync::Mutex<crate::data_grid_panel::table_views::tests::Calls>>,
) -> Option<Option<String>> {
    calls
        .lock()
        .expect("calls lock")
        .browses
        .last()
        .map(|browse| browse.filter.clone())
}

#[gpui::test]
fn applying_rules_browses_the_table_with_their_where_filter(cx: &mut TestAppContext) {
    let (panel, calls, window) = open_table(TableViewFeatures::empty(), cx);

    let actions = window.update(|_, app| panel.read(app).toolbar_actions(app));
    assert!(actions.contains(&ToolbarAction::FilterRules(FilterRulesAction::Toggle)));

    run(&panel, window, |panel, window, cx| {
        panel.toggle_filter_rules(window, cx);
        assert_eq!(
            panel.filter_rules.rows.len(),
            1,
            "the panel opens with a rule"
        );
        assert_eq!(
            panel.filter_rules.rows[0].column.as_deref(),
            Some("site_id"),
            "on the first column"
        );
        assert_eq!(panel.filter_rules.rows[0].operator, RuleOperator::Equals);

        panel.add_filter_rule(Some(0), window, cx);
        set_rule(panel, 0, "country", RuleOperator::Equals, "FR", window, cx);
        set_rule(panel, 1, "path", RuleOperator::Contains, "home", window, cx);
    });

    let browses_before = calls.lock().expect("calls lock").browses.len();
    run(&panel, window, |panel, window, cx| {
        panel.apply_filter_rules(window, cx)
    });

    let expected = r#"("country" = 'FR') AND ("path" LIKE '%home%')"#;
    assert_eq!(
        calls.lock().expect("calls lock").browses.len(),
        browses_before + 1,
        "applying runs one browse"
    );
    assert_eq!(last_browse_filter(&calls), Some(Some(expected.to_string())));
    assert_eq!(
        filter_text(&panel, window),
        expected,
        "the WHERE input shows the generated filter"
    );

    let actions = window.update(|_, app| panel.read(app).toolbar_actions(app));
    assert!(
        actions.contains(&ToolbarAction::FilterRules(FilterRulesAction::Rule {
            index: 1,
            control: RuleControl::Remove
        }))
    );

    // With every rule off, applying clears the filter.
    run(&panel, window, |panel, window, cx| {
        for index in 0..2 {
            panel.run_filter_rules_action(
                FilterRulesAction::Rule {
                    index,
                    control: RuleControl::Enable,
                },
                window,
                cx,
            );
        }
        panel.run_filter_rules_action(FilterRulesAction::Apply, window, cx);
    });

    assert_eq!(last_browse_filter(&calls), Some(None));
    assert_eq!(filter_text(&panel, window), "");
}

#[gpui::test]
fn a_new_column_keeps_the_operator_only_when_its_type_offers_it(cx: &mut TestAppContext) {
    let (panel, _calls, window) = open_table(TableViewFeatures::empty(), cx);

    run(&panel, window, |panel, window, cx| {
        let mut columns = panel.result.columns.clone();
        columns[0].kind = ColumnKind::Integer;
        columns[1].type_name = "boolean".to_string();
        panel.result = QueryResult::table(columns, Vec::new(), None, Default::default());

        panel.toggle_filter_rules(window, cx);
        let row_id = panel.filter_rules.rows[0].id;
        assert_eq!(
            panel.filter_rules.rows[0].operators,
            operators_for(ColumnClass::Number)
        );

        set_rule(panel, 0, "country", RuleOperator::Contains, "x", window, cx);

        panel.set_filter_rule_column(row_id, 0, cx);
        assert_eq!(
            panel.filter_rules.rows[0].operator,
            RuleOperator::Equals,
            "a number column does not offer contains"
        );

        panel.set_filter_rule_operator(row_id, 1, cx);
        panel.set_filter_rule_column(row_id, 2, cx);
        assert_eq!(
            panel.filter_rules.rows[0].operator,
            RuleOperator::NotEquals,
            "a text column offers the same operator"
        );

        panel.set_filter_rule_column(row_id, 1, cx);
        assert_eq!(panel.filter_rules.rows[0].operator, RuleOperator::IsTrue);
    });
}
