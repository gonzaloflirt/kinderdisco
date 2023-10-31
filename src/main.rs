use anyhow::{anyhow, Result};
use async_std::{io, task};
use clap::Parser;
use futures::{future, pin_mut, FutureExt};
use huelib::{bridge, bridge::Bridge, resource::light::StateModifier, resource::Light, Color};
use rand::Rng;
use std::{net::IpAddr, sync::Arc, time::Duration};

#[derive(Parser, Debug)]
struct Args {
    #[clap(short, long)]
    register_user: bool,

    #[clap(short, long)]
    user: Option<String>,

    #[clap(short, long)]
    halloween: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.register_user {
        return register_user();
    }

    if let Some(user) = args.user {
        return kinderdisco(user, args.halloween);
    }

    println!("Usage: \"kinderdisco --user $USER\" or \"kinderdisco --register-user\"");

    Ok(())
}

fn get_bridge_ip() -> Result<IpAddr> {
    bridge::discover_nupnp()?
        .pop()
        .ok_or_else(|| anyhow!("No hue bridge found."))
}

fn register_user() -> Result<()> {
    let ip = get_bridge_ip()?;
    let username = bridge::register_user(ip, "kinderdisco")?;
    println!("Registered user with username `{}`", username);
    Ok(())
}

fn get_color_lights(bridge: &bridge::Bridge) -> Result<Vec<Light>> {
    Ok(bridge
        .get_all_lights()?
        .drain(..)
        .filter(|light| light.kind == "Extended color light")
        .collect())
}

async fn modify_light(light: Light, bridge: &Bridge, halloween: bool) {
    loop {
        let mut rng = rand::thread_rng();
        let (modifier, time) = if halloween {
            let transition = rng.gen_range(9..90);
            (
                StateModifier::new()
                    .with_on(true)
                    .with_color(Color::from_rgb(
                        rng.gen_range(240..255),
                        rng.gen_range(25..170),
                        rng.gen_range(0..50),
                    ))
                    .with_transition_time(transition),
                transition,
            )
        } else {
            (
                StateModifier::new()
                    .with_on(true)
                    .with_color(Color::from_rgb(rng.gen(), rng.gen(), rng.gen()))
                    .with_transition_time(0),
                rng.gen_range(3..30),
            )
        };
        _ = bridge.set_light_state(&light.id, &modifier);
        task::sleep(Duration::from_millis(time as u64 * 100)).await;
    }
}

async fn modify_color_lights(user: String, halloween: bool) -> Result<()> {
    let ip = get_bridge_ip()?;
    let bridge = Arc::new(Bridge::new(ip, user));

    let lights = get_color_lights(&bridge)?
        .drain(..)
        .map(|light| modify_light(light, &bridge, halloween))
        .collect::<Vec<_>>();
    future::join_all(lights).await;
    Ok(())
}

async fn wait_for_key_press() -> Result<()> {
    let stdin = io::stdin();
    let mut line = String::new();
    stdin.read_line(&mut line).await?;
    Ok(())
}

fn kinderdisco(user: String, halloween: bool) -> Result<()> {
    let lights = modify_color_lights(user, halloween).fuse();
    pin_mut!(lights);

    let key_press = wait_for_key_press().fuse();
    pin_mut!(key_press);

    println!("Press a key to quit!");

    async_std::task::block_on(async move {
        futures::select! {
            r = lights => {  if r.is_err() { println!("ERROR: {:?}", r)}; },
            _ = key_press => (),
        };
    });

    Ok(())
}
