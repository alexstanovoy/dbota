//! One `fx` line of the manifest: the particle and sound Dota shows with a fact of the game, keyed
//! by Dota's name for it. The grammar is in the manifest's header.

use bevy::prelude::Vec3;
use bota_proto::Team;

/// When a bound effect plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// An item was used (`EventKind::ItemUsed`).
    Use,
    /// An ability took effect (`EventKind::AbilityCast`).
    Cast,
    /// Something thrown came down (`EventKind::Landed`).
    Land,
    /// A unit took a blow: of the ability or item named, or of the attacks of the unit named
    /// (`EventKind::Damaged`).
    Hit,
    /// A unit of the name died (`EventKind::Died`).
    Died,
    /// A rune of the kind appeared (`EventKind::RuneSpawned`).
    Spawn,
    /// A rune of the kind took effect on a hero, at once or out of a Bottle
    /// (`EventKind::RuneActivated`).
    Activate,
    /// A rune was stored in the item (`EventKind::RuneBottled`: the Bottle).
    Store,
    /// A hero plucked a lotus from the unit (`EventKind::LotusPlucked`: a Lotus Pool).
    Pluck,
    /// The unit was taken over by a side (`EventKind::Captured`: an Outpost).
    Captured,
    /// A unit came out of the unit after a warp through its twin (`EventKind::GateWarped`: a Twin
    /// Gate).
    Warp,
    /// While the subject holds: a modifier on a unit, an effect an ability or an item put on one,
    /// or a unit of a Dota name standing.
    Worn,
    /// While a unit carries the item in an active slot.
    Held,
    /// While a unit is in the cast point of the ability.
    Casting,
    /// While a hero channels at the unit the ability that works it (an Outpost captured, a
    /// Watcher lit).
    Channelled,
    /// While a side runs the unit's countdown by standing near it (a Shrine of Wisdom gathered, a
    /// lotus plucked from a Lotus Pool), forward or back.
    Gathering,
    /// While a missile of the ability or item flies.
    Flies,
}

/// Where a bound effect plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum At {
    /// The user, where it stands after the event.
    Caster,
    /// The user, where it stood in the view before.
    CasterBefore,
    /// The spot the event names: what a use was aimed at, where a throw came down.
    Point,
    /// The unit the fact is about: the wearer (or the rune that lies), the one that took the blow,
    /// the one that died.
    Unit,
    /// Each missile on its own, from where its source launches it to where it is.
    Flight,
    /// All the missiles one source has flying, from the source to the one farthest from it: a
    /// hook's chain.
    Chain,
}

/// Which point of a unit an effect is placed at.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Attach {
    /// Its feet.
    #[default]
    Origin,
    /// Over its head, where its health bar floats.
    Overhead,
    /// A node of its model: `attach_hitloc` where its body is hit, `attach_eyeL`, `attach_head`.
    Node(String),
}

/// What a control point of a bound particle is given.
#[derive(Clone, Debug, PartialEq)]
pub enum Feed {
    /// How many are held of the effect worn, in x.
    Stacks,
    /// Seconds the effect worn has left, in x.
    Seconds,
    /// The attribute the item concerned is set to, 0 for strength, 1 for agility, 2 for
    /// intelligence, in x.
    Mode,
    /// The share of its charges the item concerned held, in x.
    Charges,
    /// A vector the line gives.
    Vector(Vec3),
    /// A point of the unit the fact is about, followed.
    Place(Attach),
    /// Where the user stands: the hero that plucked a lotus.
    Caster,
    /// The side the unit is held for, in x: 0 the Radiant, 1 the Dire (`units::held_for`).
    Side,
    /// How far the unit sees now, or sees for the side it is lit for, in x.
    Sight,
    /// The share of the unit's countdown run (0 to 1) times the first vector, plus the second.
    Gathered(Vec3, Vec3),
    /// The worn unit's imported render-bounds radius, in x.
    ModelRadius,
}

/// Whose effect a worn line is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Whose {
    /// Any.
    Any,
    /// One its wearer put on itself (an aura's bearer).
    Own,
    /// One another unit put on its wearer.
    Foreign,
}

