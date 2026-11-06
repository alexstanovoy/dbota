//! What the client asks its hero's response rules: the concepts it raises and the facts each
//! query of them carries. The importer prunes the rules by it (a rule no query of these can
//! match is never asked) and the client builds its queries from the same table, so the two
//! cannot disagree about what is ever said.
//!
//! Dota's concepts are the `Concept` criteria of `scripts/talker/response_rules_shared.vrr`;
//! when Dota raises each is told on its heroes' response pages (Liquipedia, `<Hero>/Responses`,
//! e.g. https://liquipedia.net/dota2/Pudge/Responses). A concept left out here is one this client
//! has no event for yet; the importer counts the lines it prunes for it.

/// A concept the client raises for the hero it speaks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Concept {
    /// Entering battle: the hero first stands on the map.
    Spawn,
    /// A move order of the player.
    Move,
    /// An attack order of the player.
    Attack,
    /// One of its abilities took effect.
    CastExecute,
    /// It killed an enemy hero.
    Kill,
    /// It last-hit a unit that is not a hero.
    LastHit,
    /// It denied an allied unit.
    Deny,
    /// It gained a level.
    LevelUp,
    /// Its player bought an item.
    Purchase,
    /// It died.
    Death,
    /// It stands again after dying.
    Respawn,
    /// It took a rune into effect.
    Powerup,
    /// It put a rune into its Bottle.
    Bottling,
    /// Its side won.
    Victory,
    /// Its side lost.
    Defeat,
    /// The battle begins (`TLK_DOTA_CUSTOM` with `announce_event` `game_start`: "Beginning
    /// battle" on the response pages).
    GameStart,
}

/// A fact a query carries, by the criterion key that reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Fact {
    /// The concept's own name (`TLK_DOTA_MOVE`).
    Concept,
    /// The speaker's class (`npc_dota_hero_pudge`).
    ClassName,
    /// A random whole number from 0 to 99, drawn for each query: the install's `Chance_N%`
    /// criteria split exactly that range (`Chance_1%` is `99`, `Chance_2%` `>=97,<=98`, ...).
    RandomNum,
    /// Seconds of game time since the battle began (negative before).
    GameTime,
    /// The ability that took effect (`pudge_rot`).
    AbilityName,
    /// The class of the hero killed (`npc_dota_hero_nevermore`).
    TargetClassName,
    /// 1 when the kill is the match's first hero kill, else 0.
    FirstBlood,
    /// The level reached.
    Level,
    /// The item bought (`item_blink`).
    ItemName,
    /// The rune taken (`DOTA_RUNE_BOUNTY`).
    PowerupType,
    /// The hero's last hits, this one included.
    LastHitCount,
    /// What a custom concept announces (`game_start`).
    AnnounceEvent,
}

impl Fact {
    /// Every fact, in the order of the enum.
    pub const ALL: [Fact; 12] = [
        Fact::Concept,
        Fact::ClassName,
        Fact::RandomNum,
        Fact::GameTime,
        Fact::AbilityName,
        Fact::TargetClassName,
        Fact::FirstBlood,
        Fact::Level,
        Fact::ItemName,
        Fact::PowerupType,
        Fact::LastHitCount,
        Fact::AnnounceEvent,
    ];

    /// The criterion key that reads it (keys are compared ignoring ASCII case).
    pub const fn key(self) -> &'static str {
        match self {
            Fact::Concept => "concept",
            Fact::ClassName => "classname",
            Fact::RandomNum => "randomnum",
            Fact::GameTime => "gametime",
            Fact::AbilityName => "abilityname",
            Fact::TargetClassName => "targetclassname",
            Fact::FirstBlood => "firstblood",
            Fact::Level => "level",
            Fact::ItemName => "itemname",
            Fact::PowerupType => "poweruptype",
            Fact::LastHitCount => "lasthitcount",
            Fact::AnnounceEvent => "announce_event",
        }
    }

    /// The fact a criterion key reads, if the client ever supplies it.
    pub fn of_key(key: &str) -> Option<Fact> {
        Fact::ALL
            .into_iter()
            .find(|fact| fact.key().eq_ignore_ascii_case(key))
    }
}

/// The facts every query carries, whatever its concept.
pub const ALWAYS: [Fact; 4] = [
    Fact::Concept,
    Fact::ClassName,
    Fact::RandomNum,
    Fact::GameTime,
];

impl Concept {
    /// Every concept, in the order of the enum.
    pub const ALL: [Concept; 16] = [
        Concept::Spawn,
        Concept::Move,
        Concept::Attack,
        Concept::CastExecute,
        Concept::Kill,
        Concept::LastHit,
        Concept::Deny,
        Concept::LevelUp,
        Concept::Purchase,
        Concept::Death,
        Concept::Respawn,
        Concept::Powerup,
        Concept::Bottling,
        Concept::Victory,
        Concept::Defeat,
        Concept::GameStart,
    ];

