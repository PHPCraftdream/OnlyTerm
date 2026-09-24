use alloc::string::ToString;
use onlyterm_dynamic::{FromDynamic, ToDynamic};

#[derive(Debug, Default, ToDynamic, PartialEq, Eq, Clone, Copy)]
pub enum IntegratedTitleButtonStyle {
    #[default]
    Windows,
    Gnome,
    MacOsNative,
}

impl FromDynamic for IntegratedTitleButtonStyle {
    fn from_dynamic(
        value: &onlyterm_dynamic::Value,
        _options: onlyterm_dynamic::FromDynamicOptions,
    ) -> Result<Self, onlyterm_dynamic::Error>
    where
        Self: Sized,
    {
        let type_name = "integrated_title_button_style";

        if let onlyterm_dynamic::Value::String(string) = value {
            let style = match string.as_str() {
                "Windows" => Self::Windows,
                "Gnome" => Self::Gnome,
                "MacOsNative" => Self::MacOsNative,
                _ => {
                    return Err(onlyterm_dynamic::Error::InvalidVariantForType {
                        variant_name: string.to_string(),
                        type_name,
                        possible: &["Windows", "Gnome", "MacOsNative"],
                    });
                }
            };
            Ok(style)
        } else {
            Err(onlyterm_dynamic::Error::InvalidVariantForType {
                variant_name: value.variant_name().to_string(),
                type_name,
                possible: &["String"],
            })
        }
    }
}
