use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KittyImagePlacement {
    /// source rectangle bounds.
    /// Default is whole image.
    /// x=...
    pub x: Option<u32>,
    pub y: Option<u32>,
    pub w: Option<u32>,
    pub h: Option<u32>,
    /// Place the image at an offset from the cell.
    /// X,Y must be <= cell metrics
    /// X=...
    pub x_offset: Option<u32>,
    /// Y=...
    pub y_offset: Option<u32>,
    /// Scale so that the image fits within this number of columns
    /// c=...
    pub columns: Option<u32>,
    /// Scale so that the image fits within this number of rows
    /// r=...
    pub rows: Option<u32>,
    /// By default, cursor will move to after the bottom right
    /// cell of the image placement.  do_not_move_cursor cursor
    /// set to true prevents that.
    /// C=0, C=1
    pub do_not_move_cursor: bool,
    /// Give an explicit placement id to this placement.
    /// p=...
    pub placement_id: Option<u32>,
    /// z=...
    pub z_index: Option<i32>,
}

impl KittyImagePlacement {
    pub(super) fn from_keys(keys: &BTreeMap<&str, &str>) -> Option<Self> {
        Some(Self {
            x: geti(keys, "x"),
            y: geti(keys, "y"),
            w: geti(keys, "w"),
            h: geti(keys, "h"),
            x_offset: geti(keys, "X"),
            y_offset: geti(keys, "Y"),
            columns: geti(keys, "c"),
            rows: geti(keys, "r"),
            placement_id: geti(keys, "p"),
            do_not_move_cursor: match get(keys, "C") {
                None | Some("0") => false,
                Some("1") => true,
                _ => return None,
            },
            z_index: geti(keys, "z"),
        })
    }

    pub(super) fn to_keys(&self, keys: &mut BTreeMap<&'static str, String>) {
        set(keys, "x", &self.x);
        set(keys, "y", &self.y);
        set(keys, "w", &self.w);
        set(keys, "h", &self.h);
        set(keys, "X", &self.x_offset);
        set(keys, "Y", &self.y_offset);
        set(keys, "c", &self.columns);
        set(keys, "r", &self.rows);
        set(keys, "p", &self.placement_id);

        if self.do_not_move_cursor {
            keys.insert("C", "1".to_string());
        }

        set(keys, "z", &self.z_index);
    }
}
