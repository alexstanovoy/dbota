//! What the shop lists, where, and in which words: Dota's categories with
//! the items each holds in Dota's order, as the cache's `text/shop.json`
//! has them out of the player's own install (`dbota import`), cut to the
//! items bota sells. Without the cache the shop falls back on the client
//! core's own groups and plain words.

use std::{collections::HashMap, path::Path};

use bevy::prelude::*;
use bota_client_core::{SHOP_GROUPS, shop_groups};
use bota_core::item_def;
use bota_proto::ItemId;
use serde_json::Value;

use crate::manifest::Manifest;
use crate::tip_content::Lookup;

/// Where the shop's data lies in the cache.
const FILE: &str = "text/shop.json";
/// Most items a search lists.
pub const MAX_RESULTS: usize = 64;
/// Most items one item builds into that the build tree draws.
pub const MAX_INTO: usize = 6;
/// Most strings and categories read, so a broken cache cannot fill the memory.
const MAX_STRINGS: usize = 1024;
const MAX_CATEGORIES: usize = 64;
const MAX_CATEGORY_ITEMS: usize = 128;

/// A tab of the shop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Basics,
    Upgrades,
    Neutrals,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Basics, Tab::Upgrades, Tab::Neutrals];

    /// Its label: the localization key and the plain word.
    pub fn label(self) -> (&'static str, &'static str) {
        match self {
            Tab::Basics => ("dota_shop_category_basics", "BASICS"),
            Tab::Upgrades => ("dota_shop_category_upgrades", "UPGRADES"),
            Tab::Neutrals => ("dota_neutrals", "NEUTRALS"),
        }
    }
}

/// Dota's shop layout: each tab's categories, as `shops.txt` names them, and
/// the localization key of each one's title. Which items a category holds,
/// and in what order, is the install's.
const LAYOUT: [(Tab, &str, &str); 11] = [
    (Tab::Basics, "consumables", "dota_subshop_consumables"),
    (Tab::Basics, "attributes", "dota_subshop_attributes"),
    (Tab::Basics, "weapons_armor", "dota_subshop_weapons_armor"),
    (Tab::Basics, "misc", "dota_subshop_misc"),
    (Tab::Basics, "secretshop", "dota_subshop_secretshop"),
    (Tab::Upgrades, "basics", "dota_subshop_recipe_basics"),
    (Tab::Upgrades, "support", "dota_subshop_recipe_support"),
    (Tab::Upgrades, "magics", "dota_subshop_recipe_magics"),
    (Tab::Upgrades, "defense", "dota_subshop_recipe_defense"),
    (Tab::Upgrades, "weapons", "dota_subshop_recipe_weapons"),
    (Tab::Upgrades, "artifacts", "dota_subshop_recipe_artifacts"),
];

/// One category of a tab: its title and the items of it bota sells.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub tab: Tab,
    pub title: String,
    pub items: Vec<u16>,
}

/// The shop's categories and words.
#[derive(Resource, Clone, Debug, Default)]
pub struct Catalog {
    /// In Dota's order, tab by tab; categories bota sells nothing of are
    /// left out.
    pub sections: Vec<Section>,
    /// Dota's words by lower-cased localization key.
    strings: HashMap<String, String>,
}

impl Catalog {
    /// The cache's shop data, or the client core's groups without it.
    pub fn load(manifest: &Manifest) -> Catalog {
        let data = manifest.root.as_deref().and_then(read).unwrap_or_default();
        Catalog::build(data, manifest)
    }

    /// The shop of the cache's data, cut to what bota sells.
    fn build(data: Data, manifest: &Manifest) -> Catalog {
        let names: HashMap<&str, u16> = manifest
            .ids
            .iter()
            .filter(|(kind, id, name)| kind == "item" && name != "recipe" && sold(*id))
            .map(|(_, id, name)| (name.as_str(), *id))
            .collect();
        let mut catalog = Catalog {
            sections: Vec::new(),
            strings: data.strings,
        };
        if data.categories.is_empty() {
            catalog.sections = fallback();
            return catalog;
        }
        for (tab, key, title) in LAYOUT {
            let Some((_, items)) = data.categories.iter().find(|(name, _)| name == key) else {
                continue;
            };
            let mut ids: Vec<u16> = Vec::new();
            for id in items
                .iter()
                .filter_map(|item| names.get(item.strip_prefix("item_")?).copied())
            {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            if !ids.is_empty() {
                let title = catalog.word(title, key).to_string();
                catalog.sections.push(Section {
                    tab,
                    title,
                    items: ids,
                });
            }
        }
        catalog.add_leftovers(manifest);
        catalog
    }

    /// Items bota sells that no category of Dota's lists, recipes aside (the
    /// build tree shows those), under the client core's last group.
    fn add_leftovers(&mut self, manifest: &Manifest) {
        let placed = |id: u16, sections: &[Section]| sections.iter().any(|s| s.items.contains(&id));
        let rest: Vec<u16> = bota_client_core::ITEMS
            .iter()
            .map(|face| face.id)
            .filter(|id| sold(*id) && manifest.dota_name("item", *id) != Some("recipe"))
            .filter(|id| !placed(*id, &self.sections))
            .collect();
        if !rest.is_empty() {
            self.sections.push(Section {
                tab: Tab::Basics,
                title: SHOP_GROUPS[SHOP_GROUPS.len() - 1].to_string(),
                items: rest,
            });
        }
    }

    /// Dota's word for a localization key, or the plain one.
    pub fn word<'a>(&'a self, key: &str, plain: &'a str) -> &'a str {
        self.strings.get(key).map_or(plain, String::as_str)
    }

    /// The items a search finds, those whose name starts with it first, each
    /// once, in the shop's order otherwise. An empty search finds none.
    pub fn search(&self, look: &Lookup, query: &str) -> Vec<u16> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let mut found: Vec<(crate::tip_content::Rank, usize, u16)> = Vec::new();
        for id in self.sections.iter().flat_map(|s| s.items.iter().copied()) {
            if found.iter().any(|(_, _, seen)| *seen == id) {
                continue;
            }
            if let Some(rank) = look.search_rank(id, &query) {
                found.push((rank, found.len(), id));
            }
        }
        found.sort_unstable();
        found
            .into_iter()
            .take(MAX_RESULTS)
            .map(|(_, _, id)| id)
            .collect()
    }
}

