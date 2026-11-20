use nsh_base::{button, theme};
use nsh_core::prelude::*;

pub fn compact() -> Memo<bool> {
    computed(|| window().width.0 < 768.0)
}