/// One `fx` line.
#[derive(Clone, Debug, PartialEq)]
pub struct Fx {
    /// `item.blink`, `ability.pudge_rot`, `modifier.modifier_item_blood_grenade_debuff`,
    /// `unit.npc_dota_observer_wards`.
    pub subject: String,
    pub trigger: Trigger,
    pub at: At,
    /// The point of the unit it is placed at.
    pub attach: Attach,
    /// The logical name of the particle, when the line has one.
    pub particle: Option<String>,
    /// The logical prefix of the sounds, one of them played, when the line has one.
    pub sound: Option<String>,
    /// What the particle's control points are given, by control point (not the first).
    pub feeds: Vec<(usize, Feed)>,
    /// Whose effect a worn line is about.
    pub whose: Whose,
    /// The stacks a worn effect must hold, from and up to.
    pub stacks: (u32, u32),
    /// The side a worn line's unit must be held for, when the line names one.
    pub side: Option<Team>,
    /// The side whose look the wearer must take on (its own side's, or the one the map gives a
    /// neutral building), for a worn line that asks.
    pub look: Option<Team>,
}

impl Fx {
    /// The kind and name of the subject: `("item", "blink")`.
    pub fn subject_parts(&self) -> (&str, &str) {
        self.subject.split_once('.').unwrap_or(("", &self.subject))
    }

    /// Reads the words after `fx`: `<subject>.<trigger> at=… [attach=…] [particle=…] [sound=…]
    /// [cp<N>=…] [whose=…] [stacks=…] [look=…]`.
    pub fn parse(words: &[&str]) -> Result<Fx, String> {
        let [name, rest @ ..] = words else {
            return Err("no name".to_owned());
        };
        let (subject, trigger) = name.rsplit_once('.').ok_or("no trigger")?;
        let trigger = match trigger {
            "use" => Trigger::Use,
            "cast" => Trigger::Cast,
            "land" => Trigger::Land,
            "hit" => Trigger::Hit,
            "died" => Trigger::Died,
            "spawn" => Trigger::Spawn,
            "activate" => Trigger::Activate,
            "store" => Trigger::Store,
            "pluck" => Trigger::Pluck,
            "captured" => Trigger::Captured,
            "warp" => Trigger::Warp,
            "worn" => Trigger::Worn,
            "held" => Trigger::Held,
            "casting" => Trigger::Casting,
            "channelled" => Trigger::Channelled,
            "gathering" => Trigger::Gathering,
            "flies" => Trigger::Flies,
            other => return Err(format!("no trigger '{other}'")),
        };
        let mut fx = Fx {
            subject: subject.to_owned(),
            trigger,
            at: At::Caster,
            attach: Attach::Origin,
            particle: None,
            sound: None,
            feeds: Vec::new(),
            whose: Whose::Any,
            stacks: (0, u32::MAX),
            side: None,
            look: None,
        };
        let mut at = None;
        for word in rest {
            let (key, value) = word
                .split_once('=')
                .ok_or_else(|| format!("cannot read '{word}'"))?;
            match key {
                "at" => at = Some(at_of(value)?),
                "attach" => fx.attach = attach_of(value)?,
                "particle" => fx.particle = Some(value.to_owned()),
                "sound" => fx.sound = Some(value.to_owned()),
                "whose" => fx.whose = whose_of(value)?,
                "stacks" => fx.stacks = stacks_of(value)?,
                "side" => fx.side = Some(side_of(value)?),
                "look" => fx.look = Some(side_of(value)?),
                _ => match key.strip_prefix("cp").and_then(|n| n.parse::<usize>().ok()) {
                    Some(point) if point > 0 => fx.feeds.push((point, feed_of(value)?)),
                    _ => return Err(format!("cannot read '{word}'")),
                },
            }
        }
        fx.at = at.ok_or("no at=")?;
        fx.check()?;
        Ok(fx)
    }

