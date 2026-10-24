//! Dota's cheat commands typed into the chat (`-gold 5000`) or the console
//! (`dota_create_item item_blink`), as Dota takes them: the client library's
//! parser with Dota's own names for items and heroes (this client's ids
//! file), and the ground under the cursor for what Dota puts there. Only a
//! match started with cheats honours them; any other refuses them.

use bota_client_core::{App as Core, CommandContext, Intent, Plain, parse_command, world_vec};
use bota_proto::{HeroId, ItemId, Vec2};

use crate::manifest::Manifest;

/// Dota's names, from the ids file, and the cursor's spot.
pub struct Names<'a> {
    pub manifest: &'a Manifest,
    /// The ground under the cursor, in the sim's coordinates.
    pub cursor: Option<(f32, f32)>,
}

impl Names<'_> {
    /// The id of a Dota name of a kind: the whole name first (with or
    /// without its prefix), else the first that holds it, as Dota matches.
    fn find(&self, kind: &str, prefix: &str, word: &str) -> Option<u16> {
        let word = word.to_ascii_lowercase();
        let word = word.strip_prefix(prefix).unwrap_or(&word);
        if word.is_empty() {
            return None;
        }
        let named = || self.manifest.ids.iter().filter(|(k, _, _)| k == kind);
        named()
            .find(|(_, _, name)| name == word)
            .or_else(|| named().find(|(_, _, name)| name.contains(word)))
            .map(|(_, id, _)| *id)
    }
}

impl CommandContext for Names<'_> {
    fn item(&self, word: &str) -> Option<ItemId> {
        self.find("item", "item_", word)
            .map(ItemId)
            .filter(|item| bota_core::item_def(*item).is_some())
            .or_else(|| Plain.item(word))
    }

    fn hero(&self, word: &str) -> Option<HeroId> {
        self.find("hero", "npc_dota_hero_", word)
            .map(HeroId)
            .filter(|hero| bota_core::hero_def(*hero).is_some())
            .or_else(|| Plain.hero(word))
    }

    fn cursor(&self) -> Option<Vec2> {
        self.cursor.map(world_vec)
    }
}

/// Runs one typed command: the cheat goes to the server, or the client says
/// why it is no command.
pub fn run(core: &mut Core, line: &str, names: &Names) -> Result<(), String> {
    let cheat = parse_command(line, names)?;
    core.act(Intent::Cheat(cheat));
    Ok(())
}
