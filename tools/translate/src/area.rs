//! Which of `fenji-content`'s files a line goes in, and whether the interface holds it tight
//! (retranslation spec §6.2, §6.6, RT11), from where the game uses it.
use serde::{Deserialize, Serialize};

use crate::lines::Use;

/// An area of the game's text, in the order a line used in several areas takes the first of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Area {
    Interface,
    Items,
    Skills,
    Quests,
    Dialogue,
    Lore,
    Other,
}

impl Area {
    /// Its file's name in `text/en/`, without `.tsv`.
    pub fn name(self) -> &'static str {
        match self {
            Area::Interface => "interface",
            Area::Items => "items",
            Area::Skills => "skills",
            Area::Quests => "quests",
            Area::Dialogue => "dialogue",
            Area::Lore => "lore",
            Area::Other => "other",
        }
    }
}

/// The design tables' sheets by name prefix, the first match winning, from the cache's 339 sheet
/// names (m2-tables.md §3). The other sheets, NPCs' and maps' among them, are [`Area::Other`],
/// whose names the glossary carries.
pub const AREAS: &[(&str, Area)] = &[
    ("quest_item", Area::Items),
    ("quest_dialog", Area::Dialogue),
    ("cutscene_dialog", Area::Dialogue),
    ("dialog3d", Area::Dialogue),
    ("npc_menu", Area::Dialogue),
    ("items", Area::Items),
    ("item_source", Area::Items),
    ("fav_item", Area::Items),
    ("equip", Area::Items),
    ("gem", Area::Items),
    ("medicine", Area::Items),
    ("recipe", Area::Items),
    ("cook", Area::Items),
    ("material", Area::Items),
    ("treasure", Area::Items),
    ("exterior", Area::Items),
    ("skins", Area::Items),
    ("pets", Area::Items),
    ("home_goods", Area::Items),
    ("skill", Area::Skills),
    ("passive_skill", Area::Skills),
    ("buff", Area::Skills),
    ("bind_buff", Area::Skills),
    ("mastery", Area::Skills),
    ("prop_mastery", Area::Skills),
    ("prop_passive", Area::Skills),
    ("vocation_skill", Area::Skills),
    ("catalog_skill", Area::Skills),
    ("quest", Area::Quests),
    ("areaquest", Area::Quests),
    ("fairyland_quest", Area::Quests),
    ("guild_quest", Area::Quests),
    ("pubquest", Area::Quests),
    ("book", Area::Lore),
    ("biography", Area::Lore),
    ("fairyland_biography", Area::Lore),
    ("newspaper", Area::Lore),
    ("remnant_page", Area::Lore),
    ("journey", Area::Lore),
    ("researchquestion", Area::Lore),
    ("errors", Area::Interface),
    ("tutorials", Area::Interface),
    ("help_guide", Area::Interface),
    ("helper_guide", Area::Interface),
    ("loading_tips", Area::Interface),
    ("catalog_tips", Area::Interface),
    ("achieve", Area::Interface),
    ("title", Area::Interface),
    ("greenhand", Area::Interface),
    ("npc_function", Area::Interface),
    ("emojis", Area::Interface),
];

/// The fields and columns whose text the interface holds tight: names, titles and labels.
pub const TIGHT_FIELDS: &[&str] = &["name", "title", "label", "button", "tab", "tip"];

fn sheet_area(sheet: &str) -> Area {
    AREAS
        .iter()
        .find(|(prefix, _)| sheet.starts_with(prefix))
        .map_or(Area::Other, |&(_, area)| area)
}

/// A line's area and tightness, from its uses: the first area in [`Area`]'s order that any use
/// has, and tight if any use is. A script's strings are the interface's, and tight.
pub fn classify(uses: &[Use]) -> (Area, bool) {
    let mut area: Option<Area> = None;
    let mut tight = false;
    for u in uses {
        let (a, t) = match u {
            Use::Script { .. } => (Area::Interface, true),
            Use::Tab { column, .. } => (Area::Interface, TIGHT_FIELDS.contains(&column.as_str())),
            Use::Table { sheet, field, .. } => {
                (sheet_area(sheet), TIGHT_FIELDS.contains(&field.as_str()))
            }
        };
        area = Some(area.map_or(a, |known| known.min(a)));
        tight |= t;
    }
    (area.unwrap_or(Area::Other), tight)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lines::Use;

    fn table(sheet: &str, field: &str) -> Use {
        Use::Table {
            sheet: sheet.into(),
            row: "1".into(),
            field: field.into(),
        }
    }

    #[test]
    fn areas_and_tightness_follow_the_uses() {
        let script = Use::Script {
            path: "data/interface/ui/a.luac".into(),
        };
        let prose = table("quest_dialog", "text");
        assert_eq!(
            classify(std::slice::from_ref(&script)),
            (Area::Interface, true)
        );
        assert_eq!(classify(&[table("items", "name")]), (Area::Items, true));
        assert_eq!(classify(&[table("items", "desc")]), (Area::Items, false));
        // The first area in Area's order, and tight if any use is.
        assert_eq!(classify(&[prose.clone(), script]), (Area::Interface, true));
        assert_eq!(classify(&[prose]), (Area::Dialogue, false));
        assert_eq!(
            classify(&[table("quest_item_table", "desc")]).0,
            Area::Items
        );
        assert_eq!(classify(&[table("quest", "desc")]).0, Area::Quests);
        assert_eq!(classify(&[table("skill", "desc")]).0, Area::Skills);
        assert_eq!(classify(&[table("book", "content")]).0, Area::Lore);
        assert_eq!(classify(&[table("npcs", "name")]), (Area::Other, true));
        assert_eq!(classify(&[]), (Area::Other, false));
        assert_eq!(Area::Items.name(), "items");
    }
}
