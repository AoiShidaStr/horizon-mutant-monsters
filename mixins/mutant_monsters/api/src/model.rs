//! `MutantZombieModel.createBodyLayer()`, translated from Mutant Monsters
//! (AGPL-3.0, see NOTICE.md) onto Horizon's `MeshDefinition` (docs/native-mods.md,
//! "Mobs drawn by a mod"). Same boxes, texture offsets and initial poses
//! (`pose.rs` holds the poses, shared with the animation); the texture laid
//! over it is this port's own (`tools/make_textures.py`).
use crate::pose::{self, Part};
use horizon_client::mod_api::{CubeDeformation, CubeListBuilder, LayerDefinition, MeshDefinition, PartPose};

pub const LAYER: &str = "mutant_monsters:mutant_zombie#main";
pub const TEXTURE: &str = "mutant_monsters:entity/mutant_zombie";

fn pose_of(p: Part) -> PartPose {
    PartPose::offset_and_rotation(p.x, p.y, p.z, p.xr, p.yr, p.zr)
}

/// `mutant_monsters:mutant_zombie#main`, a 128x128 texture.
pub fn mutant_zombie() -> LayerDefinition {
    let mut mesh = MeshDefinition::new();
    let root = mesh.root();
    let grow = CubeDeformation::new(0.1);

    let pelvis = mesh.add_or_replace_child(root, "pelvis", CubeListBuilder::create(), pose_of(pose::PELVIS));
    let waist = mesh.add_or_replace_child(
        pelvis,
        "waist",
        CubeListBuilder::create().tex_offs(0, 44).add_box([-7.0, -16.0, -6.0], [14.0, 16.0, 12.0]),
        pose_of(pose::WAIST),
    );
    let chest = mesh.add_or_replace_child(
        waist,
        "chest",
        CubeListBuilder::create().tex_offs(0, 16).add_box([-12.0, -12.0, -8.0], [24.0, 12.0, 16.0]),
        pose_of(pose::CHEST),
    );
    mesh.add_or_replace_child(
        chest,
        "head",
        CubeListBuilder::create().tex_offs(0, 0).add_box([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0]),
        pose_of(pose::HEAD),
    );
    let right_arm = mesh.add_or_replace_child(
        chest,
        "right_arm",
        CubeListBuilder::create().tex_offs(104, 0).add_box([-3.0, 0.0, -3.0], [6.0, 16.0, 6.0]),
        pose_of(pose::RIGHT_ARM),
    );
    let left_arm = mesh.add_or_replace_child(
        chest,
        "left_arm",
        CubeListBuilder::create().tex_offs(104, 0).mirror().add_box([-3.0, 0.0, -3.0], [6.0, 16.0, 6.0]),
        pose_of(pose::LEFT_ARM),
    );
    mesh.add_or_replace_child(
        right_arm,
        "right_arm_lower",
        CubeListBuilder::create().tex_offs(104, 22).add_box_with([-3.0, 0.0, -3.0], [6.0, 16.0, 6.0], grow),
        pose_of(pose::ARM_LOWER),
    );
    mesh.add_or_replace_child(
        left_arm,
        "left_arm_lower",
        CubeListBuilder::create()
            .tex_offs(104, 22)
            .mirror()
            .add_box_with([-3.0, 0.0, -3.0], [6.0, 16.0, 6.0], grow),
        pose_of(pose::ARM_LOWER),
    );
    let right_leg = mesh.add_or_replace_child(
        pelvis,
        "right_leg",
        CubeListBuilder::create().tex_offs(80, 0).add_box([-3.0, 0.0, -3.0], [6.0, 11.0, 6.0]),
        pose_of(pose::RIGHT_LEG),
    );
    let left_leg = mesh.add_or_replace_child(
        pelvis,
        "left_leg",
        CubeListBuilder::create().tex_offs(80, 0).mirror().add_box([-3.0, 0.0, -3.0], [6.0, 11.0, 6.0]),
        pose_of(pose::LEFT_LEG),
    );
    mesh.add_or_replace_child(
        right_leg,
        "right_leg_lower",
        CubeListBuilder::create().tex_offs(80, 17).add_box_with([-3.0, 0.0, -3.0], [6.0, 8.0, 6.0], grow),
        pose_of(pose::LEG_LOWER),
    );
    mesh.add_or_replace_child(
        left_leg,
        "left_leg_lower",
        CubeListBuilder::create()
            .tex_offs(80, 17)
            .mirror()
            .add_box_with([-3.0, 0.0, -3.0], [6.0, 8.0, 6.0], grow),
        pose_of(pose::LEG_LOWER),
    );
    LayerDefinition::create(mesh, 128, 128)
}

/// The layers this mod registers.
pub fn layers() -> Vec<(String, LayerDefinition)> {
    vec![
        (LAYER.to_owned(), mutant_zombie()),
        (crate::creeper::LAYER.to_owned(), crate::creeper::layer()),
        (crate::skeleton::LAYER.to_owned(), crate::skeleton::layer()),
    ]
}
