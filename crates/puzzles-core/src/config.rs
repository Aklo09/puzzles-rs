//! Configuration items: the fields of a game's "Custom" dialog and of its
//! preferences (C's `config_item`, `puzzles.h`, upstream 616da16).

/// One field of a configuration dialog or preferences list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigItem {
    /// The label shown to the user.
    pub name: &'static str,
    /// The keyword used in preference files (C's `kw`); only preferences
    /// have one.
    pub keyword: Option<&'static str>,
    /// The field's type and value.
    pub value: ConfigValue,
}

/// A configuration field's type and current value (C's `C_STRING`,
/// `C_CHOICES`, `C_BOOLEAN` and the matching `u` member).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigValue {
    /// Free text (numbers are entered as text, as in C).
    String(String),
    /// One of a fixed list (C's `choicenames`, which packs the list into
    /// one string led by its delimiter, such as `":Easy:Hard"`).
    Choices {
        /// The options shown.
        names: &'static [&'static str],
        /// Their keywords in preference files (C's `choicekws`); only
        /// preferences have them.
        keywords: Option<&'static [&'static str]>,
        /// Index of the chosen option.
        selected: usize,
    },
    /// On or off.
    Bool(bool),
}

impl ConfigItem {
    /// A text field.
    #[must_use]
    pub fn string(name: &'static str, value: impl Into<String>) -> Self {
        Self {
            name,
            keyword: None,
            value: ConfigValue::String(value.into()),
        }
    }

    /// A choice field.
    #[must_use]
    pub fn choices(name: &'static str, names: &'static [&'static str], selected: usize) -> Self {
        Self {
            name,
            keyword: None,
            value: ConfigValue::Choices {
                names,
                keywords: None,
                selected,
            },
        }
    }

    /// A tick box.
    #[must_use]
    pub fn boolean(name: &'static str, value: bool) -> Self {
        Self {
            name,
            keyword: None,
            value: ConfigValue::Bool(value),
        }
    }

    /// The text, if this is a text field.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            ConfigValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// The chosen index, if this is a choice field.
    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        match self.value {
            ConfigValue::Choices { selected, .. } => Some(selected),
            _ => None,
        }
    }

    /// The value, if this is a tick box.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self.value {
            ConfigValue::Bool(b) => Some(b),
            _ => None,
        }
    }
}
