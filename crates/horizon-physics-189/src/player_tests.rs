use super::*;
use horizon_world_189::{ColumnData, ColumnUpdate, Section};

/// A loaded 3x3-column world with stone at y = 3 and `extra` placed after.
fn world(extra: &[([i32; 3], u16)]) -> World {
    let mut world = World::default();
    for cx in -1..=1 {
        for cz in -1..=1 {
            let mut sections = std::array::from_fn(|_| None);
            let mut states = vec![0u16; 4096];
            states[3 * 256..4 * 256].fill(1 << 4);
            sections[0] = Some(Section {
                states: states.into_boxed_slice().try_into().unwrap(),
                block_light: vec![0; 2048].into_boxed_slice().try_into().unwrap(),
                sky_light: Some(vec![0xff; 2048].into_boxed_slice().try_into().unwrap()),
            });
            world.apply(ColumnUpdate::Data {
                x: cx,
                z: cz,
                full: true,
                data: Box::new(ColumnData {
                    sections,
                    biomes: None,
                }),
            });
        }
    }
    for &([x, y, z], state) in extra {
        assert!(world.set_block(x, y, z, BlockState(state)));
    }
    world
}

/// A player that has fallen onto the floor: vanilla's resting state carries
/// one tick of gravity, which the next collision clears.
fn standing(world: &World, x: f64, y: f64, z: f64) -> Player {
    let mut player = Player::default();
    player.set_position(x, y, z);
    player.tick(world, Keys::default());
    assert!(!player.on_ground, "zero motion cannot collide vertically");
    player.tick(world, Keys::default());
    assert!(player.on_ground);
    assert_eq!(player.position[1], y);
    player
}

#[test]
fn standing_player_rests_exactly_on_the_block_top() {
    let world = world(&[]);
    let mut player = standing(&world, 0.5, 4.0, 0.5);
    for _ in 0..40 {
        player.tick(&world, Keys::default());
        assert!(player.on_ground);
        assert_eq!(player.position[1], 4.0);
    }
    assert_eq!(player.motion[0], 0.0);
}

#[test]
fn walking_and_sprinting_reach_vanilla_ground_speeds() {
    let world = world(&[]);
    let speed = |sprint: bool| {
        let mut player = standing(&world, 0.5, 4.0, 0.5);
        let keys = Keys {
            forward: true,
            sprint,
            ..Keys::default()
        };
        let mut last = player.position[2];
        let mut delta = 0.0;
        for _ in 0..40 {
            player.tick(&world, keys);
            delta = player.position[2] - last;
            last = player.position[2];
        }
        assert_eq!(player.sprinting, sprint);
        delta
    };
    // 4.317 and 5.612 blocks per second at twenty ticks per second.
    assert!((speed(false) - 0.215_86).abs() < 1.0e-4, "{}", speed(false));
    assert!((speed(true) - 0.280_62).abs() < 1.0e-4, "{}", speed(true));
}

#[test]
fn a_standing_jump_peaks_at_the_vanilla_apex() {
    let world = world(&[]);
    let mut player = standing(&world, 0.5, 4.0, 0.5);
    let jump = Keys {
        jump: true,
        ..Keys::default()
    };
    player.tick(&world, jump);
    assert_eq!(player.position[1], 4.0 + f64::from(0.42f32));
    let mut apex: f64 = 0.0;
    for _ in 0..20 {
        player.tick(&world, Keys::default());
        apex = apex.max(player.position[1] - 4.0);
    }
    // 1.8.9 zeroes |motion| < 0.005 (1.9 lowered it to 0.003, giving the
    // often-quoted 1.2522), so the 0.0030 rise at the apex never happens.
    assert_eq!(apex, 1.249_187_078_744_681);
    assert_eq!(player.position[1], 4.0);
    assert!(player.on_ground);
}

#[test]
fn walking_into_a_bottom_slab_steps_up_half_a_block() {
    // Stone slab (44:0) at z = 2 across the walking path.
    let world = world(&[([0, 4, 2], 44 << 4)]);
    let mut player = standing(&world, 0.5, 4.0, 0.5);
    let keys = Keys {
        forward: true,
        ..Keys::default()
    };
    // The step lands on the slab top within one tick, before reaching it.
    while player.position[2] < 2.5 {
        player.tick(&world, keys);
        if player.position[2] > 2.0 - f64::from(WIDTH / 2.0) {
            assert_eq!(player.position[1], 4.5);
        }
    }
    assert!(player.on_ground);
    assert!(!player.collided_horizontally);
}

#[test]
fn a_full_block_is_not_stepped_and_stops_horizontal_motion() {
    let world = world(&[([0, 4, 2], 1 << 4)]);
    let mut player = standing(&world, 0.5, 4.0, 0.5);
    let keys = Keys {
        forward: true,
        ..Keys::default()
    };
    for _ in 0..20 {
        player.tick(&world, keys);
    }
    assert_eq!(player.position[1], 4.0);
    assert_eq!(player.position[2], 2.0 - f64::from(WIDTH / 2.0));
    assert!(player.collided_horizontally);
}

#[test]
fn sneaking_stops_at_the_edge_of_a_drop() {
    // Remove the floor beyond z = 2 inside chunk (0, 0).
    let holes: Vec<([i32; 3], u16)> = (2..16)
        .flat_map(|z| (-16..16).map(move |x| ([x, 3, z], 0)))
        .collect();
    let world = world(&holes);
    let mut player = standing(&world, 0.5, 4.0, 0.5);
    let keys = Keys {
        forward: true,
        sneak: true,
        ..Keys::default()
    };
    for _ in 0..80 {
        player.tick(&world, keys);
    }
    assert_eq!(player.position[1], 4.0);
    assert!(player.position[2] < 2.0 + f64::from(WIDTH / 2.0));
    assert!(player.position[2] > 2.0);
}