    /// Its name in Dota's rules, the value of its query's `concept` fact.
    pub const fn dota_name(self) -> &'static str {
        match self {
            Concept::Spawn => "TLK_DOTA_SPAWN",
            Concept::Move => "TLK_DOTA_MOVE",
            Concept::Attack => "TLK_DOTA_ATTACK",
            Concept::CastExecute => "TLK_DOTA_CAST_EXECUTE",
            Concept::Kill => "TLK_DOTA_KILL",
            Concept::LastHit => "TLK_DOTA_LAST_HIT",
            Concept::Deny => "TLK_DOTA_DENY",
            Concept::LevelUp => "TLK_DOTA_LEVELUP",
            Concept::Purchase => "TLK_DOTA_PURCHASE",
            Concept::Death => "TLK_DOTA_DEATH",
            Concept::Respawn => "TLK_DOTA_RESPAWN",
            Concept::Powerup => "TLK_DOTA_POWERUP",
            Concept::Bottling => "TLK_DOTA_BOTTLING",
            Concept::Victory => "TLK_DOTA_VICTORY",
            Concept::Defeat => "TLK_DOTA_DEFEAT",
            Concept::GameStart => "TLK_DOTA_CUSTOM",
        }
    }

    /// The facts its queries carry besides [`ALWAYS`].
    pub const fn facts(self) -> &'static [Fact] {
        match self {
            Concept::CastExecute => &[Fact::AbilityName],
            Concept::Kill => &[Fact::TargetClassName, Fact::FirstBlood],
            Concept::LastHit => &[Fact::LastHitCount],
            Concept::LevelUp => &[Fact::Level],
            Concept::Purchase => &[Fact::ItemName],
            Concept::Powerup => &[Fact::PowerupType],
            Concept::GameStart => &[Fact::AnnounceEvent],
            Concept::Spawn
            | Concept::Move
            | Concept::Attack
            | Concept::Deny
            | Concept::Death
            | Concept::Respawn
            | Concept::Bottling
            | Concept::Victory
            | Concept::Defeat => &[],
        }
    }

    /// Whether its queries carry `fact`.
    pub fn carries(self, fact: Fact) -> bool {
        ALWAYS.contains(&fact) || self.facts().contains(&fact)
    }

    /// The seconds after one of its lines before the hero says another of it: Dota's own,
    /// outside the rules data, the same on every hero's response page ("Moving: 6 seconds
    /// cooldown", "Attacking: 6 seconds cooldown", "Last hitting: 60 seconds cooldown",
    /// "Denying: ... 60 seconds cooldown"; https://liquipedia.net/dota2/Pudge/Responses,
    /// https://liquipedia.net/dota2/Shadow_Fiend/Responses). The rest have none beyond the
    /// lines' own `respeakdelay`.
    pub const fn cooldown_secs(self) -> f32 {
        match self {
            Concept::Move | Concept::Attack => 6.0,
            Concept::LastHit | Concept::Deny => 60.0,
            _ => 0.0,
        }
    }
}

/// The `poweruptype` of a rune (`DOTA_RUNE_BOUNTY`): Dota's rune names, its art's in capitals.
pub fn powerup_type(kind: bota_proto::RuneKind) -> String {
    format!(
        "DOTA_RUNE_{}",
        crate::catalog::rune_art(kind).to_ascii_uppercase()
    )
}

/// What a custom concept announces when the battle begins.
pub const GAME_START: &str = "game_start";

/// The facts of one query: the concept's and the values the client found for them. A fact the
/// concept does not carry is never set.
#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub concept: Concept,
    values: Vec<(Fact, String)>,
}

impl Query {
    /// A query of `concept` by the hero of class `class`, with its random number and the game
    /// time; the concept's own facts are added with [`Query::with`].
    pub fn new(concept: Concept, class: &str, random: u32, game_time: f32) -> Query {
        let mut values = vec![
            (Fact::Concept, concept.dota_name().to_owned()),
            (Fact::ClassName, class.to_owned()),
            (Fact::RandomNum, (random % 100).to_string()),
            (Fact::GameTime, format!("{game_time:.3}")),
        ];
        if concept == Concept::GameStart {
            values.push((Fact::AnnounceEvent, GAME_START.to_owned()));
        }
        Query { concept, values }
    }

    /// The query with `fact` set to `value`, when its concept carries the fact.
    pub fn with(mut self, fact: Fact, value: impl Into<String>) -> Query {
        if self.concept.carries(fact) {
            self.values.retain(|(f, _)| *f != fact);
            self.values.push((fact, value.into()));
        }
        self
    }

    /// The value of the fact a criterion key reads, if the query carries it.
    pub fn value(&self, key: &str) -> Option<&str> {
        let fact = Fact::of_key(key)?;
        let (_, value) = self.values.iter().find(|(f, _)| *f == fact)?;
        Some(value)
    }

    /// The facts set, in the order they were.
    pub fn values(&self) -> impl Iterator<Item = (Fact, &str)> {
        self.values
            .iter()
            .map(|(fact, value)| (*fact, value.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_carries_only_its_concepts_facts() {
        let query = Query::new(Concept::Move, "npc_dota_hero_pudge", 107, 12.0)
            .with(Fact::AbilityName, "pudge_rot");
        assert_eq!(query.value("Concept"), Some("TLK_DOTA_MOVE"));
        assert_eq!(query.value("randomnum"), Some("7"));
        assert_eq!(query.value("abilityname"), None);
        assert_eq!(query.value("customresponse"), None);
    }

    #[test]
    fn every_fact_is_read_by_its_own_key() {
        for fact in Fact::ALL {
            assert_eq!(Fact::of_key(&fact.key().to_ascii_uppercase()), Some(fact));
        }
    }
}
