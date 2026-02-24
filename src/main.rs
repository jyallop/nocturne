use bevy_kira_audio::prelude::*;
use bevy::{
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    post_process::bloom::Bloom,
    prelude::*,
};
use hound;
use std::cmp::min;
use rand::prelude::*;
use faer::{Mat};
//use goertzel_algorithm::Goertzel;
use ultrafastgoertzel::goertzel_batch;

const NUM_TRIANGLES: f32 = 20.0;

// FOR CHIMES
// const RMS_THRESHOLD: f64 = 10.0;
// const SONG: &str = "chime.wav";
// const FREQS: usize = 1;
// const RMS_WINDOW: usize = 1000;
// const DECAY: f64 = 0.60;

//const SONG: &str = "dotf.wav";

// FOR CHOPIN
const RMS_THRESHOLD: f64 = 50.0;
const SONG: &str = "chopin.wav";
//const SONG: &str = "fudd.wav";
const FREQS: usize = 4 * 12;
const RMS_WINDOW: usize = 800;
const DECAY: f64 = 0.30;

const START_FREQ: usize = 20;
const DELAY: f32 = 1.0;

fn piano_keys(start: usize, keys: usize) -> Vec<f64> {
    (start..(start + keys)).into_iter().map(|x| 1.0 / (2_f64.powf((x as f64 - 49.0) / 12.0) * 440.0)).collect::<Vec<f64>>()
}

fn smooth_function(samples: usize) -> Mat<f64> {
    Mat::from_fn(samples, samples,
		 |i, j| if i == j { 1.0 } else if i == j + 1 { 1.0 }
		 else if i + 1 == j { 1.0 }
//		 else if i + 2 == j { 0.8 }
//		 else if j + 2 == i { 0.8 }
		 else { 0.0 })
}

fn generate_filter(size: usize, x: usize, y: usize) -> Mat<f32> {
    Mat::from_fn(size, size,
		 |i, j|
		 if x < i {
		     (if i == j { 1.0 }
		     else if i == j + 1 { 1.2 }
		      else if i + 1 == j { 0.2 }
		      else { 0.0 }) as f32
		 } else if x > i {
		     (if i == j + 1 { 0.2 }
		      else if i == j { 1.0 }
		      else if i + 1 == j { 1.2 }
		      else { 0.0 }) as f32
		 } else if x == i && j == y {
		     0.49 as f32
		 } else {
		     0.0 as f32
		 })
}

#[derive(Component)]
struct Coordinate {
    x: i32,
    y: i32
}

#[derive(Resource)]
struct LoopAudioInstanceHandle(Handle<AudioInstance>);

#[derive(Resource)]
struct State {
//    sound: Vec<Vec<f64>>,
    sound: Mat<f64>,
    max_rms: f64,
    started: Vec<bool>,
    rate: f64,
    tri: Mat<f32>,
    color: (f32, f32, f32),
    filter: Mat<f32>,
    time: f32,
}

fn main() {
    let window_plugin = WindowPlugin {
        primary_window: Some(Window {
            title: "art".into(),
            ..default()
        }),
        ..default()
    };
    App::new()
	.add_plugins((DefaultPlugins.set(window_plugin), AudioPlugin))
//	.add_plugins((DefaultPlugins, AudioPlugin))
	.add_systems(Startup, setup)
	.add_systems(Update, move_system)
	.run();
}


fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    audio: Res<Audio>,
    window: Single<&mut Window>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let mut bloom = Bloom::default();
    bloom.intensity = 0.10;
    bloom.low_frequency_boost = 0.30;
    bloom.low_frequency_boost_curvature = 0.80;

    commands.spawn((
        Camera2d,
        Camera {
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        Tonemapping::TonyMcMapface, // 1. Using a tonemapper that desaturates to white is recommended
        bloom,
        DebandDither::Enabled,      // Optional: bloom causes gradients which cause banding
    ));

    let x_shift: f32 = window.width() / 2.0;
    let y_shift: f32 = window.height() / 2.0;

    let x_size: f32 = window.width() / NUM_TRIANGLES;
    let y_size: f32 = window.height() / NUM_TRIANGLES;

    for i in 0..NUM_TRIANGLES as i32 {
	for j in 0..NUM_TRIANGLES as i32 {
	    let point_one: Vec2 = if j % 2 == 0 { Vec2::new((i as f32) * x_size - x_shift,
							    (j as f32) * y_size - y_shift) }
	    else {  Vec2::new((i as f32) * x_size - x_shift - x_size / 2.0, (j as f32) * y_size - y_shift) };

	    let point_two: Vec2 = if j % 2 == 0 { Vec2::new(((i + 1) as f32) * x_size - x_shift,
							    (j as f32) * y_size - y_shift) }
	    else {  Vec2::new((i as f32) * x_size - x_shift + x_size / 2.0, (j as f32) * y_size - y_shift) };

	    let point_three: Vec2 = if j % 2 == 0 { Vec2::new((i as f32) * x_size - x_shift + x_size / 2.0,
							      ((j + 1) as f32) * y_size - y_shift) }
	    else {  Vec2::new((i as f32) * x_size - x_shift, ((j + 1) as f32) * y_size - y_shift) };
		
	    commands.spawn((
		Mesh2d(meshes.add(Triangle2d::new(point_one, point_two, point_three))),
		MeshMaterial2d(materials.add(Color::BLACK)), //Color::srgb(10.0, 0.0, 10.0))),
		Transform::from_translation(Vec3::new(0., 0., 0.)),
		Coordinate { x: i, y: j }
	    ));
	}
    }

    let mut reader = hound::WavReader::open("assets/".to_owned() + SONG).unwrap();

    let mut sound: Vec<f64> = Vec::new();
    for s in reader.samples::<i32>() {
	let sample = s.unwrap() as f64;
	sound.push(sample);
    }

    let mut mags: Vec<Vec<f64>> = Vec::new();
    let mut frequencies = piano_keys(START_FREQ, FREQS);
    if SONG == "chime.wav" {
	frequencies = vec![ 1.0 /  5086.0 ];
    }

    for i in 0..(sound.len() / RMS_WINDOW) {
	let end = min(i * RMS_WINDOW + RMS_WINDOW, sound.len());

	let magnitudes = goertzel_batch(&sound[i * RMS_WINDOW..end], &frequencies);

	mags.push(magnitudes);
    }

    let magnitudes = Mat::from_fn(FREQS, mags.len(), |i, j| mags[j][i]);
    let smoothed = (1.0 / 3.0) * magnitudes.clone() * smooth_function(mags.len());

    let m = mags.clone().into_iter().fold(0.0, |acc, x| {
	let new = x.into_iter().fold(0.0, |acc_two, y| if y > acc_two { y } else { acc_two });
	if new > acc { new } else { acc }
    });

    println!("max: {}", m / RMS_THRESHOLD);

    let instance_handle = audio.play(asset_server.load(SONG)).looped().handle();

    let test = generate_filter(NUM_TRIANGLES as usize, 3, 3);
    commands.insert_resource(LoopAudioInstanceHandle(instance_handle));
    commands
	.insert_resource(State { // sound: mags, // Vec::new(), //sound,
	    sound: smoothed, // Vec::new(), //sound,
	    max_rms: m,
	    started: vec![false; FREQS],
	    rate: reader.spec().sample_rate as f64,
	    tri: Mat::from_fn(NUM_TRIANGLES as usize, NUM_TRIANGLES as usize,
			      |_, _| (0.0) as f32),
	    color: (0.0, 0.0, 0.0),
	    filter: test,
	    time: 0.0
	});
}

fn move_system(time: Res<Time>,
	       audio: Res<Audio>,
	       mut state: ResMut<State>,
	       mut materials: ResMut<Assets<ColorMaterial>>,
	       triangles: Query<(&Coordinate, &mut MeshMaterial2d<ColorMaterial>,)>,
	       loop_audio: Res<LoopAudioInstanceHandle>) {
    let pos = audio.state(&loop_audio.0).position().unwrap_or(0.0);
    let curr = min((pos * state.rate) as usize / RMS_WINDOW, state.sound.ncols() - 1);
    let prev = if curr > 0 { curr - 1 } else { 0 };
    let mut changed = false;
    for i in 0..FREQS {
	if state.sound[(i, prev)] - state.sound[(i, curr)] < -(state.max_rms / RMS_THRESHOLD) && !state.started[i] {
	    state.started[i] = true;
	    let i = rand::random_range(0..NUM_TRIANGLES as i32);
	    let j = rand::random_range(0..NUM_TRIANGLES as i32);
	    state.tri[(i as usize, j as usize)] = 1.0;

	    changed = true;
	    state.filter = generate_filter(NUM_TRIANGLES as usize, i as usize, j as usize);
	} else if state.sound[(i, prev)] > state.sound[(i, curr)] && state.started[i] {
	    state.started[i] = false;
	}
    }

    if state.time > DELAY && changed {
	state.time = 0.0;
	let choices = [0.0, 1.0];
	let mut rng = rand::rng();
	let mut r = choices.choose(&mut rng).unwrap_or(&0.0) * 255.0;
	let g = choices.choose(&mut rng).unwrap_or(&0.0) * 255.0;
	let b = choices.choose(&mut rng).unwrap_or(&0.0) * 255.0;
	if r + g + b < 10.0 {
	    r = 255.0;
	}
	
	state.color = (r, g, b);
    } else {
	state.time += time.delta_secs();
    }

    let (h, s, l) = state.color;
    let mut next_state = state.tri.clone() + state.tri.clone().transpose();
    for (pos, mut color) in triangles {
	let a = next_state[(pos.x as usize, pos.y as usize)];
	if a > 0.02 {
	    *color = MeshMaterial2d(materials.add(Color::srgba(h, s, l, a)));
	} else {
	    //		next_state[(pos.x as usize, pos.y as usize)] = 0.0;
	    *color = MeshMaterial2d(materials.add(Color::srgba(h, s, l, a)));
	}

    }
    state.tri = DECAY * (1.0 / 3.0) * (&state.filter * next_state);
}

