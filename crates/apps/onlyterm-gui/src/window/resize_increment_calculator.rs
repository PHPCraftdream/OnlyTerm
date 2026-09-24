use ::window::parameters::Border;
use ::window::ResizeIncrement;

pub struct ResizeIncrementCalculator {
    pub x: u16,
    pub y: u16,
    pub padding_left: usize,
    pub padding_top: usize,
    pub padding_right: usize,
    pub padding_bottom: usize,
    pub border: Border,
    pub tab_bar_height: usize,
}

impl From<ResizeIncrementCalculator> for ResizeIncrement {
    fn from(val: ResizeIncrementCalculator) -> Self {
        ResizeIncrement {
            x: val.x,
            y: val.y,
            base_width: (val.padding_left
                + val.padding_right
                + (val.border.left + val.border.right).get()) as u16,
            base_height: (val.padding_top
                + val.padding_bottom
                + (val.border.top + val.border.bottom).get()
                + val.tab_bar_height) as u16,
        }
    }
}
