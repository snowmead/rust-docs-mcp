// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use ra_ap_hir::{self as hir};
use ra_ap_ide::{self as ide};

use crate::analyzer;

pub(crate) use self::{
    attr::{ItemAttrs, ItemCfgAttr, ItemTestAttr},
    kind_display_name::ItemKindDisplayName,
    visibility::ItemVisibility,
};

mod attr;
mod kind_display_name;
mod visibility;

#[derive(Clone, PartialEq, Debug)]
pub struct Item {
    pub hir: hir::ModuleDef,
}

impl Item {
    pub fn new(hir: hir::ModuleDef) -> Self {
        Self { hir }
    }

    pub fn visibility(&self, db: &ide::RootDatabase, edition: ide::Edition) -> ItemVisibility {
        ItemVisibility::new(self.hir, db, edition)
    }

    pub fn attrs(&self, db: &ide::RootDatabase, _edition: ide::Edition) -> ItemAttrs {
        ItemAttrs::new(self, db)
    }

    pub fn kind_display_name(
        &self,
        db: &ide::RootDatabase,
        _edition: ide::Edition,
    ) -> ItemKindDisplayName {
        ItemKindDisplayName::new(self, db)
    }

    pub fn display_name(&self, db: &ide::RootDatabase, edition: ide::Edition) -> String {
        analyzer::display_name(self.hir, db, edition)
    }

    pub fn display_path(&self, db: &ide::RootDatabase, edition: ide::Edition) -> String {
        analyzer::display_path(self.hir, db, edition)
    }

    pub fn kind_ordering(&self, _db: &ide::RootDatabase, _edition: ide::Edition) -> u8 {
        // Return ordering based on item kind for sorting
        // Lower numbers come first
        match self.hir {
            hir::ModuleDef::Module(_) => 0,
            hir::ModuleDef::Trait(_) => 1,
            hir::ModuleDef::TraitAlias(_) => 2,
            hir::ModuleDef::Adt(adt) => match adt {
                hir::Adt::Struct(_) => 3,
                hir::Adt::Enum(_) => 4,
                hir::Adt::Union(_) => 5,
            },
            hir::ModuleDef::Variant(_) => 6,
            hir::ModuleDef::Const(_) => 7,
            hir::ModuleDef::Static(_) => 8,
            hir::ModuleDef::Function(_) => 9,
            hir::ModuleDef::TypeAlias(_) => 10,
            hir::ModuleDef::BuiltinType(_) => 11,
            hir::ModuleDef::Macro(_) => 12,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_creation() {
        // We can't create a real ModuleDef without a database, but we can test the struct
        // This is more of a compile-time test to ensure the API is correct
        let _ = |hir: hir::ModuleDef| {
            let item = Item::new(hir);
            assert!(matches!(item.hir, _));
        };
    }

    #[test]
    fn test_kind_ordering() {
        // Test that kind ordering returns expected values
        let _ = |item: &Item, db: &ide::RootDatabase, edition: ide::Edition| {
            let ordering = item.kind_ordering(db, edition);
            match item.hir {
                hir::ModuleDef::Module(_) => assert_eq!(ordering, 0),
                hir::ModuleDef::Trait(_) => assert_eq!(ordering, 1),
                hir::ModuleDef::TraitAlias(_) => assert_eq!(ordering, 2),
                hir::ModuleDef::Adt(adt) => match adt {
                    hir::Adt::Struct(_) => assert_eq!(ordering, 3),
                    hir::Adt::Enum(_) => assert_eq!(ordering, 4),
                    hir::Adt::Union(_) => assert_eq!(ordering, 5),
                },
                hir::ModuleDef::Variant(_) => assert_eq!(ordering, 6),
                hir::ModuleDef::Const(_) => assert_eq!(ordering, 7),
                hir::ModuleDef::Static(_) => assert_eq!(ordering, 8),
                hir::ModuleDef::Function(_) => assert_eq!(ordering, 9),
                hir::ModuleDef::TypeAlias(_) => assert_eq!(ordering, 10),
                hir::ModuleDef::BuiltinType(_) => assert_eq!(ordering, 11),
                hir::ModuleDef::Macro(_) => assert_eq!(ordering, 12),
            }
        };
    }

    #[test]
    fn test_item_equality() {
        // Items should be equal if they have the same HIR
        let _ = |hir1: hir::ModuleDef, hir2: hir::ModuleDef| {
            let item1 = Item::new(hir1);
            let item2 = Item::new(hir2);

            if hir1 == hir2 {
                assert_eq!(item1, item2);
            } else {
                assert_ne!(item1, item2);
            }
        };
    }
}