#[test]
fn speed_modifiers_multiply_in_hash_set_order() {
    let mut speed = Attribute::new(0.10000000149011612);
    speed.apply(Modifier {
        uuid: SPRINT_UUID,
        amount: 0.30000001192092896,
        operation: 2,
    });
    assert_eq!(
        speed.value(),
        0.10000000149011612 * (1.0 + 0.30000001192092896)
    );
    speed.apply(Modifier {
        uuid: SPEED_UUID,
        amount: 0.400_000_005_960_464_5,
        operation: 2,
    });
    let (first, second) = if bucket(SPRINT_UUID) <= bucket(SPEED_UUID) {
        (0.30000001192092896, 0.400_000_005_960_464_5)
    } else {
        (0.400_000_005_960_464_5, 0.30000001192092896)
    };
    assert_eq!(
        speed.value(),
        0.10000000149011612 * (1.0 + first) * (1.0 + second)
    );
    speed.remove(SPRINT_UUID);
    assert!(!speed.has(SPRINT_UUID));
}

#[test]
fn server_teleport_keeps_relative_axes_and_clears_absolute_motion() {
    let world = world(&[]);
    let mut player = standing(&world, 0.5, 4.0, 0.5);
    player.motion = [0.1, 0.2, 0.3];
    player.yaw = 10.0;
    player.teleport([1.0, 0.0, 2.0], 370.0, 5.0, 0b0_1010);
    assert_eq!(player.position, [1.0, 4.0, 2.0]);
    assert_eq!(player.motion, [0.0, 0.2, 0.0]);
    assert_eq!(player.yaw, 20.0);
    assert_eq!(player.pitch, 5.0);
}

#[test]
fn double_tap_jump_flies_and_landing_ends_survival_flight() {
    let world = world(&[]);
    let mut player = standing(&world, 0.5, 4.0, 0.5);
    player.allow_flying = true;
    let jump = Keys {
        jump: true,
        ..Keys::default()
    };
    player.tick(&world, jump);
    player.tick(&world, Keys::default());
    player.tick(&world, jump);
    assert!(player.flying && player.abilities_changed);
    player.abilities_changed = false;
    let before = player.position[1];
    for _ in 0..10 {
        player.tick(&world, jump);
    }
    assert!(player.position[1] > before + 1.0);
    let descend = Keys {
        sneak: true,
        ..Keys::default()
    };
    for _ in 0..80 {
        player.tick(&world, descend);
    }
    assert!(!player.flying && player.abilities_changed);
    assert_eq!(player.position[1], 4.0);
}

#[test]
fn depth_strider_raises_water_speed_toward_land_speed() {
    // Still water filling y = 4..5 over the floor.
    let water: Vec<([i32; 3], u16)> = (-16..16)
        .flat_map(|x| (-16..16).map(move |z| ([x, 4, z], 9 << 4)))
        .collect();
    let world = world(&water);
    let swim = |level: i32| {
        let mut player = Player::default();
        player.set_position(0.5, 4.0, 0.5);
        player.depth_strider = level;
        let keys = Keys {
            forward: true,
            ..Keys::default()
        };
        for _ in 0..40 {
            player.tick(&world, keys);
        }
        assert!(player.in_water);
        player.position[2]
    };
    let (plain, strider) = (swim(0), swim(3));
    assert!(strider > plain * 2.0, "{plain} {strider}");
}

#[test]
fn spectator_flight_crosses_solid_blocks_and_stays_flying() {
    let world = world(&[([0, 4, 1], 16), ([0, 5, 1], 16)]);
    let mut player = Player::default();
    player.set_position(0.5, 4.0, 0.5);
    player.no_clip = true;
    player.allow_flying = true;
    for _ in 0..20 {
        player.tick(
            &world,
            Keys {
                forward: true,
                ..Keys::default()
            },
        );
    }
    assert!(player.position[2] > 2.0);
    assert!(player.flying && !player.on_ground);
    player.tick(
        &world,
        Keys {
            jump: true,
            ..Keys::default()
        },
    );
    player.tick(&world, Keys::default());
    player.tick(
        &world,
        Keys {
            jump: true,
            ..Keys::default()
        },
    );
    assert!(player.flying);
}

#[test]
fn death_shrinks_the_box_and_starts_a_hop_and_drift() {
    let floor = world(&[]);
    let mut player = standing(&floor, 0.5, 4.0, 0.5);
    player.yaw = 90.0;
    player.on_death();
    assert!((lo(player.bounds)[0] - 0.4).abs() < 1e-6);
    assert!((hi(player.bounds)[1] - 4.2).abs() < 1e-6);
    assert_eq!(player.motion[1], 0.10000000149011612);
    assert!(player.motion[0].abs() < 1e-6);
    assert!((player.motion[2] + 0.1).abs() < 1e-6);
    // Next to a hole, a 0.6 player is still held by the edge; the 0.2 body
    // is not.
    let gap = world(&[([1, 3, 0], 0)]);
    let mut alive = standing(&gap, 1.15, 4.0, 0.5);
    let mut body = standing(&gap, 1.15, 4.0, 0.5);
    body.on_death();
    body.motion = [0.0; 3];
    for _ in 0..10 {
        alive.tick(&gap, Keys::default());
        body.tick(&gap, Keys::default());
    }
    assert_eq!(alive.position[1], 4.0);
    assert!(body.position[1] < 4.0, "{:?}", body.position);
}