    /// Whether the line makes sense: it plays something, at a place its trigger can name, and
    /// asks of the fact only what its trigger knows.
    fn check(&self) -> Result<(), String> {
        if self.particle.is_none() && self.sound.is_none() {
            return Err("neither a particle nor a sound".to_owned());
        }
        let places: &[At] = match self.trigger {
            Trigger::Use => &[At::Caster, At::CasterBefore, At::Point],
            Trigger::Cast => &[At::Caster, At::CasterBefore],
            Trigger::Land => &[At::Caster, At::Point],
            Trigger::Hit => &[At::Unit, At::Caster],
            Trigger::Spawn => &[At::Point],
            Trigger::Activate | Trigger::Store => &[At::Caster],
            Trigger::Pluck => &[At::Unit, At::Caster],
            Trigger::Died
            | Trigger::Captured
            | Trigger::Worn
            | Trigger::Held
            | Trigger::Casting
            | Trigger::Channelled
            | Trigger::Gathering => &[At::Unit],
            Trigger::Warp => &[At::Unit, At::Caster, At::Point],
            Trigger::Flies => &[At::Flight, At::Chain],
        };
        if !places.contains(&self.at) {
            return Err(format!(
                "a {:?} line cannot be at {:?}",
                self.trigger, self.at
            ));
        }
        let worn = self.trigger == Trigger::Worn;
        if !worn
            && self.feeds.iter().any(|(_, feed)| matches!(feed, Feed::ModelRadius))
        {
            return Err("model_radius requires a worn line".to_owned());
        }
        if !worn
            && (self.whose != Whose::Any || self.stacks != (0, u32::MAX) || self.look.is_some())
        {
            return Err("only a worn line asks whose, how many or whose look".to_owned());
        }
        if !worn && self.side.is_some() {
            return Err("only a worn line asks which side holds the unit".to_owned());
        }
        if !matches!(self.at, At::Unit | At::Caster | At::CasterBefore)
            && self.attach != Attach::Origin
        {
            return Err("only an effect at a unit is attached to it".to_owned());
        }
        if self.trigger == Trigger::Flies && self.sound.is_some() {
            return Err("a flight plays no sound".to_owned());
        }
        Ok(())
    }
}

fn at_of(value: &str) -> Result<At, String> {
    Ok(match value {
        "caster" => At::Caster,
        "caster_before" => At::CasterBefore,
        "point" => At::Point,
        "unit" => At::Unit,
        "flight" => At::Flight,
        "chain" => At::Chain,
        other => return Err(format!("no place '{other}'")),
    })
}

fn attach_of(value: &str) -> Result<Attach, String> {
    Ok(match value {
        "origin" => Attach::Origin,
        "overhead" => Attach::Overhead,
        "hitloc" => Attach::Node("attach_hitloc".to_owned()),
        node if node.starts_with("attach_") => Attach::Node(node.to_owned()),
        other => return Err(format!("no attach point '{other}'")),
    })
}

fn side_of(value: &str) -> Result<Team, String> {
    Ok(match value {
        "radiant" => Team::Radiant,
        "dire" => Team::Dire,
        other => return Err(format!("no side '{other}'")),
    })
}

fn whose_of(value: &str) -> Result<Whose, String> {
    Ok(match value {
        "own" => Whose::Own,
        "foreign" => Whose::Foreign,
        other => return Err(format!("no owner '{other}'")),
    })
}

/// `3`, `3..`, `..5` or `3..5`: from and up to, both counted.
fn stacks_of(value: &str) -> Result<(u32, u32), String> {
    let number = |text: &str, default: u32| {
        if text.is_empty() {
            Ok(default)
        } else {
            text.parse().map_err(|_| format!("no count '{text}'"))
        }
    };
    match value.split_once("..") {
        Some((from, up_to)) => Ok((number(from, 0)?, number(up_to, u32::MAX)?)),
        None => number(value, 0).map(|n| (n, n)),
    }
}

fn feed_of(value: &str) -> Result<Feed, String> {
    Ok(match value {
        "stacks" => Feed::Stacks,
        "seconds" => Feed::Seconds,
        "mode" => Feed::Mode,
        "charges" => Feed::Charges,
        "caster" => Feed::Caster,
        "side" => Feed::Side,
        "sight" => Feed::Sight,
        "model_radius" => Feed::ModelRadius,
        place if attach_of(place).is_ok() => Feed::Place(attach_of(place)?),
        _ => match value.strip_prefix("gathered:") {
            Some(vectors) => {
                let (share, plus) = vectors.split_once('+').unwrap_or((vectors, "0,0,0"));
                Feed::Gathered(vector_of(share)?, vector_of(plus)?)
            }
            None => Feed::Vector(vector_of(value)?),
        },
    })
}

/// `x,y,z`.
fn vector_of(value: &str) -> Result<Vec3, String> {
    let parts: Vec<f32> = value
        .split(',')
        .map(|n| n.parse().map_err(|_| format!("no feed '{value}'")))
        .collect::<Result<_, _>>()?;
    let [x, y, z] = parts[..] else {
        return Err(format!("no feed '{value}'"));
    };
    Ok(Vec3::new(x, y, z))
}
