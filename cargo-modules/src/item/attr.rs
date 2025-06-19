// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fmt;

use ra_ap_cfg::{self as cfg};
use ra_ap_ide::{self as ide};

use crate::{analyzer, item::Item};

#[derive(Clone, PartialEq, Debug)]
pub enum ItemCfgAttr {
    Flag(String),
    KeyValue(String, String),
    All(Vec<Self>),
    Any(Vec<Self>),
    Not(Box<Self>),
}

impl ItemCfgAttr {
    pub fn new(cfg: &cfg::CfgExpr) -> Option<Self> {
        match cfg {
            cfg::CfgExpr::Invalid => None,
            cfg::CfgExpr::Atom(cfg::CfgAtom::Flag(flag)) => Some(Self::Flag(flag.to_string())),
            cfg::CfgExpr::Atom(cfg::CfgAtom::KeyValue { key, value }) => {
                Some(Self::KeyValue(key.to_string(), value.to_string()))
            }
            cfg::CfgExpr::All(cfgs) => Some(Self::All(cfgs.iter().filter_map(Self::new).collect())),
            cfg::CfgExpr::Any(cfgs) => Some(Self::Any(cfgs.iter().filter_map(Self::new).collect())),
            cfg::CfgExpr::Not(cfg) => Self::new(cfg).map(|cfg| Self::Not(Box::new(cfg))),
        }
    }
}

impl fmt::Display for ItemCfgAttr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn fmt_cfgs(f: &mut fmt::Formatter<'_>, cfgs: &[ItemCfgAttr]) -> fmt::Result {
            let mut is_first = true;
            for cfg in cfgs {
                if !is_first {
                    write!(f, ", ")?;
                }
                is_first = false;
                write!(f, "{cfg}")?;
            }
            Ok(())
        }

        match self {
            Self::Flag(content) => {
                write!(f, "{content}")?;
            }
            Self::KeyValue(key, value) => {
                write!(f, "{key} = {value:?}")?;
            }
            Self::All(cfgs) => {
                write!(f, "all(")?;
                fmt_cfgs(f, cfgs)?;
                write!(f, ")")?;
            }
            Self::Any(cfgs) => {
                write!(f, "any(")?;
                fmt_cfgs(f, cfgs)?;
                write!(f, ")")?;
            }
            Self::Not(cfg) => {
                write!(f, "not(")?;
                write!(f, "{}", *cfg)?;
                write!(f, ")")?;
            }
        }

        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ItemTestAttr;

impl fmt::Display for ItemTestAttr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "test")
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct ItemAttrs {
    pub cfgs: Vec<ItemCfgAttr>,
    pub test: Option<ItemTestAttr>,
}

impl ItemAttrs {
    pub fn new(item: &Item, db: &ide::RootDatabase) -> ItemAttrs {
        let cfgs: Vec<_> = analyzer::cfg_attrs(item.hir, db);
        let test = analyzer::test_attr(item.hir, db);
        Self { cfgs, test }
    }

    pub fn is_empty(&self) -> bool {
        self.test.is_none() && self.cfgs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_ap_hir::Symbol;

    #[test]
    fn test_cfg_attr_flag() {
        let cfg = cfg::CfgExpr::Atom(cfg::CfgAtom::Flag(Symbol::intern("test")));
        let attr = ItemCfgAttr::new(&cfg).unwrap();

        assert_eq!(attr, ItemCfgAttr::Flag("test".to_string()));
        assert_eq!(attr.to_string(), "test");
    }

    #[test]
    fn test_cfg_attr_key_value() {
        let cfg = cfg::CfgExpr::Atom(cfg::CfgAtom::KeyValue {
            key: Symbol::intern("target_os"),
            value: Symbol::intern("linux"),
        });
        let attr = ItemCfgAttr::new(&cfg).unwrap();

        assert_eq!(
            attr,
            ItemCfgAttr::KeyValue("target_os".to_string(), "linux".to_string())
        );
        assert_eq!(attr.to_string(), "target_os = \"linux\"");
    }

    #[test]
    fn test_cfg_attr_all() {
        let cfgs = vec![
            cfg::CfgExpr::Atom(cfg::CfgAtom::Flag(Symbol::intern("test"))),
            cfg::CfgExpr::Atom(cfg::CfgAtom::Flag(Symbol::intern("debug_assertions"))),
        ];
        let cfg = cfg::CfgExpr::All(cfgs.into_boxed_slice());
        let attr = ItemCfgAttr::new(&cfg).unwrap();

        match &attr {
            ItemCfgAttr::All(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], ItemCfgAttr::Flag("test".to_string()));
                assert_eq!(items[1], ItemCfgAttr::Flag("debug_assertions".to_string()));
            }
            _ => panic!("Expected All variant"),
        }
        assert_eq!(attr.to_string(), "all(test, debug_assertions)");
    }

    #[test]
    fn test_cfg_attr_any() {
        let cfgs = vec![
            cfg::CfgExpr::Atom(cfg::CfgAtom::KeyValue {
                key: Symbol::intern("target_os"),
                value: Symbol::intern("linux"),
            }),
            cfg::CfgExpr::Atom(cfg::CfgAtom::KeyValue {
                key: Symbol::intern("target_os"),
                value: Symbol::intern("macos"),
            }),
        ];
        let cfg = cfg::CfgExpr::Any(cfgs.into_boxed_slice());
        let attr = ItemCfgAttr::new(&cfg).unwrap();

        match &attr {
            ItemCfgAttr::Any(items) => {
                assert_eq!(items.len(), 2);
            }
            _ => panic!("Expected Any variant"),
        }
        assert_eq!(
            attr.to_string(),
            "any(target_os = \"linux\", target_os = \"macos\")"
        );
    }

    #[test]
    fn test_cfg_attr_not() {
        let inner = cfg::CfgExpr::Atom(cfg::CfgAtom::Flag(Symbol::intern("test")));
        let cfg = cfg::CfgExpr::Not(Box::new(inner));
        let attr = ItemCfgAttr::new(&cfg).unwrap();

        match &attr {
            ItemCfgAttr::Not(inner) => {
                assert_eq!(**inner, ItemCfgAttr::Flag("test".to_string()));
            }
            _ => panic!("Expected Not variant"),
        }
        assert_eq!(attr.to_string(), "not(test)");
    }

    #[test]
    fn test_cfg_attr_invalid() {
        let cfg = cfg::CfgExpr::Invalid;
        let attr = ItemCfgAttr::new(&cfg);
        assert!(attr.is_none());
    }

    #[test]
    fn test_item_test_attr_display() {
        let attr = ItemTestAttr;
        assert_eq!(attr.to_string(), "test");
    }

    #[test]
    fn test_item_attrs_is_empty() {
        let attrs = ItemAttrs {
            cfgs: vec![],
            test: None,
        };
        assert!(attrs.is_empty());

        let attrs = ItemAttrs {
            cfgs: vec![ItemCfgAttr::Flag("test".to_string())],
            test: None,
        };
        assert!(!attrs.is_empty());

        let attrs = ItemAttrs {
            cfgs: vec![],
            test: Some(ItemTestAttr),
        };
        assert!(!attrs.is_empty());
    }
}
