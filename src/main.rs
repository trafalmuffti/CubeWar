//! CubeWar: a first-person space shooter demo built with Bevy.
//!
//! You sit in the cockpit of a starfighter and shoot at Borg-inspired cube
//! ships. Each cube takes four hits:
//!   1. shields come up and glow blue
//!   2. shields glow violet
//!   3. shields collapse, the glow disappears
//!   4. the cube explodes and a new one spawns off screen

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

// ---------------------------------------------------------------------------
// Tuning constants
// ---------------------------------------------------------------------------

const CUBE_SIZE: f32 = 10.0;
const CUBE_HIT_RADIUS: f32 = CUBE_SIZE * 0.72;
const CUBE_SPAWN_DISTANCE: f32 = 260.0;
const CUBE_APPROACH_SPEED: f32 = 45.0;
const CUBE_HOLD_DISTANCE: f32 = 70.0;
const CUBE_RESPAWN_DELAY: f32 = 1.4;

const LASER_SPEED: f32 = 320.0;
const LASER_LIFETIME: f32 = 2.5;
const FIRE_COOLDOWN: f32 = 0.18;

const MOUSE_SENSITIVITY: f32 = 0.0022;
const STAR_COUNT: usize = 700;

// ---------------------------------------------------------------------------
// Components & resources
// ---------------------------------------------------------------------------

/// The player's camera. Yaw/pitch are stored separately so the view can never roll.
#[derive(Component)]
struct Player {
    yaw: f32,
    pitch: f32,
    fire_cooldown: Timer,
}

/// The single enemy cube.
#[derive(Component)]
struct Cube {
    hits: u8,
    /// Where the cube drifts to before holding position.
    target: Vec3,
    spin_axis: Vec3,
    spin_speed: f32,
    age: f32,
}

/// The translucent shield shell that is a child of the cube.
#[derive(Component)]
struct Shield;

/// Fades out the shield's emissive after a hit "flash".
#[derive(Component)]
struct ShieldFlash(f32);

#[derive(Component)]
struct Laser {
    velocity: Vec3,
    life: Timer,
}

#[derive(Component)]
struct ExplosionCore {
    life: Timer,
    /// Peak scale of the fireball.
    size: f32,
}

#[derive(Component)]
struct Debris {
    velocity: Vec3,
    spin: Vec3,
    life: Timer,
}

#[derive(Component)]
struct HudText;

#[derive(Resource, Default)]
struct Score {
    destroyed: u32,
}

/// Counts down after a cube is destroyed before the next one appears.
#[derive(Resource)]
struct RespawnTimer(Option<Timer>);

/// Handles to reusable meshes and materials created once at startup.
#[derive(Resource)]
struct Assets3d {
    laser_mesh: Handle<Mesh>,
    laser_material: Handle<StandardMaterial>,
    hull_material: Handle<StandardMaterial>,
    greeble_dark: Handle<StandardMaterial>,
    greeble_glow: Handle<StandardMaterial>,
    shield_material: Handle<StandardMaterial>,
    cube_mesh: Handle<Mesh>,
    shield_mesh: Handle<Mesh>,
    greeble_meshes: Vec<Handle<Mesh>>,
    debris_mesh: Handle<Mesh>,
    debris_material: Handle<StandardMaterial>,
    core_mesh: Handle<Mesh>,
    core_material: Handle<StandardMaterial>,
}

