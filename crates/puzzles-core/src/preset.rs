//! Preset menus (C's `struct preset_menu`, `puzzles.h`, upstream 616da16).
//!
//! A game builds its menu of preset parameters, possibly with submenus.
//! C's per-entry `id` numbers are assigned by the mid-end, so they live
//! there, not in what games build.

/// A menu of presets and submenus, in display order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetMenu<P> {
    /// The entries, in display order.
    pub entries: Vec<PresetEntry<P>>,
}

/// One entry of a [`PresetMenu`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresetEntry<P> {
    /// A preset: choosing it starts a game with `params`.
    Preset {
        /// The menu label, such as "5x5 Easy".
        title: String,
        /// The parameters it selects.
        params: P,
    },
    /// A submenu.
    Submenu {
        /// The menu label.
        title: String,
        /// Its entries.
        menu: PresetMenu<P>,
    },
}

impl<P> Default for PresetMenu<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P> PresetMenu<P> {
    /// An empty menu (C's `preset_menu_new`).
    #[doc(alias = "preset_menu_new")]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Appends a preset (C's `preset_menu_add_preset`).
    #[doc(alias = "preset_menu_add_preset")]
    pub fn add_preset(&mut self, title: impl Into<String>, params: P) {
        self.entries.push(PresetEntry::Preset {
            title: title.into(),
            params,
        });
    }

    /// Appends an empty submenu and returns it for filling (C's
    /// `preset_menu_add_submenu`).
    #[doc(alias = "preset_menu_add_submenu")]
    pub fn add_submenu(&mut self, title: impl Into<String>) -> &mut Self {
        self.entries.push(PresetEntry::Submenu {
            title: title.into(),
            menu: Self::new(),
        });
        match self.entries.last_mut() {
            Some(PresetEntry::Submenu { menu, .. }) => menu,
            _ => unreachable!("just pushed a submenu"),
        }
    }

    /// True if the menu has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every preset, depth first in display order.
    pub fn presets(&self) -> impl Iterator<Item = (&str, &P)> {
        let mut stack = vec![self.entries.iter()];
        std::iter::from_fn(move || {
            while let Some(entries) = stack.last_mut() {
                match entries.next() {
                    Some(PresetEntry::Preset { title, params }) => {
                        return Some((title.as_str(), params));
                    }
                    Some(PresetEntry::Submenu { menu, .. }) => stack.push(menu.entries.iter()),
                    None => {
                        stack.pop();
                    }
                }
            }
            None
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_listed_depth_first() {
        let mut menu = PresetMenu::new();
        menu.add_preset("3x3", 3);
        let sub = menu.add_submenu("Large");
        sub.add_preset("9x9", 9);
        sub.add_submenu("Huge").add_preset("20x20", 20);
        menu.add_preset("5x5", 5);
        let all: Vec<_> = menu.presets().collect();
        assert_eq!(all, [("3x3", &3), ("9x9", &9), ("20x20", &20), ("5x5", &5)]);
        assert!(!menu.is_empty());
        assert!(PresetMenu::<u8>::default().is_empty());
    }
}
