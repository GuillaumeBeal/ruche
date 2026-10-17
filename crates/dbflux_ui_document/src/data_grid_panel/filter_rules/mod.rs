//! The filter rules panel of a table browse: a list of column / operator /
//! value rules under the filter row, Sequel Ace style, that writes the WHERE
//! filter for the user.
//!
//! Applying the rules renders them to SQL through the connection's dialect
//! and puts the result in the raw WHERE input, so the filter the rows come
//! from is always the one the input shows, and the count, history and clear
//! button keep working on it. The rules live as long as the tab.

pub(crate) mod model;
mod render;

use super::{DataGridPanel, EditState, TableReload};
use dbflux_components::controls::{
    Dropdown, DropdownItem, DropdownSelectionChanged, InputEvent, InputState,
};
use dbflux_components::icons::AppIcon;
use dbflux_core::{ColumnKind, ColumnMeta, QueryCapabilities, WhereOperator};
use dbflux_ui_base::user_error::{ErrorKind, UserFacingError, report_error};
use gpui::*;
use model::{
    ColumnClass, Combinator, FilterRule, RuleOperator, available_operators,
    rules_to_semantic_filter,
};

/// The rules panel of one grid.
#[derive(Default)]
pub(super) struct FilterRulesState {
    open: bool,
    rows: Vec<RuleRow>,
    combinator: Combinator,
    combinator_dropdown: Option<Entity<Dropdown>>,
    _combinator_subscription: Option<Subscription>,
    /// Source of the stable ids the rows' elements are named by.
    next_row_id: usize,
    /// Column names the column dropdowns list, to notice a reload that
    /// changed them.
    columns: Vec<String>,
    /// The rule input holding the keyboard: `(row id, second value)`.
    focused_input: Option<(usize, bool)>,
}

/// One rule and its controls.
struct RuleRow {
    id: usize,
    enabled: bool,
    column: Option<String>,
    operator: RuleOperator,
    /// The operators the operator dropdown lists, in its order.
    operators: Vec<RuleOperator>,
    column_dropdown: Entity<Dropdown>,
    operator_dropdown: Entity<Dropdown>,
    value_input: Entity<InputState>,
    second_value_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

/// A control of one rule, as the Toolbar submenu reaches it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuleControl {
    Enable,
    Column,
    Operator,
    Value,
    SecondValue,
    Remove,
    AddAfter,
}

/// An entry of the Toolbar submenu that works the rules panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FilterRulesAction {
    /// The Rules button: shows or hides the panel.
    Toggle,
    /// Adds a rule at the end.
    AddRule,
    /// Opens the AND / OR dropdown.
    Combinator,
    /// Apply filters.
    Apply,
    /// A control of the rule at this position.
    Rule { index: usize, control: RuleControl },
}

impl FilterRulesAction {
    /// Stable, locale-independent id of the menu entry.
    pub(crate) fn id(self) -> SharedString {
        match self {
            FilterRulesAction::Toggle => "filter-rules".into(),
            FilterRulesAction::AddRule => "filter-rules-add".into(),
            FilterRulesAction::Combinator => "filter-rules-combinator".into(),
            FilterRulesAction::Apply => "filter-rules-apply".into(),
            FilterRulesAction::Rule { index, control } => {
                let control = match control {
                    RuleControl::Enable => "enable",
                    RuleControl::Column => "column",
                    RuleControl::Operator => "operator",
                    RuleControl::Value => "value",
                    RuleControl::SecondValue => "second-value",
                    RuleControl::Remove => "remove",
                    RuleControl::AddAfter => "add",
                };
                format!("filter-rule-{}-{control}", index + 1).into()
            }
        }
    }

