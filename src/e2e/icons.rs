//! Icons drawn smaller than their art (reported: pixelated icons): averaged over a mip
//! chain, not sampled from their top level.

use bevy::image::{ImageFilterMode, ImageSampler};
use bevy::prelude::*;
use bota_proto::{Cheat, HeroId, ItemId};

use super::harness::{Client, Server};

/// An item in the bag: the icon its slot shows is the install's, and the image it draws has its
/// whole chain of levels and a sampler that blends them (it had one level, sampled at its top,
/// which skips texels: jagged, pixelated edges in a 70-pixel slot).
#[test]
fn an_item_icon_in_the_bag_is_drawn_from_its_mip_chain() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Item {
        item: ItemId::TANGO,
    });
    let icon = client
        .world()
        .resource::<crate::manifest::Manifest>()
        .item_icon(ItemId::TANGO.0)
        .expect("the install's tango icon in the cache");
    let shown = |c: &mut Client| {
        let world = c.world_mut();
        let server = world.resource::<AssetServer>().clone();
        let mut nodes = world.query::<(&ImageNode, &InheritedVisibility)>();
        let found = nodes
            .iter(world)
            .find(|(node, seen)| {
                seen.get()
                    && server
                        .get_path(node.image.id())
                        .is_some_and(|p| p.to_string() == icon)
            })
            .map(|(node, _)| node.image.clone());
        found.and_then(|handle| world.resource::<Assets<Image>>().get(&handle).cloned())
    };
    let patience = std::time::Duration::from_secs(30);
    client.within(patience, "the tango's icon shown and loaded", |c| {
        shown(c).is_some()
    });
    client.frames(2);
    let image = shown(&mut client).expect("shown");
    let size = image.texture_descriptor.size;
    let levels = 1 + size.width.max(size.height).ilog2();
    assert_eq!(
        image.texture_descriptor.mip_level_count, levels,
        "the whole chain"
    );
    let ImageSampler::Descriptor(sampler) = &image.sampler else {
        panic!("the default sampler, which samples one level");
    };
    assert_eq!(
        sampler.mipmap_filter,
        ImageFilterMode::Linear,
        "levels blended"
    );
    assert_eq!(sampler.min_filter, ImageFilterMode::Linear);
}
