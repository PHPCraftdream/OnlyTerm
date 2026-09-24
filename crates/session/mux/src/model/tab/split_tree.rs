use crate::pane::*;
use crate::WindowId;
use onlyterm_term::TerminalSize;
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::sync::Arc;

use super::codec::{PaneEntry, PaneNode};
use super::{BinaryCursor, BinaryTree, TabId};

pub type Tree = BinaryTree<Arc<dyn Pane>, SplitDirectionAndSize>;
pub type Cursor = BinaryCursor<Arc<dyn Pane>, SplitDirectionAndSize>;

#[derive(Clone)]
pub struct PositionedPane {
    /// The topological pane index that can be used to reference this pane
    pub index: usize,
    /// true if this is the active pane at the time the position was computed
    pub is_active: bool,
    /// true if this pane is zoomed
    pub is_zoomed: bool,
    /// The offset from the top left corner of the containing tab to the top
    /// left corner of this pane, in cells.
    pub left: usize,
    /// The offset from the top left corner of the containing tab to the top
    /// left corner of this pane, in cells.
    pub top: usize,
    /// The width of this pane in cells
    pub width: usize,
    pub pixel_width: usize,
    /// The height of this pane in cells
    pub height: usize,
    pub pixel_height: usize,
    /// The pane instance
    pub pane: Arc<dyn Pane>,
}

