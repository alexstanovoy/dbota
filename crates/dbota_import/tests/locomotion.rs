//! Locomotion-only export leaves the building activities unchanged.
use dbota_import::catalog::{Line, parse};
use heatel::{
    DirSource, Resource, Vpk,
    anim::{read_clips, read_clips_in_place},
    find_game_dir,
};

#[test]
#[ignore = "needs a local Dota 2 install"]
fn every_listed_building_clip_keeps_its_pose_with_locomotion_export() {
    let catalogue = parse(
        include_str!("../../../assets-manifest/manifest.txt"),
        include_str!("../../../assets-manifest/ids.txt"),
    )
    .expect("manifest");
    let source = DirSource::new(find_game_dir(None).expect("install"));
    let pak = Vpk::open(&source, "pak01_dir.vpk").expect("VPK");
    let mut checked = 0;
    for line in &catalogue.lines {
        let Line::Model(model) = line else {
            continue;
        };
        if !["tower.", "ancient.", "barracks.", "filler.", "objective."]
            .iter()
            .any(|prefix| model.logical.starts_with(prefix))
        {
            continue;
        }
        let bytes = pak.read_file(&source, &model.src).expect("building model");
        let resource = Resource::parse(&bytes).expect("resource");
        let names: Vec<_> = model.anims.iter().map(String::as_str).collect();
        let raw = read_clips(&resource, &names).expect("original poses");
        let placed = read_clips_in_place(&resource, &names, &["ACT_DOTA_RUN", "ACT_DOTA_WALK"])
            .expect("in-place request");
        assert_eq!(raw.len(), placed.len());
        for (a, b) in raw.iter().zip(&placed) {
            checked += 1;
            assert_eq!(a.name, b.name);
            for (a, b) in a.tracks.iter().zip(&b.tracks) {
                assert_eq!(a.translation.times, b.translation.times);
                assert_eq!(a.translation.values, b.translation.values);
                assert_eq!(a.rotation.times, b.rotation.times);
                assert_eq!(a.rotation.values, b.rotation.values);
                assert_eq!(a.scale.times, b.scale.times);
                assert_eq!(a.scale.values, b.scale.values);
            }
        }
    }
    assert!(checked > 20, "building clips checked: {checked}");
    eprintln!("building clips unchanged: {checked}");
}