    pub(crate) fn icon(self) -> AppIcon {
        match self {
            FilterRulesAction::Toggle => AppIcon::Rows3,
            FilterRulesAction::AddRule
            | FilterRulesAction::Rule {
                control: RuleControl::AddAfter,
                ..
            } => AppIcon::Plus,
            FilterRulesAction::Combinator => AppIcon::ListFilter,
            FilterRulesAction::Apply => AppIcon::Check,
            FilterRulesAction::Rule {
                control: RuleControl::Remove,
                ..
            } => AppIcon::Minus,
            FilterRulesAction::Rule {
                control: RuleControl::Enable,
                ..
            } => AppIcon::CircleCheck,
            FilterRulesAction::Rule { .. } => AppIcon::ListFilter,
        }
    }
}

/// The element id of a rule control, from its row's stable id.
fn rule_element_id(row_id: usize, control: &str) -> SharedString {
    format!("grid-filter-rule-{row_id}-{control}").into()
}

impl DataGridPanel {
    /// Whether the filter row offers the Rules button: a table browse whose
    /// WHERE input shows, in its Data view, outside similar rows.
    pub(super) fn filter_rules_available(&self) -> bool {
        self.source.is_table()
            && self.filter_input_visible()
            && self.neighbor_bar_search().is_none()
            && self.table_tab() == super::table_views::TableTab::Data
    }

    /// Whether the rules panel shows under the filter row.
    pub(super) fn filter_rules_open(&self) -> bool {
        self.filter_rules.open && self.filter_rules_available()
    }

    /// Shows or hides the rules panel. Opening it the first time starts
    /// with one rule on the first column.
    pub(super) fn toggle_filter_rules(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_rules.open = !self.filter_rules.open;

        if self.filter_rules.open && self.filter_rules.rows.is_empty() {
            self.add_filter_rule(None, window, cx);
        }

        if !self.filter_rules.open {
            self.leave_filter_rule_input();
        }

        cx.notify();
    }

    /// The Toolbar submenu entries of the rules panel: the Rules button, and
    /// while the panel is open every control it draws.
    pub(super) fn filter_rules_toolbar_actions(&self) -> Vec<FilterRulesAction> {
        if !self.filter_rules_available() {
            return Vec::new();
        }

        let mut actions = vec![FilterRulesAction::Toggle];

        if !self.filter_rules.open {
            return actions;
        }

        let removable = self.filter_rules.rows.len() > 1;

        for (index, row) in self.filter_rules.rows.iter().enumerate() {
            let mut controls = vec![
                RuleControl::Enable,
                RuleControl::Column,
                RuleControl::Operator,
            ];

            if row.operator.value_count() >= 1 {
                controls.push(RuleControl::Value);
            }
            if row.operator.value_count() >= 2 {
                controls.push(RuleControl::SecondValue);
            }
            if removable {
                controls.push(RuleControl::Remove);
            }
            controls.push(RuleControl::AddAfter);

            actions.extend(
                controls
                    .into_iter()
                    .map(|control| FilterRulesAction::Rule { index, control }),
            );
        }

        actions.extend([
            FilterRulesAction::AddRule,
            FilterRulesAction::Combinator,
            FilterRulesAction::Apply,
        ]);
        actions
    }

    /// The label of a rules entry in the Toolbar submenu.
    pub(super) fn filter_rules_action_label(&self, action: FilterRulesAction) -> String {
        let rule_label = |key: &str, index: usize| {
            dbflux_i18n::t!(
                "document.data.grid.filter.rules.menu.rule",
                number = index + 1,
                control =
                    dbflux_i18n::translate(&format!("document.data.grid.filter.rules.menu.{key}"))
            )
        };

        match action {
            FilterRulesAction::Toggle if self.filter_rules.open => {
                dbflux_i18n::t!("document.data.grid.filter.rules.menu.hide")
            }
            FilterRulesAction::Toggle => {
                dbflux_i18n::t!("document.data.grid.filter.rules.menu.show")
            }
            FilterRulesAction::AddRule => {
                dbflux_i18n::t!("document.data.grid.filter.rules.menu.add_rule")
            }
            FilterRulesAction::Combinator => {
                dbflux_i18n::t!("document.data.grid.filter.rules.menu.combinator")
            }
            FilterRulesAction::Apply => dbflux_i18n::t!("document.data.grid.filter.rules.apply"),
            FilterRulesAction::Rule { index, control } => {
                let enabled = self
                    .filter_rules
                    .rows
                    .get(index)
                    .is_some_and(|row| row.enabled);
                let key = match control {
                    RuleControl::Enable if enabled => "disable",
                    RuleControl::Enable => "enable",
                    RuleControl::Column => "column",
                    RuleControl::Operator => "operator",
                    RuleControl::Value => "value",
                    RuleControl::SecondValue => "second_value",
                    RuleControl::Remove => "remove",
                    RuleControl::AddAfter => "add_after",
                };
                rule_label(key, index)
            }
        }
    }

