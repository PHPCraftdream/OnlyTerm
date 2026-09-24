use crate::{format_err, Error, Result};
use cassowary::strength::{REQUIRED, STRONG, WEAK};
use cassowary::WeightedRelation::*;
use cassowary::{AddConstraintError, Expression, Solver, SuggestValueError, Variable};
use std::collections::HashMap;

use crate::widgets::{Rect, WidgetId};

use super::{ChildOrientation, Constraints, DimensionSpec, HorizontalAlignment, VerticalAlignment};

/// Expands to an Expression holding the value of the variable,
/// or if there is no variable, a constant with the specified
/// value.
/// Equivalent to Option::unwrap_or().
fn unwrap_variable_or(var: Option<Variable>, value: f64) -> Expression {
    // The `v + 0.0` portion "converts" the variable to an Expression.
    var.map(|v| v + 0.0)
        .unwrap_or_else(|| Expression::from_constant(value))
}

/// Holds state used to compute the layout of a tree of widgets
pub struct LayoutState {
    solver: Solver,
    screen_width: Variable,
    screen_height: Variable,
    widget_states: HashMap<WidgetId, WidgetState>,
}

/// Each `WidgetId` has a `WidgetState` associated with it.
/// This allows us to look up the `Variable` for each of the
/// layout features of a widget by `WidgetId` after the solver
/// has computed the layout solution.
#[derive(Clone)]
struct WidgetState {
    left: Variable,
    top: Variable,
    width: Variable,
    height: Variable,
    constraints: Constraints,
    children: Vec<WidgetId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaidOutWidget {
    pub widget: WidgetId,
    pub rect: Rect,
}

fn suggesterr(e: SuggestValueError) -> Error {
    match e {
        SuggestValueError::UnknownEditVariable => format_err!("Unknown edit variable"),
        SuggestValueError::InternalSolverError(e) => format_err!("Internal solver error: {}", e),
    }
}

fn adderr(e: AddConstraintError) -> Error {
    format_err!("{:?}", e)
}

impl Default for LayoutState {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutState {
    /// Create a new `LayoutState`
    pub fn new() -> Self {
        let mut solver = Solver::new();
        let screen_width = Variable::new();
        let screen_height = Variable::new();
        solver
            .add_edit_variable(screen_width, STRONG)
            .expect("failed to add screen_width to solver");
        solver
            .add_edit_variable(screen_height, STRONG)
            .expect("failed to add screen_height to solver");
        Self {
            solver,
            screen_width,
            screen_height,
            widget_states: HashMap::new(),
        }
    }

    /// Creates a WidgetState entry for a widget.
    pub fn add_widget(
        &mut self,
        widget: WidgetId,
        constraints: &Constraints,
        children: &[WidgetId],
    ) {
        let state = WidgetState {
            left: Variable::new(),
            top: Variable::new(),
            width: Variable::new(),
            height: Variable::new(),
            constraints: *constraints,
            children: children.to_vec(),
        };
        self.widget_states.insert(widget, state);
    }

    /// Assign the screen dimensions, compute constraints, solve
    /// the layout and then apply the size and positioning information
    /// to the widgets in the widget tree.
    pub fn compute_constraints(
        &mut self,
        screen_width: usize,
        screen_height: usize,
        root_widget: WidgetId,
    ) -> Result<Vec<LaidOutWidget>> {
        self.solver
            .suggest_value(self.screen_width, screen_width as f64)
            .map_err(suggesterr)?;
        self.solver
            .suggest_value(self.screen_height, screen_height as f64)
            .map_err(suggesterr)?;

        let width = self.screen_width;
        let height = self.screen_height;
        self.update_widget_constraint(root_widget, width, height, None, None)?;

        // The updates are in an unspecified order, and the coordinates are in
        // the screen absolute coordinate space, rather than the parent-relative
        // coordinates that we desire.  So we need to either to accumulate the
        // deltas and then sort them such that we walk the widget tree to apply
        // them, or just walk the tree and apply them all anyway.  The latter
        // feels easier and probably has a low enough cardinality that it won't
        // feel too expensive.
        self.solver.fetch_changes();

        let mut results = Vec::new();
        self.compute_widget_state(root_widget, 0, 0, &mut results)?;

        Ok(results)
    }