/// Tiny deterministic xorshift RNG so the demo has no extra dependencies.
#[derive(Resource)]
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    fn unit_vector(&mut self) -> Vec3 {
        let z = self.range(-1.0, 1.0);
        let a = self.range(0.0, TAU);
        let r = (1.0 - z * z).sqrt();
        Vec3::new(r * a.cos(), r * a.sin(), z)
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "CubeWar".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.004, 0.004, 0.012)))
        .insert_resource(Score::default())
        .insert_resource(RespawnTimer(None))
        .insert_resource(Rng(0x9E37_79B9_7F4A_7C15))
        .add_systems(Startup, (setup, grab_cursor))
        .add_systems(
            Update,
            (
                toggle_cursor,
                mouse_look,
                fire_laser,
                move_lasers,
                move_cube,
                laser_hits,
                shield_flash,
                update_explosions,
                respawn_cube,
                update_hud,
            )
                .chain(),
        )
        .run();
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rng: ResMut<Rng>,
) {
    // --- Shared assets --------------------------------------------------
    let assets = Assets3d {
        laser_mesh: meshes.add(Capsule3d::new(0.12, 6.0)),
        laser_material: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.25, 0.15),
            emissive: LinearRgba::rgb(40.0, 4.0, 1.5),
            unlit: true,
            ..default()
        }),
        hull_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.16, 0.18, 0.17),
            perceptual_roughness: 0.75,
            metallic: 0.6,
            ..default()
        }),
        greeble_dark: materials.add(StandardMaterial {
            base_color: Color::srgb(0.08, 0.09, 0.09),
            perceptual_roughness: 0.55,
            metallic: 0.85,
            ..default()
        }),
        greeble_glow: materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 1.0, 0.3),
            emissive: LinearRgba::rgb(0.3, 6.0, 0.6),
            ..default()
        }),
        shield_material: materials.add(shield_material_for(0)),
        cube_mesh: meshes.add(Cuboid::from_length(CUBE_SIZE)),
        shield_mesh: meshes.add(Cuboid::from_length(CUBE_SIZE * 1.18)),
        greeble_meshes: vec![
            meshes.add(Cuboid::new(1.6, 0.5, 1.6)),
            meshes.add(Cuboid::new(0.7, 0.9, 2.4)),
            meshes.add(Cuboid::new(2.6, 0.4, 0.7)),
            meshes.add(Cuboid::new(0.5, 0.5, 0.5)),
            meshes.add(Cylinder::new(0.4, 0.8)),
        ],
        debris_mesh: meshes.add(Cuboid::from_length(1.0)),
        debris_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.3, 0.28),
            emissive: LinearRgba::rgb(6.0, 2.2, 0.4),
            perceptual_roughness: 0.9,
            ..default()
        }),
        core_mesh: meshes.add(Sphere::new(1.0)),
        core_material: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.6, 0.2, 0.9),
            emissive: LinearRgba::rgb(30.0, 12.0, 3.0),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        }),
    };

    // --- Camera / cockpit ----------------------------------------------
    let cockpit_dark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.05, 0.06),
        perceptual_roughness: 0.8,
        metallic: 0.3,
        ..default()
    });
    let cockpit_glow = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.7, 1.0),
        emissive: LinearRgba::rgb(0.4, 1.6, 3.0),
        ..default()
    });
    let strut = meshes.add(Cuboid::new(0.06, 0.06, 1.0));
    let panel = meshes.add(Cuboid::new(1.0, 0.16, 0.6));
    let light_strip = meshes.add(Cuboid::new(0.5, 0.012, 0.012));

    commands
        .spawn((
            Camera3d::default(),
            Camera {
                hdr: true,
                ..default()
            },
            Bloom::NATURAL,
            Projection::from(PerspectiveProjection {
                fov: 75f32.to_radians(),
                ..default()
            }),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Player {
                yaw: 0.0,
                pitch: 0.0,
                fire_cooldown: Timer::from_seconds(FIRE_COOLDOWN, TimerMode::Once),
            },
        ))
        .with_children(|cam| {
            // Canopy frame: two struts sweeping forward from the corners.
            for x in [-0.9, 0.9] {
                cam.spawn((
                    Mesh3d(strut.clone()),
                    MeshMaterial3d(cockpit_dark.clone()),
                    Transform::from_xyz(x, 0.45, -1.4)
                        .with_rotation(Quat::from_rotation_y(x.signum() * 0.35))
                        .with_scale(Vec3::new(1.0, 1.0, 1.6)),
                ));
            }
            // Dashboard below the view.
            cam.spawn((
                Mesh3d(panel.clone()),
                MeshMaterial3d(cockpit_dark.clone()),
                Transform::from_xyz(0.0, -0.62, -1.1)
                    .with_rotation(Quat::from_rotation_x(0.35))
                    .with_scale(Vec3::new(2.6, 1.0, 1.0)),
            ));
            for x in [-0.6, 0.0, 0.6] {
                cam.spawn((
                    Mesh3d(light_strip.clone()),
                    MeshMaterial3d(cockpit_glow.clone()),
                    Transform::from_xyz(x, -0.53, -0.95).with_rotation(Quat::from_rotation_x(0.35)),
                ));
            }
        });

    // --- Lighting -------------------------------------------------------
    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(60.0, 100.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 1_500.0,
            color: Color::srgb(0.5, 0.6, 1.0),
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(-80.0, -30.0, -60.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // --- Starfield ------------------------------------------------------
    let star_mesh = meshes.add(Sphere::new(1.0));
    let star_mats = [
        materials.add(star_material(LinearRgba::rgb(4.0, 4.0, 4.5))),
        materials.add(star_material(LinearRgba::rgb(4.0, 3.2, 2.2))),
        materials.add(star_material(LinearRgba::rgb(2.6, 3.2, 5.0))),
    ];
    for i in 0..STAR_COUNT {
        let dir = rng.unit_vector();
        let size = rng.range(0.9, 2.6);
        commands.spawn((
            Mesh3d(star_mesh.clone()),
            MeshMaterial3d(star_mats[i % star_mats.len()].clone()),
            Transform::from_translation(dir * 900.0).with_scale(Vec3::splat(size)),
        ));
    }

    // --- HUD ------------------------------------------------------------
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: 22.0,
            ..default()
        },
        TextColor(Color::srgb(0.6, 0.9, 1.0)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(14.0),
            left: Val::Px(16.0),
            ..default()
        },
        HudText,
    ));
    commands.spawn((
        Text::new("Mouse: aim   LMB / Space: fire   Esc: release cursor"),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgba(0.7, 0.8, 0.9, 0.7)),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(14.0),
            left: Val::Px(16.0),
            ..default()
        },
    ));
    spawn_crosshair(&mut commands);

    // --- First enemy ----------------------------------------------------
    spawn_cube(&mut commands, &assets, &mut rng, Vec3::NEG_Z);
    commands.insert_resource(assets);
}