impl std::fmt::Debug for PositionedPane {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        fmt.debug_struct("PositionedPane")
            .field("index", &self.index)
            .field("is_active", &self.is_active)
            .field("left", &self.left)
            .field("top", &self.top)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("pane_id", &self.pane.pane_id())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// The size is of the (first, second) child of the split
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub struct SplitDirectionAndSize {
    pub direction: SplitDirection,
    pub first: TerminalSize,
    pub second: TerminalSize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum SplitSize {
    Cells(usize),
    Percent(u8),
}

impl Default for SplitSize {
    fn default() -> Self {
        Self::Percent(50)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub struct SplitRequest {
    pub direction: SplitDirection,
    /// Whether the newly created item will be in the second part
    /// of the split (right/bottom)
    pub target_is_second: bool,
    /// Split across the top of the tab rather than the active pane
    pub top_level: bool,
    /// The size of the new item
    pub size: SplitSize,
}

impl Default for SplitRequest {
    fn default() -> Self {
        Self {
            direction: SplitDirection::Horizontal,
            target_is_second: true,
            top_level: false,
            size: SplitSize::default(),
        }
    }
}

impl SplitDirectionAndSize {
    pub(super) fn top_of_second(&self) -> usize {
        match self.direction {
            SplitDirection::Horizontal => 0,
            SplitDirection::Vertical => self.first.rows + 1,
        }
    }

    pub(super) fn left_of_second(&self) -> usize {
        match self.direction {
            SplitDirection::Horizontal => self.first.cols + 1,
            SplitDirection::Vertical => 0,
        }
    }

    pub fn width(&self) -> usize {
        if self.direction == SplitDirection::Horizontal {
            self.first.cols + self.second.cols + 1
        } else {
            self.first.cols
        }
    }

    pub fn height(&self) -> usize {
        if self.direction == SplitDirection::Vertical {
            self.first.rows + self.second.rows + 1
        } else {
            self.first.rows
        }
    }

    pub fn size(&self) -> TerminalSize {
        let cell_width = self.first.pixel_width / self.first.cols;
        let cell_height = self.first.pixel_height / self.first.rows;

        let rows = self.height();
        let cols = self.width();

        TerminalSize {
            rows,
            cols,
            pixel_height: cell_height * rows,
            pixel_width: cell_width * cols,
            dpi: self.first.dpi,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct PositionedSplit {
    /// The topological node index that can be used to reference this split
    pub index: usize,
    pub direction: SplitDirection,
    /// The offset from the top left corner of the containing tab to the top
    /// left corner of this split, in cells.
    pub left: usize,
    /// The offset from the top left corner of the containing tab to the top
    /// left corner of this split, in cells.
    pub top: usize,
    /// For Horizontal splits, how tall the split should be, for Vertical
    /// splits how wide it should be
    pub size: usize,
}

pub(super) fn is_pane(pane: &Arc<dyn Pane>, other: &Option<&Arc<dyn Pane>>) -> bool {
    if let Some(other) = other {
        other.pane_id() == pane.pane_id()
    } else {
        false
    }
}

#[allow(clippy::too_many_arguments)] // private recursive tree-builder; builder-struct refactor adds churn for no behavioral gain
pub(super) fn pane_tree(
    tree: &Tree,
    tab_id: TabId,
    window_id: WindowId,
    active: Option<&Arc<dyn Pane>>,
    zoomed: Option<&Arc<dyn Pane>>,
    workspace: &str,
    left_col: usize,
    top_row: usize,
) -> PaneNode {
    match tree {
        Tree::Empty => PaneNode::Empty,
        Tree::Node { left, right, data } => {
            let data = data.unwrap();
            PaneNode::Split {
                left: Box::new(pane_tree(
                    left, tab_id, window_id, active, zoomed, workspace, left_col, top_row,
                )),
                right: Box::new(pane_tree(
                    right,
                    tab_id,
                    window_id,
                    active,
                    zoomed,
                    workspace,
                    if data.direction == SplitDirection::Vertical {
                        left_col
                    } else {
                        left_col + data.left_of_second()
                    },
                    if data.direction == SplitDirection::Horizontal {
                        top_row
                    } else {
                        top_row + data.top_of_second()
                    },
                )),
                node: data,
            }
        }
        Tree::Leaf(pane) => {
            let dims = pane.get_dimensions();
            let working_dir = pane.get_current_working_dir(CachePolicy::AllowStale);
            let cursor_pos = pane.get_cursor_position();

            PaneNode::Leaf(PaneEntry {
                window_id,
                tab_id,
                pane_id: pane.pane_id(),
                title: pane.get_title(),
                is_active_pane: is_pane(pane, &active),
                is_zoomed_pane: is_pane(pane, &zoomed),
                size: TerminalSize {
                    cols: dims.cols,
                    rows: dims.viewport_rows,
                    pixel_height: dims.pixel_height,
                    pixel_width: dims.pixel_width,
                    dpi: dims.dpi,
                },
                working_dir: working_dir.map(Into::into),
                workspace: workspace.to_string(),
                cursor_pos,
                physical_top: dims.physical_top,
                left_col,
                top_row,
                tty_name: pane.tty_name(),
            })
        }
    }
}

pub(super) fn build_from_pane_tree<F>(
    tree: BinaryTree<PaneEntry, SplitDirectionAndSize>,
    active: &mut Option<Arc<dyn Pane>>,
    zoomed: &mut Option<Arc<dyn Pane>>,
    make_pane: &mut F,
) -> Tree
where
    F: FnMut(PaneEntry) -> Arc<dyn Pane>,
{
    match tree {
        BinaryTree::Empty => Tree::Empty,
        BinaryTree::Node { left, right, data } => Tree::Node {
            left: Box::new(build_from_pane_tree(*left, active, zoomed, make_pane)),
            right: Box::new(build_from_pane_tree(*right, active, zoomed, make_pane)),
            data,
        },
        BinaryTree::Leaf(entry) => {
            let is_zoomed_pane = entry.is_zoomed_pane;
            let is_active_pane = entry.is_active_pane;
            let pane = make_pane(entry);
            if is_zoomed_pane {
                zoomed.replace(Arc::clone(&pane));
            }
            if is_active_pane {
                active.replace(Arc::clone(&pane));
            }
            Tree::Leaf(pane)
        }
    }
}

/// Computes the minimum (x, y) size based on the panes in this portion
/// of the tree.
pub(super) fn compute_min_size(tree: &mut Tree) -> (usize, usize) {
    match tree {
        Tree::Node { data: None, .. } | Tree::Empty => (1, 1),
        Tree::Node {
            left,
            right,
            data: Some(data),
        } => {
            let (left_x, left_y) = compute_min_size(&mut *left);
            let (right_x, right_y) = compute_min_size(&mut *right);
            match data.direction {
                SplitDirection::Vertical => (left_x.max(right_x), left_y + right_y + 1),
                SplitDirection::Horizontal => (left_x + right_x + 1, left_y.max(right_y)),
            }
        }
        Tree::Leaf(_) => (1, 1),
    }
}

/// Adjusts the horizontal (column) size of the tree by `x_adjust` cells,
/// which is the delta applied to the *total* width of `tree`.
///
/// Two structurally different cases arise, depending on the split
/// direction of each node we recurse through:
///
/// * `SplitDirection::Vertical` (children stacked top/bottom): both
///   children span the *entire* width of the container, so the whole
///   delta is applied identically to both sides. There is no ratio to
///   preserve here -- `first.cols` and `second.cols` are always equal.
/// * `SplitDirection::Horizontal` (children side by side): `first.cols`
///   and `second.cols` partition the container's width between them, so
///   growing/shrinking the container needs to decide how much of the
///   delta goes to each side. We preserve the *ratio* between `first`
///   and `second` (as it was immediately prior to this resize) rather
///   than handing the entire delta to one side, which is what kept a
///   user's e.g. 40/60 split creeping towards 90/10 (or worse) every
///   time the containing tab was resized -- see #5011, #6052.
pub(super) fn adjust_x_size(tree: &mut Tree, x_adjust: isize, cell_dimensions: &TerminalSize) {
    if x_adjust == 0 {
        return;
    }
    let (min_x, _) = compute_min_size(tree);
    match tree {
        Tree::Empty | Tree::Leaf(_) => {}
        Tree::Node { data: None, .. } => {}
        Tree::Node {
            left,
            right,
            data: Some(data),
        } => {
            data.first.dpi = cell_dimensions.dpi;
            data.second.dpi = cell_dimensions.dpi;
            match data.direction {
                SplitDirection::Vertical => {
                    let new_cols = (data.first.cols as isize)
                        .saturating_add(x_adjust)
                        .max(min_x as isize);
                    let x_adjust = new_cols.saturating_sub(data.first.cols as isize);

                    if x_adjust != 0 {
                        adjust_x_size(&mut *left, x_adjust, cell_dimensions);
                        data.first.cols = new_cols.try_into().unwrap();
                        data.first.pixel_width =
                            data.first.cols.saturating_mul(cell_dimensions.pixel_width);

                        adjust_x_size(&mut *right, x_adjust, cell_dimensions);
                        data.second.cols = data.first.cols;
                        data.second.pixel_width = data.first.pixel_width;
                    }
                }
                SplitDirection::Horizontal => {
                    let (left_min_x, _) = compute_min_size(&mut *left);
                    let (right_min_x, _) = compute_min_size(&mut *right);

                    let total_before = data.first.cols + data.second.cols;
                    let total_after = (total_before as isize + x_adjust)
                        .max((left_min_x + right_min_x) as isize)
                        as usize;

                    let ratio = if total_before == 0 {
                        0.5
                    } else {
                        data.first.cols as f64 / total_before as f64
                    };

                    let new_first = ((total_after as f64 * ratio).round() as isize)
                        .max(left_min_x as isize)
                        .min(total_after as isize - right_min_x as isize)
                        as usize;
                    let new_second = total_after.saturating_sub(new_first);

                    let left_delta = new_first as isize - data.first.cols as isize;
                    let right_delta = new_second as isize - data.second.cols as isize;

                    adjust_x_size(&mut *left, left_delta, cell_dimensions);
                    data.first.cols = new_first;
                    data.first.pixel_width =
                        data.first.cols.saturating_mul(cell_dimensions.pixel_width);

                    adjust_x_size(&mut *right, right_delta, cell_dimensions);
                    data.second.cols = new_second;
                    data.second.pixel_width =
                        data.second.cols.saturating_mul(cell_dimensions.pixel_width);
                }
            }
        }
    }
}

/// Adjusts the vertical (row) size of the tree by `y_adjust` cells. This is
/// the row/height mirror of `adjust_x_size` -- see its doc comment for the
/// rationale. Here it is `SplitDirection::Horizontal` (children side by
/// side) that shares a single row count across both children, and
/// `SplitDirection::Vertical` (children stacked top/bottom) that
/// partitions the height between `first` and `second`, and therefore
/// needs the before-resize ratio preserved.
pub(super) fn adjust_y_size(tree: &mut Tree, y_adjust: isize, cell_dimensions: &TerminalSize) {
    if y_adjust == 0 {
        return;
    }
    let (_, min_y) = compute_min_size(tree);
    match tree {
        Tree::Empty | Tree::Leaf(_) => {}
        Tree::Node { data: None, .. } => {}
        Tree::Node {
            left,
            right,
            data: Some(data),
        } => {
            data.first.dpi = cell_dimensions.dpi;
            data.second.dpi = cell_dimensions.dpi;
            match data.direction {
                SplitDirection::Horizontal => {
                    let new_rows = (data.first.rows as isize)
                        .saturating_add(y_adjust)
                        .max(min_y as isize);
                    let y_adjust = new_rows.saturating_sub(data.first.rows as isize);

                    if y_adjust != 0 {
                        adjust_y_size(&mut *left, y_adjust, cell_dimensions);
                        data.first.rows = new_rows.try_into().unwrap();
                        data.first.pixel_height =
                            data.first.rows.saturating_mul(cell_dimensions.pixel_height);

                        adjust_y_size(&mut *right, y_adjust, cell_dimensions);
                        data.second.rows = data.first.rows;
                        data.second.pixel_height = data.first.pixel_height;
                    }
                }
                SplitDirection::Vertical => {
                    let (_, left_min_y) = compute_min_size(&mut *left);
                    let (_, right_min_y) = compute_min_size(&mut *right);

                    let total_before = data.first.rows + data.second.rows;
                    let total_after = (total_before as isize + y_adjust)
                        .max((left_min_y + right_min_y) as isize)
                        as usize;

                    let ratio = if total_before == 0 {
                        0.5
                    } else {
                        data.first.rows as f64 / total_before as f64
                    };

                    let new_first = ((total_after as f64 * ratio).round() as isize)
                        .max(left_min_y as isize)
                        .min(total_after as isize - right_min_y as isize)
                        as usize;
                    let new_second = total_after.saturating_sub(new_first);

                    let left_delta = new_first as isize - data.first.rows as isize;
                    let right_delta = new_second as isize - data.second.rows as isize;

                    adjust_y_size(&mut *left, left_delta, cell_dimensions);
                    data.first.rows = new_first;
                    data.first.pixel_height =
                        data.first.rows.saturating_mul(cell_dimensions.pixel_height);

                    adjust_y_size(&mut *right, right_delta, cell_dimensions);
                    data.second.rows = new_second;
                    data.second.pixel_height = data
                        .second
                        .rows
                        .saturating_mul(cell_dimensions.pixel_height);
                }
            }
        }
    }
}

pub(super) fn apply_sizes_from_splits(tree: &Tree, size: &TerminalSize) {
    match tree {
        Tree::Empty => {}
        Tree::Node { data: None, .. } => {}
        Tree::Node {
            left,
            right,
            data: Some(data),
        } => {
            apply_sizes_from_splits(left, &data.first);
            apply_sizes_from_splits(right, &data.second);
        }
        Tree::Leaf(pane) => {
            pane.resize(*size).ok();
        }
    }
}

pub(super) fn cell_dimensions(size: &TerminalSize) -> TerminalSize {
    TerminalSize {
        rows: 1,
        cols: 1,
        pixel_width: size.pixel_width / size.cols,
        pixel_height: size.pixel_height / size.rows,
        dpi: size.dpi,
    }
}
