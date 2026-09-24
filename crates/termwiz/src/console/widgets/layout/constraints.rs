/// Specify whether a width or a height has a preferred fixed size
/// or whether it should occupy a percentage of its parent container.
/// The default is 100% of the parent container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DimensionSpec {
    /// Occupy a fixed number of cells
    Fixed(u16),
    /// Occupy a percentage of the space in the parent container
    Percentage(u8),
}

impl Default for DimensionSpec {
    fn default() -> Self {
        DimensionSpec::Percentage(100)
    }
}

/// Specifies the extent of a width or height.  The `spec` field
/// holds the preferred size, while the `minimum` and `maximum`
/// fields set optional lower and upper bounds.
#[derive(Clone, Default, Copy, Debug, PartialEq, Eq)]
pub struct Dimension {
    pub spec: DimensionSpec,
    pub maximum: Option<u16>,
    pub minimum: Option<u16>,
}

/// Specifies whether the children of a widget are laid out
/// vertically (top to bottom) or horizontally (left to right).
/// The default is horizontal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildOrientation {
    Vertical,
    Horizontal,
}

impl Default for ChildOrientation {
    fn default() -> Self {
        ChildOrientation::Horizontal
    }
}

/// Specifies whether the widget should be aligned to the top,
/// middle or bottom of the vertical space in its parent.
/// The default is Top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAlignment {
    Top,
    Middle,
    Bottom,
}

impl Default for VerticalAlignment {
    fn default() -> Self {
        VerticalAlignment::Top
    }
}

/// Specifies whether the widget should be aligned to the left,
/// center or right of the horizontal space in its parent.
/// The default is Left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalAlignment {
    Left,
    Center,
    Right,
}

impl Default for HorizontalAlignment {
    fn default() -> Self {
        HorizontalAlignment::Left
    }
}

/// Specifies the size constraints for a widget
#[derive(Clone, Default, Copy, Debug, PartialEq, Eq)]
pub struct Constraints {
    pub width: Dimension,
    pub height: Dimension,
    pub valign: VerticalAlignment,
    pub halign: HorizontalAlignment,
    pub child_orientation: ChildOrientation,
}

impl Constraints {
    pub fn with_fixed_width_height(width: u16, height: u16) -> Self {
        *Self::default()
            .set_fixed_width(width)
            .set_fixed_height(height)
    }

    pub fn set_fixed_width(&mut self, width: u16) -> &mut Self {
        self.width = Dimension {
            spec: DimensionSpec::Fixed(width),
            minimum: Some(width),
            maximum: Some(width),
        };
        self
    }

    pub fn set_pct_width(&mut self, width: u8) -> &mut Self {
        self.width = Dimension {
            spec: DimensionSpec::Percentage(width),
            ..Default::default()
        };
        self
    }

    pub fn set_fixed_height(&mut self, height: u16) -> &mut Self {
        self.height = Dimension {
            spec: DimensionSpec::Fixed(height),
            minimum: Some(height),
            maximum: Some(height),
        };
        self
    }

    pub fn set_pct_height(&mut self, height: u8) -> &mut Self {
        self.height = Dimension {
            spec: DimensionSpec::Percentage(height),
            ..Default::default()
        };
        self
    }

    pub fn set_valign(&mut self, valign: VerticalAlignment) -> &mut Self {
        self.valign = valign;
        self
    }

    pub fn set_halign(&mut self, halign: HorizontalAlignment) -> &mut Self {
        self.halign = halign;
        self
    }
}