fn star_material(emissive: LinearRgba) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        emissive,
        unlit: true,
        ..default()
    }
}

fn spawn_crosshair(commands: &mut Commands) {
    let color = Color::srgba(0.6, 1.0, 0.7, 0.85);
    let bar = |w: f32, h: f32, left: f32, top: f32| {
        (
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(w),
                height: Val::Px(h),
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                margin: UiRect {
                    left: Val::Px(left),
                    top: Val::Px(top),
                    ..default()
                },
                ..default()
            },
            BackgroundColor(color),
        )
    };
    commands.spawn(bar(14.0, 2.0, -22.0, -1.0));
    commands.spawn(bar(14.0, 2.0, 8.0, -1.0));
    commands.spawn(bar(2.0, 14.0, -1.0, -22.0));
    commands.spawn(bar(2.0, 14.0, -1.0, 8.0));
    commands.spawn(bar(3.0, 3.0, -1.5, -1.5));
}

/// Emissive colour of the shield shell for a given hit count.
fn shield_material_for(hits: u8) -> StandardMaterial {
    let (base, emissive) = match hits {
        1 => (Color::srgba(0.2, 0.5, 1.0, 0.28), LinearRgba::rgb(0.6, 2.4, 9.0)),
        2 => (Color::srgba(0.7, 0.3, 1.0, 0.32), LinearRgba::rgb(5.0, 1.2, 9.5)),
        _ => (Color::srgba(0.0, 0.0, 0.0, 0.0), LinearRgba::BLACK),
    };
    StandardMaterial {
        base_color: base,
        emissive,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    }
}

// ---------------------------------------------------------------------------
// Cube spawning
// ---------------------------------------------------------------------------

