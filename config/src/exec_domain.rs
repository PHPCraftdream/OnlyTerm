use crate::config::validate_domain_name;
use crate::impl_rhai_conversion_dynamic;
use wezterm_dynamic::{FromDynamic, ToDynamic, Value};

#[derive(Debug, Clone, FromDynamic, ToDynamic)]
pub enum ValueOrFunc {
    Value(Value),
    Func(String),
}
// Sample application of the L2 rhai-side analogue of `impl_lua_conversion_dynamic!`
// (see `config/src/rhai_value.rs` for why this is a separate macro rather than a
// backend-agnostic extension of the mlua one).
impl_rhai_conversion_dynamic!(ValueOrFunc);

#[derive(Debug, Clone, FromDynamic, ToDynamic)]
pub struct ExecDomain {
    #[dynamic(validate = "validate_domain_name")]
    pub name: String,
    pub fixup_command: String,
    pub label: Option<ValueOrFunc>,
}
impl_rhai_conversion_dynamic!(ExecDomain);