/// Whether bota sells an item: it exists and is not retired.
fn sold(id: u16) -> bool {
    item_def(ItemId(id)).is_some()
}

/// The parts an item is built from, its first recipe's, in order.
pub fn parts_of(id: u16) -> &'static [ItemId] {
    item_def(ItemId(id)).map_or(&[], |def| def.components)
}

/// The items one is a part of, in id order, at most [`MAX_INTO`].
pub fn builds_into(id: u16) -> Vec<u16> {
    bota_client_core::ITEMS
        .iter()
        .filter(|face| {
            item_def(ItemId(face.id)).is_some_and(|def| {
                def.recipes()
                    .any(|recipe| recipe.parts.contains(&ItemId(id)))
            })
        })
        .map(|face| face.id)
        .take(MAX_INTO)
        .collect()
}

/// The client core's own groups, recipes aside, for a client without the
/// cache: its upgrades on the second tab, the rest on the first.
fn fallback() -> Vec<Section> {
    shop_groups("")
        .into_iter()
        .filter(|(group, _)| SHOP_GROUPS[*group] != "Recipes")
        .map(|(group, items)| Section {
            tab: if SHOP_GROUPS[group] == "Upgrades" {
                Tab::Upgrades
            } else {
                Tab::Basics
            },
            title: SHOP_GROUPS[group].to_string(),
            items,
        })
        .collect()
}

/// What the cache's file holds.
#[derive(Default)]
struct Data {
    categories: Vec<(String, Vec<String>)>,
    strings: HashMap<String, String>,
}

/// Reads the cache's shop file; nothing when it is absent or broken.
fn read(root: &Path) -> Option<Data> {
    let bytes = crate::cachefs::read_small(root, FILE)?;
    let json: Value = match serde_json::from_slice(&bytes) {
        Ok(json) => json,
        Err(err) => {
            warn!("{FILE}: {err}");
            return None;
        }
    };
    let categories = json
        .get("shops")?
        .as_array()?
        .iter()
        .take(MAX_CATEGORIES)
        .filter_map(|pair| {
            let name = pair.get(0)?.as_str()?.to_string();
            let items = pair
                .get(1)?
                .as_array()?
                .iter()
                .take(MAX_CATEGORY_ITEMS)
                .filter_map(|item| Some(item.as_str()?.to_string()))
                .collect();
            Some((name, items))
        })
        .collect();
    let strings = json
        .get("strings")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .take(MAX_STRINGS)
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    Some(Data {
        categories,
        strings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every item bota sells can be bought from the shop's tabs, once,
    /// recipes aside (the build tree has those), whether the cache gives
    /// Dota's categories (whatever they list) or not.
    #[test]
    fn every_item_bota_sells_is_in_the_shop_once() {
        let manifest = Manifest::load(Path::new("/nonexistent"));
        let dota = Data {
            categories: vec![
                (
                    "misc".into(),
                    vec![
                        "item_boots".into(),
                        "item_blink".into(),
                        "item_boots".into(),
                    ],
                ),
                (
                    "weapons".into(),
                    vec!["item_butterfly".into(), "item_rapier".into()],
                ),
                ("sideshop1".into(), vec!["item_tango".into()]),
            ],
            strings: HashMap::new(),
        };
        for catalog in [
            Catalog::build(Data::default(), &manifest),
            Catalog::build(dota, &manifest),
        ] {
            let listed: Vec<u16> = catalog
                .sections
                .iter()
                .flat_map(|s| s.items.clone())
                .collect();
            for face in bota_client_core::ITEMS.iter().filter(|f| sold(f.id)) {
                let recipe = manifest.dota_name("item", face.id) == Some("recipe");
                let times = listed.iter().filter(|id| **id == face.id).count();
                assert_eq!(times, usize::from(!recipe), "{}", face.name);
            }
        }
    }
}