/// Spawns a cube well outside the player's field of view. `forward` is the
/// direction the camera currently faces; the cube starts at least ~100° away
/// from it and drifts to a holding position roughly in front of the player.
fn spawn_cube(commands: &mut Commands, assets: &Assets3d, rng: &mut Rng, forward: Vec3) {
    let forward = forward.normalize_or(Vec3::NEG_Z);

    // Pick a random direction until it's comfortably off screen (75° FOV).
    let mut dir = rng.unit_vector();
    for _ in 0..64 {
        if dir.dot(forward) < -0.15 {
            break;
        }
        dir = rng.unit_vector();
    }
    let start = dir * CUBE_SPAWN_DISTANCE;

    // Hold somewhere in the general direction the player is looking, offset a bit.
    let side = forward.cross(Vec3::Y).normalize_or(Vec3::X);
    let up = side.cross(forward).normalize_or(Vec3::Y);
    let target = forward * CUBE_HOLD_DISTANCE
        + side * rng.range(-25.0, 25.0)
        + up * rng.range(-14.0, 14.0);

    let rotation = Quat::from_euler(
        EulerRot::XYZ,
        rng.range(0.0, TAU),
        rng.range(0.0, TAU),
        rng.range(0.0, TAU),
    );

    commands
        .spawn((
            Mesh3d(assets.cube_mesh.clone()),
            MeshMaterial3d(assets.hull_material.clone()),
            Transform::from_translation(start).with_rotation(rotation),
            Cube {
                hits: 0,
                target,
                spin_axis: rng.unit_vector(),
                spin_speed: rng.range(0.15, 0.4),
                age: 0.0,
            },
        ))
        .with_children(|cube| {
            add_greebles(cube, assets, rng);
            cube.spawn((
                Mesh3d(assets.shield_mesh.clone()),
                MeshMaterial3d(assets.shield_material.clone()),
                Transform::IDENTITY,
                Visibility::Hidden,
                Shield,
            ));
        });
}

