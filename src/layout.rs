#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Columns {
    pub compact: bool,
    pub too_narrow: bool,
}

pub fn choose_columns(width: u16) -> Columns {
    if width < 30 {
        Columns { compact: true, too_narrow: true }
    } else if width < 60 {
        Columns { compact: true, too_narrow: false }
    } else {
        Columns { compact: false, too_narrow: false }
    }
}
