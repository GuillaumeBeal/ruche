//! Drawing of the filter rules panel and its Rules button.

use super::{RuleRow, rule_element_id};
use crate::data_grid_panel::DataGridPanel;
use dbflux_components::controls::{Button, Checkbox, Input, InputState};
use dbflux_components::icons::AppIcon;
use dbflux_components::primitives::Text;
use dbflux_components::tokens::{Spacing, TableViewMetrics};
use gpui::prelude::*;
use gpui::*;

impl DataGridPanel {
    /// The Rules button of the filter row, pressed while the panel shows.
    pub(in crate::data_grid_panel) fn render_filter_rules_toggle(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let open = self.filter_rules.open;
        let tooltip = if open {
            dbflux_i18n::t!("document.data.grid.filter.rules.menu.hide")
        } else {
            dbflux_i18n::t!("document.data.grid.filter.rules.menu.show")
        };

        Button::new(
            "grid-filter-rules-toggle",
            dbflux_i18n::t!("document.data.grid.filter.rules.toggle"),
        )
        .ghost()
        .icon(AppIcon::Rows3)
        .toggled(open)
        .tooltip(tooltip)
        .tab_stop(false)
        .on_click(cx.listener(|this, _, window, cx| {
            this.toggle_filter_rules(window, cx);
        }))
    }

    /// The panel under the filter row: one line per rule, then the AND / OR
    /// choice and Apply filters.
    pub(in crate::data_grid_panel) fn render_filter_rules_panel(
        &self,
        theme: &gpui_component::theme::Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let removable = self.filter_rules.rows.len() > 1;
        let rows: Vec<AnyElement> = self
            .filter_rules
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| render_rule_row(index, row, removable, theme, cx))
            .collect();

        div()
            .id("grid-filter-rules")
            .debug_selector(|| "grid-filter-rules".to_string())
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap(TableViewMetrics::FILTER_RULES_ROW_GAP)
            .px(TableViewMetrics::FILTER_ROW_PADDING_X)
            .py(TableViewMetrics::FILTER_RULES_PADDING_Y)
            .border_b_1()
            .border_color(theme.border)
            .children(rows)
            .child(self.render_filter_rules_footer(cx))
            .into_any_element()
    }

    fn render_filter_rules_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(Spacing::SM)
            .when_some(
                self.filter_rules.combinator_dropdown.clone(),
                |footer, dropdown| {
                    footer.child(
                        div()
                            .w(TableViewMetrics::FILTER_RULES_COMBINATOR_WIDTH)
                            .flex_shrink_0()
                            .child(dropdown),
                    )
                },
            )
            .child(div().flex_1())
            .child(
                Button::new(
                    "grid-filter-rules-apply",
                    dbflux_i18n::t!("document.data.grid.filter.rules.apply"),
                )
                .primary()
                .icon(AppIcon::Check)
                .tab_stop(false)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.apply_filter_rules(window, cx);
                })),
            )
    }
}

/// One rule: enable box, column, operator, its values, remove and add.
fn render_rule_row(
    index: usize,
    row: &RuleRow,
    removable: bool,
    theme: &gpui_component::theme::Theme,
    cx: &mut Context<DataGridPanel>,
) -> AnyElement {
    let row_id = row.id;
    let value_count = row.operator.value_count();
    let rule_name = dbflux_i18n::t!(
        "document.data.grid.filter.rules.rule_name",
        number = index + 1
    );

    let value_input = |input: &Entity<InputState>, control: &str| {
        div().flex_1().min_w(px(0.0)).child(
            Input::new(input)
                .id(rule_element_id(row_id, control))
                .aria_label(dbflux_i18n::t!("document.data.grid.filter.rules.value"))
                .small()
                .w_full(),
        )
    };

    div()
        .flex()
        .items_center()
        .gap(Spacing::SM)
        .child(
            Checkbox::new(rule_element_id(row_id, "enabled"))
                .checked(row.enabled)
                .aria_label(rule_name)
                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                    this.set_filter_rule_enabled(row_id, *checked, cx);
                })),
        )
        .child(
            div()
                .w(TableViewMetrics::FILTER_RULE_COLUMN_WIDTH)
                .flex_shrink_0()
                .child(row.column_dropdown.clone()),
        )
        .child(
            div()
                .w(TableViewMetrics::FILTER_RULE_OPERATOR_WIDTH)
                .flex_shrink_0()
                .child(row.operator_dropdown.clone()),
        )
        .when(value_count >= 1, |line| {
            line.child(value_input(&row.value_input, "value"))
        })
        .when(value_count >= 2, |line| {
            line.child(
                Text::body_sm(dbflux_i18n::t!("document.data.grid.filter.rules.and"))
                    .color(theme.muted_foreground),
            )
            .child(value_input(&row.second_value_input, "second-value"))
        })
        .when(value_count == 0, |line| line.child(div().flex_1()))
        .child(
            Button::new(
                rule_element_id(row_id, "remove"),
                dbflux_i18n::t!("document.data.grid.filter.rules.remove"),
            )
            .ghost()
            .inline()
            .icon(AppIcon::Minus)
            .icon_only()
            .disabled(!removable)
            .tab_stop(false)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.remove_filter_rule(row_id, cx);
            })),
        )
        .child(
            Button::new(
                rule_element_id(row_id, "add"),
                dbflux_i18n::t!("document.data.grid.filter.rules.add_after"),
            )
            .ghost()
            .inline()
            .icon(AppIcon::Plus)
            .icon_only()
            .tab_stop(false)
            .on_click(cx.listener(move |this, _, window, cx| {
                let position = this
                    .filter_rules
                    .rows
                    .iter()
                    .position(|candidate| candidate.id == row_id);
                this.add_filter_rule(position, window, cx);
            })),
        )
        .into_any_element()
}