    fn compute_widget_state(
        &self,
        widget: WidgetId,
        parent_left: usize,
        parent_top: usize,
        results: &mut Vec<LaidOutWidget>,
    ) -> Result<()> {
        let state = self
            .widget_states
            .get(&widget)
            .ok_or_else(|| format_err!("widget has no solver state"))?;
        let width = self.solver.get_value(state.width) as usize;
        let height = self.solver.get_value(state.height) as usize;
        let left = self.solver.get_value(state.left) as usize;
        let top = self.solver.get_value(state.top) as usize;

        results.push(LaidOutWidget {
            widget,
            rect: Rect {
                x: left - parent_left,
                y: top - parent_top,
                width,
                height,
            },
        });

        for child in &state.children {
            self.compute_widget_state(*child, left, top, results)?;
        }

        Ok(())
    }

    fn update_widget_constraint(
        &mut self,
        widget: WidgetId,
        parent_width: Variable,
        parent_height: Variable,
        parent_left: Option<Variable>,
        parent_top: Option<Variable>,
    ) -> Result<WidgetState> {
        let state = self
            .widget_states
            .get(&widget)
            .ok_or_else(|| format_err!("widget has no solver state"))?
            .clone();

        let is_root_widget = parent_left.is_none();

        let parent_left = unwrap_variable_or(parent_left, 0.0);
        let parent_top = unwrap_variable_or(parent_top, 0.0);

        // First, we should fit inside the parent container
        self.solver
            .add_constraint(
                (state.left + state.width) | LE(REQUIRED) | (parent_left.clone() + parent_width),
            )
            .map_err(adderr)?;
        self.solver
            .add_constraint(state.left | GE(REQUIRED) | parent_left)
            .map_err(adderr)?;

        self.solver
            .add_constraint(
                (state.top + state.height) | LE(REQUIRED) | (parent_top.clone() + parent_height),
            )
            .map_err(adderr)?;
        self.solver
            .add_constraint(state.top | GE(REQUIRED) | parent_top)
            .map_err(adderr)?;

        if is_root_widget {
            // We handle alignment on the root widget specially here;
            // for non-root widgets, we handle it when assessing children
            match state.constraints.halign {
                HorizontalAlignment::Left => self
                    .solver
                    .add_constraint(state.left | EQ(STRONG) | 0.0)
                    .map_err(adderr)?,
                HorizontalAlignment::Right => self
                    .solver
                    .add_constraint(state.left | EQ(STRONG) | (parent_width - state.width))
                    .map_err(adderr)?,
                HorizontalAlignment::Center => self
                    .solver
                    .add_constraint(state.left | EQ(STRONG) | ((parent_width - state.width) / 2.0))
                    .map_err(adderr)?,
            }

            match state.constraints.valign {
                VerticalAlignment::Top => self
                    .solver
                    .add_constraint(state.top | EQ(STRONG) | 0.0)
                    .map_err(adderr)?,
                VerticalAlignment::Bottom => self
                    .solver
                    .add_constraint(state.top | EQ(STRONG) | (parent_height - state.height))
                    .map_err(adderr)?,
                VerticalAlignment::Middle => self
                    .solver
                    .add_constraint(state.top | EQ(STRONG) | ((parent_height - state.height) / 2.0))
                    .map_err(adderr)?,
            }
        }

        match state.constraints.width.spec {
            DimensionSpec::Fixed(width) => {
                self.solver
                    .add_constraint(state.width | EQ(STRONG) | f64::from(width))
                    .map_err(adderr)?;
            }
            DimensionSpec::Percentage(pct) => {
                self.solver
                    .add_constraint(
                        state.width | EQ(STRONG) | (f64::from(pct) * parent_width / 100.0),
                    )
                    .map_err(adderr)?;
            }
        }
        self.solver
            .add_constraint(
                state.width
                    | GE(STRONG)
                    | f64::from(state.constraints.width.minimum.unwrap_or(1).max(1)),
            )
            .map_err(adderr)?;
        if let Some(max_width) = state.constraints.width.maximum {
            self.solver
                .add_constraint(state.width | LE(STRONG) | f64::from(max_width))
                .map_err(adderr)?;
        }

        match state.constraints.height.spec {
            DimensionSpec::Fixed(height) => {
                self.solver
                    .add_constraint(state.height | EQ(STRONG) | f64::from(height))
                    .map_err(adderr)?;
            }
            DimensionSpec::Percentage(pct) => {
                self.solver
                    .add_constraint(
                        state.height | EQ(STRONG) | (f64::from(pct) * parent_height / 100.0),
                    )
                    .map_err(adderr)?;
            }
        }
        self.solver
            .add_constraint(
                state.height
                    | GE(STRONG)
                    | f64::from(state.constraints.height.minimum.unwrap_or(1).max(1)),
            )
            .map_err(adderr)?;
        if let Some(max_height) = state.constraints.height.maximum {
            self.solver
                .add_constraint(state.height | LE(STRONG) | f64::from(max_height))
                .map_err(adderr)?;
        }

        let has_children = !state.children.is_empty();
        if has_children {
            let mut left_edge: Expression = state.left + 0.0;
            let mut top_edge: Expression = state.top + 0.0;
            let mut width_constraint = Expression::from_constant(0.0);
            let mut height_constraint = Expression::from_constant(0.0);

            for child in &state.children {
                let child_state = self.update_widget_constraint(
                    *child,
                    state.width,
                    state.height,
                    Some(state.left),
                    Some(state.top),
                )?;

                match child_state.constraints.halign {
                    HorizontalAlignment::Left => self
                        .solver
                        .add_constraint(child_state.left | EQ(STRONG) | left_edge.clone())
                        .map_err(adderr)?,
                    HorizontalAlignment::Right => self
                        .solver
                        .add_constraint(
                            (child_state.left + child_state.width)
                                | EQ(STRONG)
                                | (state.left + state.width),
                        )
                        .map_err(adderr)?,
                    HorizontalAlignment::Center => self
                        .solver
                        .add_constraint(
                            child_state.left
                                | EQ(STRONG)
                                | (state.left + (state.width - child_state.width) / 2.0),
                        )
                        .map_err(adderr)?,
                }

                match child_state.constraints.valign {
                    VerticalAlignment::Top => self
                        .solver
                        .add_constraint(child_state.top | EQ(STRONG) | top_edge.clone())
                        .map_err(adderr)?,
                    VerticalAlignment::Bottom => self
                        .solver
                        .add_constraint(
                            (child_state.top + child_state.height)
                                | EQ(STRONG)
                                | (state.top + state.height),
                        )
                        .map_err(adderr)?,
                    VerticalAlignment::Middle => self
                        .solver
                        .add_constraint(
                            child_state.top
                                | EQ(STRONG)
                                | (state.top + (state.height - child_state.height) / 2.0),
                        )
                        .map_err(adderr)?,
                }

                match state.constraints.child_orientation {
                    ChildOrientation::Horizontal => {
                        left_edge = child_state.left + child_state.width;
                        width_constraint = width_constraint + child_state.width;
                    }
                    ChildOrientation::Vertical => {
                        top_edge = child_state.top + child_state.height;
                        height_constraint = height_constraint + child_state.height;
                    }
                }
            }

            // This constraint encourages the contents to fill out to the width
            // of the container, rather than clumping left
            self.solver
                .add_constraint(left_edge | EQ(STRONG) | (state.left + state.width))
                .map_err(adderr)?;

            self.solver
                .add_constraint(state.width | GE(WEAK) | width_constraint)
                .map_err(adderr)?;

            // This constraint encourages the contents to fill out to the height
            // of the container, rather than clumping top
            self.solver
                .add_constraint(top_edge | EQ(STRONG) | (state.top + state.height))
                .map_err(adderr)?;

            self.solver
                .add_constraint(state.height | GE(WEAK) | height_constraint)
                .map_err(adderr)?;
        }

        Ok(state)
    }
}