    /// Runs what the panel control behind `action` runs; a dropdown or a
    /// value input takes the keyboard.
    pub(super) fn run_filter_rules_action(
        &mut self,
        action: FilterRulesAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            FilterRulesAction::Toggle => self.toggle_filter_rules(window, cx),
            FilterRulesAction::AddRule => {
                let last = self.filter_rules.rows.len().checked_sub(1);
                self.add_filter_rule(last, window, cx);
            }
            FilterRulesAction::Combinator => {
                if let Some(dropdown) = self.filter_rules.combinator_dropdown.clone() {
                    dropdown.update(cx, |dropdown, cx| dropdown.focus_and_open(window, cx));
                }
            }
            FilterRulesAction::Apply => self.apply_filter_rules(window, cx),
            FilterRulesAction::Rule { index, control } => {
                self.run_rule_control(index, control, window, cx)
            }
        }
    }

    fn run_rule_control(
        &mut self,
        index: usize,
        control: RuleControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.filter_rules.rows.get(index) else {
            return;
        };
        let row_id = row.id;

        match control {
            RuleControl::Enable => {
                let enabled = !row.enabled;
                self.set_filter_rule_enabled(row_id, enabled, cx);
            }
            RuleControl::Column => {
                let dropdown = row.column_dropdown.clone();
                dropdown.update(cx, |dropdown, cx| dropdown.focus_and_open(window, cx));
            }
            RuleControl::Operator => {
                let dropdown = row.operator_dropdown.clone();
                dropdown.update(cx, |dropdown, cx| dropdown.focus_and_open(window, cx));
            }
            RuleControl::Value => {
                let input = row.value_input.clone();
                input.update(cx, |input, cx| input.focus(window, cx));
            }
            RuleControl::SecondValue => {
                let input = row.second_value_input.clone();
                input.update(cx, |input, cx| input.focus(window, cx));
            }
            RuleControl::Remove => self.remove_filter_rule(row_id, cx),
            RuleControl::AddAfter => self.add_filter_rule(Some(index), window, cx),
        }
    }

    /// The columns a rule can test, from the rows on screen.
    fn filter_rule_columns(&self) -> &[ColumnMeta] {
        &self.result.columns
    }

    fn filter_rule_column(&self, name: Option<&str>) -> Option<&ColumnMeta> {
        let name = name?;
        self.filter_rule_columns()
            .iter()
            .find(|column| column.name == name)
    }

    /// The semantic operators the connection declares it renders. Without a
    /// connection, or a driver that declares none, the core defaults stand.
    fn filter_rule_where_operators(&self, cx: &App) -> Vec<WhereOperator> {
        let super::DataSource::Table { profile_id, .. } = &self.source else {
            return QueryCapabilities::default().where_operators;
        };

        self.app_state
            .read(cx)
            .connections()
            .get(profile_id)
            .and_then(|connected| connected.connection.metadata().query.as_ref())
            .map(|query| query.where_operators.clone())
            .unwrap_or_else(|| QueryCapabilities::default().where_operators)
    }

    /// The operators offered for the column named `column`.
    fn filter_rule_operators(&self, column: Option<&str>, cx: &App) -> Vec<RuleOperator> {
        let class = self
            .filter_rule_column(column)
            .map(ColumnClass::of)
            .unwrap_or(ColumnClass::Text);
        let where_operators = self.filter_rule_where_operators(cx);

        available_operators(class, &where_operators)
    }

    fn column_dropdown_items(&self) -> Vec<DropdownItem> {
        self.filter_rule_columns()
            .iter()
            .map(|column| DropdownItem::new(column.name.clone()))
            .collect()
    }

    fn column_index(&self, column: Option<&str>) -> Option<usize> {
        let column = column?;
        self.filter_rule_columns()
            .iter()
            .position(|meta| meta.name == column)
    }

    /// Adds a rule after the rule at `after`, or first when `None`, on the
    /// first column with its type's first operator.
    pub(super) fn add_filter_rule(
        &mut self,
        after: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row_id = self.filter_rules.next_row_id;
        self.filter_rules.next_row_id += 1;

        let column = self
            .filter_rule_columns()
            .first()
            .map(|column| column.name.clone());
        let operators = self.filter_rule_operators(column.as_deref(), cx);
        let operator = operators.first().copied().unwrap_or(RuleOperator::Equals);

        let column_items = self.column_dropdown_items();
        let column_selected = self.column_index(column.as_deref());

        let column_dropdown = cx.new(|_| {
            Dropdown::new(rule_element_id(row_id, "column"))
                .items(column_items)
                .selected_index(column_selected)
                .aria_label(dbflux_i18n::t!("document.data.grid.filter.rules.column"))
        });
        let operator_dropdown = cx.new(|_| {
            Dropdown::new(rule_element_id(row_id, "operator"))
                .items(operator_items(&operators))
                .selected_index(Some(0))
                .aria_label(dbflux_i18n::t!("document.data.grid.filter.rules.operator"))
        });
        let value_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(dbflux_i18n::t!("document.data.grid.filter.rules.value"))
        });
        let second_value_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(dbflux_i18n::t!("document.data.grid.filter.rules.value"))
        });

        let subscriptions = vec![
            cx.subscribe_in(
                &column_dropdown,
                window,
                move |this, _, event: &DropdownSelectionChanged, _, cx| {
                    this.set_filter_rule_column(row_id, event.index, cx);
                },
            ),
            cx.subscribe_in(
                &operator_dropdown,
                window,
                move |this, _, event: &DropdownSelectionChanged, _, cx| {
                    this.set_filter_rule_operator(row_id, event.index, cx);
                },
            ),
            self.subscribe_rule_input(&value_input, row_id, false, window, cx),
            self.subscribe_rule_input(&second_value_input, row_id, true, window, cx),
        ];

        let row = RuleRow {
            id: row_id,
            enabled: true,
            column,
            operator,
            operators,
            column_dropdown,
            operator_dropdown,
            value_input,
            second_value_input,
            _subscriptions: subscriptions,
        };

        let position = after
            .map(|index| (index + 1).min(self.filter_rules.rows.len()))
            .unwrap_or(0);
        self.filter_rules.rows.insert(position, row);
        self.ensure_filter_rules_combinator(window, cx);
        cx.notify();
    }

    /// Enter in a value applies the rules; focus moves the grid into text
    /// entry, so its letter keys type instead of navigating.
    fn subscribe_rule_input(
        &self,
        input: &Entity<InputState>,
        row_id: usize,
        second: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(
            input,
            window,
            move |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter {
                    secondary: false, ..
                } => {
                    this.apply_filter_rules(window, cx);
                }
                InputEvent::Focus => {
                    this.filter_rules.focused_input = Some((row_id, second));
                    this.focus.edit_state = EditState::Editing;
                    cx.notify();
                }
                InputEvent::Blur if this.filter_rules.focused_input == Some((row_id, second)) => {
                    this.leave_filter_rule_input();
                    cx.notify();
                }
                _ => {}
            },
        )
    }

    /// Hands the keys back to the grid once no rule input holds them.
    fn leave_filter_rule_input(&mut self) {
        if self.filter_rules.focused_input.take().is_some() {
            self.focus.edit_state = EditState::Navigating;
        }
    }

    fn ensure_filter_rules_combinator(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.filter_rules.combinator_dropdown.is_some() {
            return;
        }

        let selected = self.filter_rules.combinator.index();
        let dropdown = cx.new(|_| {
            Dropdown::new("grid-filter-rules-combinator")
                .items(
                    Combinator::ALL
                        .iter()
                        .map(|combinator| DropdownItem::new(combinator.label()))
                        .collect(),
                )
                .selected_index(Some(selected))
                .aria_label(dbflux_i18n::t!(
                    "document.data.grid.filter.rules.menu.combinator"
                ))
        });

        let subscription = cx.subscribe_in(
            &dropdown,
            window,
            |this, _, event: &DropdownSelectionChanged, _, cx| {
                this.filter_rules.combinator = Combinator::from_index(event.index);
                cx.notify();
            },
        );

        self.filter_rules.combinator_dropdown = Some(dropdown);
        self.filter_rules._combinator_subscription = Some(subscription);
    }

    fn remove_filter_rule(&mut self, row_id: usize, cx: &mut Context<Self>) {
        if self.filter_rules.rows.len() <= 1 {
            return;
        }

        self.filter_rules.rows.retain(|row| row.id != row_id);

        if self
            .filter_rules
            .focused_input
            .is_some_and(|(focused, _)| focused == row_id)
        {
            self.leave_filter_rule_input();
        }

        cx.notify();
    }

    fn set_filter_rule_enabled(&mut self, row_id: usize, enabled: bool, cx: &mut Context<Self>) {
        if let Some(row) = self
            .filter_rules
            .rows
            .iter_mut()
            .find(|row| row.id == row_id)
        {
            row.enabled = enabled;
            cx.notify();
        }
    }

    /// Points a rule at another column. The operator stays when the new
    /// column's type offers it, and falls back to the type's first otherwise.
    fn set_filter_rule_column(&mut self, row_id: usize, index: usize, cx: &mut Context<Self>) {
        let Some(column) = self
            .filter_rule_columns()
            .get(index)
            .map(|column| column.name.clone())
        else {
            return;
        };

        let operators = self.filter_rule_operators(Some(&column), cx);
        let Some(row) = self
            .filter_rules
            .rows
            .iter_mut()
            .find(|row| row.id == row_id)
        else {
            return;
        };

        row.column = Some(column);
        reset_operators(row, operators, cx);
        cx.notify();
    }

    fn set_filter_rule_operator(&mut self, row_id: usize, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self
            .filter_rules
            .rows
            .iter_mut()
            .find(|row| row.id == row_id)
        else {
            return;
        };

        if let Some(operator) = row.operators.get(index).copied() {
            row.operator = operator;
            cx.notify();
        }
    }

    /// Follows a reload that changed the table's columns: the dropdowns list
    /// the new columns, and a rule whose column is gone moves to the first.
    pub(super) fn sync_filter_rule_columns(&mut self, cx: &mut Context<Self>) {
        let names: Vec<String> = self
            .filter_rule_columns()
            .iter()
            .map(|column| column.name.clone())
            .collect();

        if names == self.filter_rules.columns {
            return;
        }

        let items = self.column_dropdown_items();
        let first = names.first().cloned();
        let row_ids: Vec<usize> = self.filter_rules.rows.iter().map(|row| row.id).collect();

        for row_id in row_ids {
            let current = self
                .filter_rules
                .rows
                .iter()
                .find(|row| row.id == row_id)
                .and_then(|row| row.column.clone());
            let kept = current.filter(|column| names.contains(column));
            let column = kept.or_else(|| first.clone());
            let operators = self.filter_rule_operators(column.as_deref(), cx);
            let selected = self.column_index(column.as_deref());

            let Some(row) = self
                .filter_rules
                .rows
                .iter_mut()
                .find(|row| row.id == row_id)
            else {
                continue;
            };

            row.column = column;
            row.column_dropdown.update(cx, |dropdown, cx| {
                dropdown.set_items(items.clone(), cx);
                dropdown.set_selected_index(selected, cx);
            });
            reset_operators(row, operators, cx);
        }

        self.filter_rules.columns = names;
    }

    /// The rules as the model reads them, with the values typed in.
    fn filter_rules_snapshot(&self, cx: &App) -> Vec<FilterRule> {
        self.filter_rules
            .rows
            .iter()
            .map(|row| FilterRule {
                enabled: row.enabled,
                column: row.column.clone(),
                kind: self
                    .filter_rule_column(row.column.as_deref())
                    .map(|column| column.kind)
                    .unwrap_or(ColumnKind::Unknown),
                operator: row.operator,
                value: row.value_input.read(cx).value().to_string(),
                second_value: row.second_value_input.read(cx).value().to_string(),
            })
            .collect()
    }

    /// Writes the enabled, complete rules into the WHERE input as SQL in the
    /// connection's dialect and reloads the rows under it. With no such rule
    /// the filter is cleared.
    pub(super) fn apply_filter_rules(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rules = self.filter_rules_snapshot(cx);

        let sql = match rules_to_semantic_filter(&rules, self.filter_rules.combinator) {
            None => String::new(),
            Some(filter) => match self.render_filter_rules_sql(&filter, cx) {
                Ok(sql) => sql,
                Err(error) => {
                    report_error(
                        UserFacingError::new(
                            ErrorKind::User,
                            dbflux_i18n::t!("document.data.grid.filter.rules.apply_failed"),
                        )
                        .with_cause(error),
                        cx,
                    );
                    return;
                }
            },
        };

        if self.reload_blocked_by_pending_edits(cx) {
            return;
        }

        // A new filter selects a different row set.
        self.grid_table.reload = TableReload::ResetRows;
        self.replace_filter_and_reload(&sql, window, cx);
    }

    fn render_filter_rules_sql(
        &self,
        filter: &dbflux_core::SemanticFilter,
        cx: &App,
    ) -> Result<String, String> {
        let super::DataSource::Table { profile_id, .. } = &self.source else {
            return Err(dbflux_i18n::t!(
                "document.data.grid.error.connection_not_available"
            ));
        };

        let app_state = self.app_state.read(cx);
        let Some(connected) = app_state.connections().get(profile_id) else {
            return Err(dbflux_i18n::t!(
                "document.data.grid.error.connection_not_available"
            ));
        };

        dbflux_core::render_semantic_filter_sql(filter, connected.connection.dialect())
            .map_err(|error| error.to_string())
    }
}

fn operator_items(operators: &[RuleOperator]) -> Vec<DropdownItem> {
    operators
        .iter()
        .map(|operator| DropdownItem::new(operator.label()))
        .collect()
}

/// Lists `operators` in the row's operator dropdown, keeping the current
/// operator when it is among them.
fn reset_operators(row: &mut RuleRow, operators: Vec<RuleOperator>, cx: &mut App) {
    let operator = if operators.contains(&row.operator) {
        row.operator
    } else {
        operators.first().copied().unwrap_or(row.operator)
    };
    let selected = operators
        .iter()
        .position(|candidate| *candidate == operator);
    let items = operator_items(&operators);

    row.operator = operator;
    row.operators = operators;
    row.operator_dropdown.update(cx, |dropdown, cx| {
        dropdown.set_items(items, cx);
        dropdown.set_selected_index(selected, cx);
    });
}

#[cfg(test)]
mod tests;
