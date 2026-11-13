//! Screen-independent text and node primitives used by shared widgets.
use crate::{theme, ui::Fonts};
use bevy::prelude::*;

pub(crate) struct Draw<'w, 's, 'a> {
    pub commands: Commands<'w, 's>,
    pub fonts: &'a Fonts,
}

impl Draw<'_, '_, '_> {
    pub fn node(&mut self, parent: Entity, node: Node) -> Entity {
        self.commands
            .spawn((node, Pickable::IGNORE, ChildOf(parent)))
            .id()
    }

    pub fn typed(
        &mut self,
        parent: Entity,
        words: impl Into<String>,
        role: theme::Type,
        color: Color,
    ) -> Entity {
        self.commands
            .spawn((
                theme::text(words, self.fonts, role, color),
                Pickable::IGNORE,
                ChildOf(parent),
            ))
            .id()
    }
}