/// Covers all six faces of the cube in Borg-style surface clutter.
fn add_greebles(cube: &mut ChildSpawnerCommands, assets: &Assets3d, rng: &mut Rng) {
    let half = CUBE_SIZE / 2.0;
    let faces: [(Vec3, Vec3, Vec3); 6] = [
        (Vec3::X, Vec3::Y, Vec3::Z),
        (Vec3::NEG_X, Vec3::Y, Vec3::Z),
        (Vec3::Y, Vec3::X, Vec3::Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::X, Vec3::Y),
    ];
    for (normal, u, v) in faces {
        let rot = Quat::from_rotation_arc(Vec3::Y, normal);
        // Grid of clutter, jittered.
        for gu in 0..4 {
            for gv in 0..4 {
                let cu = -half + (gu as f32 + 0.5) * (CUBE_SIZE / 4.0) + rng.range(-0.6, 0.6);
                let cv = -half + (gv as f32 + 0.5) * (CUBE_SIZE / 4.0) + rng.range(-0.6, 0.6);
                let idx = (rng.unit() * assets.greeble_meshes.len() as f32) as usize;
                let mesh = assets.greeble_meshes[idx.min(assets.greeble_meshes.len() - 1)].clone();
                let glowing = rng.unit() < 0.14;
                let material = if glowing {
                    assets.greeble_glow.clone()
                } else if rng.unit() < 0.5 {
                    assets.greeble_dark.clone()
                } else {
                    assets.hull_material.clone()
                };
                let height = rng.range(0.15, 0.5);
                let spin = Quat::from_rotation_y(rng.range(0.0, TAU) * if rng.unit() < 0.5 { 0.0 } else { 1.0 });
                cube.spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::from_translation(normal * (half + height) + u * cu + v * cv)
                        .with_rotation(rot * spin),
                ));
            }
        }
        // Long edge rails along two sides of each face.
        for s in [-1.0, 1.0] {
            cube.spawn((
                Mesh3d(assets.greeble_meshes[2].clone()),
                MeshMaterial3d(assets.greeble_dark.clone()),
                Transform::from_translation(normal * (half + 0.2) + v * (s * (half - 0.4)))
                    .with_rotation(rot)
                    .with_scale(Vec3::new(CUBE_SIZE / 2.6, 1.0, 0.8)),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

fn grab_cursor(mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>) {
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
}

fn toggle_cursor(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    } else if cursor.grab_mode == CursorGrabMode::None && mouse.just_pressed(MouseButton::Left) {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
}

fn mouse_look(
    motion: Res<AccumulatedMouseMotion>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    mut player: Single<(&mut Transform, &mut Player)>,
) {
    if cursor.grab_mode == CursorGrabMode::None {
        return;
    }
    let (transform, player) = &mut *player;
    let delta = motion.delta;
    if delta != Vec2::ZERO {
        player.yaw -= delta.x * MOUSE_SENSITIVITY;
        player.pitch = (player.pitch - delta.y * MOUSE_SENSITIVITY).clamp(-FRAC_PI_2 + 0.05, FRAC_PI_2 - 0.05);
        transform.rotation = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
    }
}

fn fire_laser(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    assets: Res<Assets3d>,
    mut player: Single<(&Transform, &mut Player)>,
) {
    let (transform, player) = &mut *player;
    player.fire_cooldown.tick(time.delta());

    let wants_fire = keys.pressed(KeyCode::Space)
        || (cursor.grab_mode != CursorGrabMode::None && mouse.pressed(MouseButton::Left));
    if !wants_fire || !player.fire_cooldown.is_finished() {
        return;
    }
    player.fire_cooldown.reset();

    let forward = transform.forward().as_vec3();
    let right = transform.right().as_vec3();
    let down = -transform.up().as_vec3();

    // Twin cannons, mounted low on either side of the cockpit, converging on the crosshair.
    for side in [-1.0, 1.0] {
        let muzzle = transform.translation + right * side * 0.9 + down * 0.45 + forward * 1.5;
        let aim = transform.translation + forward * 140.0;
        let dir = (aim - muzzle).normalize();
        commands.spawn((
            Mesh3d(assets.laser_mesh.clone()),
            MeshMaterial3d(assets.laser_material.clone()),
            Transform::from_translation(muzzle)
                .with_rotation(Quat::from_rotation_arc(Vec3::Y, dir)),
            Laser {
                velocity: dir * LASER_SPEED,
                life: Timer::from_seconds(LASER_LIFETIME, TimerMode::Once),
            },
        ));
    }
}

// ---------------------------------------------------------------------------
// Simulation
// ---------------------------------------------------------------------------

fn move_lasers(
    mut commands: Commands,
    time: Res<Time>,
    mut lasers: Query<(Entity, &mut Transform, &mut Laser)>,
) {
    for (entity, mut transform, mut laser) in &mut lasers {
        laser.life.tick(time.delta());
        if laser.life.is_finished() {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation += laser.velocity * time.delta_secs();
    }
}

fn move_cube(time: Res<Time>, mut cubes: Query<(&mut Transform, &mut Cube)>) {
    let dt = time.delta_secs();
    for (mut transform, mut cube) in &mut cubes {
        cube.age += dt;
        let to_target = cube.target - transform.translation;
        let dist = to_target.length();
        if dist > 1.0 {
            // Ease in as it reaches the holding position.
            let speed = CUBE_APPROACH_SPEED.min(dist * 0.9 + 4.0);
            transform.translation += to_target / dist * speed * dt;
        }
        // Gentle bobbing while holding, so it never sits perfectly still.
        let bob = Vec3::new(
            (cube.age * 0.5).sin() * 0.8,
            (cube.age * 0.37).cos() * 0.6,
            0.0,
        );
        transform.translation += bob * dt;
        let (axis, speed) = (cube.spin_axis, cube.spin_speed);
        transform.rotate(Quat::from_axis_angle(axis, speed * dt));
    }
}

fn laser_hits(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<Assets3d>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut score: ResMut<Score>,
    mut respawn: ResMut<RespawnTimer>,
    mut rng: ResMut<Rng>,
    lasers: Query<(Entity, &Transform, &Laser)>,
    mut cubes: Query<(Entity, &Transform, &mut Cube, &Children)>,
    mut shields: Query<(&mut Visibility, &MeshMaterial3d<StandardMaterial>), With<Shield>>,
) {
    let Ok((cube_entity, cube_transform, mut cube, children)) = cubes.single_mut() else {
        return;
    };
    let center = cube_transform.translation;
    let dt = time.delta_secs();

    for (laser_entity, laser_transform, laser) in &lasers {
        // Swept sphere test: segment from previous to current position vs. the cube's bounding sphere.
        let p1 = laser_transform.translation;
        let p0 = p1 - laser.velocity * dt;
        if segment_hits_sphere(p0, p1, center, CUBE_HIT_RADIUS) {
            commands.entity(laser_entity).despawn();
            cube.hits += 1;

            // Impact spark.
            let impact = center + (p1 - center).normalize_or(Vec3::Z) * CUBE_HIT_RADIUS;
            spawn_impact(&mut commands, &assets, &mut rng, impact);

            if cube.hits >= 4 {
                spawn_explosion(&mut commands, &assets, &mut rng, center);
                commands.entity(cube_entity).despawn();
                score.destroyed += 1;
                respawn.0 = Some(Timer::from_seconds(CUBE_RESPAWN_DELAY, TimerMode::Once));
                return;
            }

            // Update the shield shell.
            for child in children.iter() {
                if let Ok((mut visibility, material)) = shields.get_mut(child) {
                    *visibility = if cube.hits < 3 {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                    if let Some(mat) = materials.get_mut(&material.0) {
                        *mat = shield_material_for(cube.hits);
                    }
                    commands.entity(child).insert(ShieldFlash(1.0));
                }
            }
            // Only one laser may connect per frame; the next one will be
            // evaluated next frame so each hit is distinct and visible.
            break;
        }
    }
}

/// Returns true when the segment p0→p1 passes within `radius` of `center`.
fn segment_hits_sphere(p0: Vec3, p1: Vec3, center: Vec3, radius: f32) -> bool {
    let d = p1 - p0;
    let len_sq = d.length_squared();
    let t = if len_sq > 0.0 {
        ((center - p0).dot(d) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p0 + d * t).distance_squared(center) <= radius * radius
}

/// Brightens the shield material briefly after each hit, then settles back.
fn shield_flash(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shields: Query<(Entity, &mut ShieldFlash, &MeshMaterial3d<StandardMaterial>, &mut Transform), With<Shield>>,
    cubes: Query<&Cube>,
) {
    let Ok(cube) = cubes.single() else {
        return;
    };
    for (entity, mut flash, material, mut transform) in &mut shields {
        flash.0 -= time.delta_secs() * 2.5;
        let f = flash.0.max(0.0);
        // Pulse the shell size a little on impact.
        transform.scale = Vec3::splat(1.0 + f * 0.06);
        if let Some(mat) = materials.get_mut(&material.0) {
            let base = shield_material_for(cube.hits);
            mat.emissive = base.emissive * (1.0 + f * 2.5);
            if cube.hits == 3 {
                // Shield collapse: brief white flash that fades to nothing.
                mat.emissive = LinearRgba::rgb(6.0, 6.0, 8.0) * f;
                mat.base_color = Color::srgba(0.8, 0.8, 1.0, 0.3 * f);
            }
        }
        if flash.0 <= 0.0 {
            transform.scale = Vec3::ONE;
            commands.entity(entity).remove::<ShieldFlash>();
        }
    }
}

fn spawn_impact(commands: &mut Commands, assets: &Assets3d, rng: &mut Rng, at: Vec3) {
    commands.spawn((
        Mesh3d(assets.core_mesh.clone()),
        MeshMaterial3d(assets.core_material.clone()),
        Transform::from_translation(at).with_scale(Vec3::splat(0.3)),
        ExplosionCore {
            life: Timer::from_seconds(0.25, TimerMode::Once),
            size: 2.0,
        },
    ));
    for _ in 0..6 {
        commands.spawn((
            Mesh3d(assets.debris_mesh.clone()),
            MeshMaterial3d(assets.laser_material.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(0.2)),
            Debris {
                velocity: rng.unit_vector() * rng.range(8.0, 20.0),
                spin: rng.unit_vector() * 6.0,
                life: Timer::from_seconds(0.4, TimerMode::Once),
            },
        ));
    }
}

fn spawn_explosion(commands: &mut Commands, assets: &Assets3d, rng: &mut Rng, at: Vec3) {
    commands.spawn((
        Mesh3d(assets.core_mesh.clone()),
        MeshMaterial3d(assets.core_material.clone()),
        Transform::from_translation(at).with_scale(Vec3::splat(2.0)),
        ExplosionCore {
            life: Timer::from_seconds(0.7, TimerMode::Once),
            size: CUBE_SIZE * 1.4,
        },
    ));
    commands.spawn((
        PointLight {
            color: Color::srgb(1.0, 0.6, 0.25),
            intensity: 40_000_000.0,
            range: 200.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_translation(at),
        ExplosionCore {
            life: Timer::from_seconds(0.5, TimerMode::Once),
            size: 0.0,
        },
    ));
    for _ in 0..70 {
        let offset = rng.unit_vector() * rng.range(0.0, CUBE_SIZE * 0.5);
        commands.spawn((
            Mesh3d(assets.debris_mesh.clone()),
            MeshMaterial3d(assets.debris_material.clone()),
            Transform::from_translation(at + offset)
                .with_scale(Vec3::splat(rng.range(0.4, 1.6)))
                .with_rotation(Quat::from_axis_angle(rng.unit_vector(), rng.range(0.0, PI))),
            Debris {
                velocity: offset.normalize_or(Vec3::Y) * rng.range(15.0, 45.0) + rng.unit_vector() * 6.0,
                spin: rng.unit_vector() * rng.range(2.0, 8.0),
                life: Timer::from_seconds(rng.range(0.8, 1.6), TimerMode::Once),
            },
        ));
    }
}

fn update_explosions(
    mut commands: Commands,
    time: Res<Time>,
    mut cores: Query<(Entity, &mut Transform, &mut ExplosionCore, Option<&mut PointLight>), Without<Debris>>,
    mut debris: Query<(Entity, &mut Transform, &mut Debris)>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut core, light) in &mut cores {
        core.life.tick(time.delta());
        let t = core.life.fraction();
        if let Some(mut light) = light {
            light.intensity = 40_000_000.0 * (1.0 - t);
        } else {
            // Expand quickly then shrink away.
            let s = (t * PI).sin().max(0.0) * core.size + 0.2;
            transform.scale = Vec3::splat(s);
        }
        if core.life.is_finished() {
            commands.entity(entity).despawn();
        }
    }
    for (entity, mut transform, mut d) in &mut debris {
        d.life.tick(time.delta());
        if d.life.is_finished() {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation += d.velocity * dt;
        d.velocity *= 1.0 - 0.9 * dt;
        transform.rotate(Quat::from_scaled_axis(d.spin * dt));
        let remaining = d.life.fraction_remaining();
        transform.scale *= (1.0 - dt * (1.0 - remaining)).max(0.0);
    }
}

fn respawn_cube(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<Assets3d>,
    mut rng: ResMut<Rng>,
    mut respawn: ResMut<RespawnTimer>,
    player: Single<&Transform, With<Player>>,
) {
    let Some(timer) = respawn.0.as_mut() else {
        return;
    };
    timer.tick(time.delta());
    if timer.is_finished() {
        respawn.0 = None;
        spawn_cube(&mut commands, &assets, &mut rng, player.forward().as_vec3());
    }
}

fn update_hud(
    score: Res<Score>,
    cubes: Query<&Cube>,
    mut hud: Single<&mut Text, With<HudText>>,
) {
    let status = match cubes.single() {
        Ok(cube) => match cube.hits {
            0 => "Target: shields down, hull intact",
            1 => "Target: SHIELDS UP (blue)",
            2 => "Target: SHIELDS STRAINED (violet)",
            _ => "Target: SHIELDS COLLAPSED - finish it!",
        },
        Err(_) => "Target destroyed - scanning for contacts...",
    };
    hud.0 = format!("Cubes destroyed: {}\n{}", score.destroyed, status);
}
