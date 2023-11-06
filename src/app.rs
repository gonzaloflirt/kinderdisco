use anyhow::{anyhow, Result};
use async_std::task;
use core::ops::Range;
use futures::{future::Either, pin_mut, FutureExt};
use huelib::{bridge, bridge::Bridge, resource::light::StateModifier, resource::Light, Color};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{mpsc, Arc, RwLock},
    time::Duration,
};

static APP_NAME: &str = "kinderdisco";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncMode {
    None,
    Time,
    TimeAndColor,
}
#[derive(Clone)]
pub struct Data {
    pub r: Range<u8>,
    pub g: Range<u8>,
    pub b: Range<u8>,
    pub time: Range<u16>,
    pub fade: bool,
}

impl Default for Data {
    fn default() -> Self {
        Self {
            r: 0..255,
            g: 0..255,
            b: 0..255,
            time: 1..10,
            fade: false,
        }
    }
}

pub struct DiscoLight {
    pub light: Light,
    pub on: bool,
}

impl DiscoLight {
    pub fn new(light: Light) -> Self {
        Self { light, on: false }
    }
}

fn rand_range<S, T>(rng: &mut S, range: &core::ops::Range<T>) -> T
where
    S: random::Source,
    T: Copy
        + num_traits::identities::Zero
        + random::Value
        + std::ops::Sub<Output = T>
        + std::ops::Rem
        + std::ops::Add<<T as std::ops::Rem>::Output, Output = T>,
{
    let span = range.end - range.start;
    if span.is_zero() {
        range.start
    } else {
        range.start + (rng.read::<T>() % span)
    }
}

async fn modify_lights_same_color(bridge: Bridge, light_ids: Vec<String>, data: Arc<RwLock<Data>>) {
    let mut rng = random::default(43);
    loop {
        let time;
        {
            let data = data.read().unwrap();
            time = rand_range(&mut rng, &data.time);
            let transition_time = if data.fade { time } else { 0 };
            let modifier = StateModifier::new()
                .with_on(true)
                .with_color(Color::from_rgb(
                    rand_range(&mut rng, &data.r),
                    rand_range(&mut rng, &data.g),
                    rand_range(&mut rng, &data.b),
                ))
                .with_transition_time(transition_time);
            for light_id in &light_ids {
                _ = bridge.set_light_state(light_id, &modifier);
            }
        }
        task::sleep(Duration::from_millis(time as u64 * 100)).await;
    }
}

async fn modify_lights_different_colors(
    bridge: Bridge,
    light_ids: Vec<String>,
    data: Arc<RwLock<Data>>,
) {
    let mut rng = random::default(
        light_ids
            .first()
            .unwrap_or(&42.to_string())
            .parse::<u64>()
            .unwrap_or(42),
    );
    loop {
        let time;
        {
            let data = data.read().unwrap();
            time = rand_range(&mut rng, &data.time);
            let transition_time = if data.fade { time } else { 0 };
            for light_id in &light_ids {
                let modifier = StateModifier::new()
                    .with_on(true)
                    .with_color(Color::from_rgb(
                        rand_range(&mut rng, &data.r),
                        rand_range(&mut rng, &data.g),
                        rand_range(&mut rng, &data.b),
                    ))
                    .with_transition_time(transition_time);
                _ = bridge.set_light_state(light_id, &modifier);
            }
        }
        task::sleep(Duration::from_millis(time as u64 * 100)).await;
    }
}
struct Modulator(futures::channel::oneshot::Sender<()>);

impl Modulator {
    fn new(
        sync_mode: SyncMode,
        light_ids: Vec<String>,
        bridge: Bridge,
        data: Arc<RwLock<Data>>,
    ) -> Self {
        let (sender, receiver) = futures::channel::oneshot::channel::<()>();
        task::spawn(async move {
            let task = match sync_mode {
                SyncMode::TimeAndColor => {
                    Either::Left(modify_lights_same_color(bridge, light_ids, data))
                }
                _ => Either::Right(modify_lights_different_colors(bridge, light_ids, data)),
            };
            let task = task.fuse();
            let receiver = receiver.fuse();
            pin_mut!(task);
            pin_mut!(receiver);
            futures::select! {
            _ = receiver => (),
            _ = task => (),
            };
        });
        Self(sender)
    }
}

enum Signal {
    Ip(Option<IpAddr>),
    Bridge(Option<Bridge>),
    Lights(Vec<Light>),
    Error(String),
}

pub struct App {
    pub ip: Option<IpAddr>,
    pub user: Option<String>,
    pub bridge: Option<Bridge>,
    pub lights: HashMap<String, DiscoLight>,
    pub error: Option<String>,
    channel: (mpsc::Sender<Signal>, mpsc::Receiver<Signal>),
    pub data: Data,
    pub async_data: Arc<RwLock<Data>>,
    pub sync_mode: SyncMode,
    modulators: Vec<Modulator>,
    pub rebuild_modulators: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            ip: None,
            user: None,
            bridge: None,
            lights: HashMap::default(),
            error: None,
            channel: mpsc::channel::<Signal>(),
            data: Data::default(),
            async_data: Arc::new(RwLock::new(Data::default())),
            sync_mode: SyncMode::None,
            modulators: vec![],
            rebuild_modulators: false,
        }
    }
}

impl App {
    pub fn poll(&mut self) {
        while let Ok(signal) = self.channel.1.try_recv() {
            match signal {
                Signal::Ip(ip) => {
                    self.error = None;
                    self.ip = ip;
                    self.user = load_user();
                    if let Some(user) = self.user.clone() {
                        self.bridge = Some(Bridge::new(ip.unwrap(), user));
                        self.get_color_lights();
                    }
                }
                Signal::Bridge(bridge) => {
                    self.error = None;
                    self.bridge = bridge;
                    self.get_color_lights()
                }
                Signal::Lights(mut lights) => {
                    self.error = None;
                    _ = lights
                        .drain(..)
                        .map(|light| {
                            if !self.lights.contains_key(&light.unique_id) {
                                _ = self
                                    .lights
                                    .insert(light.unique_id.clone(), DiscoLight::new(light));
                            }
                        })
                        .collect::<Vec<_>>();
                }
                Signal::Error(e) => self.error = Some(e),
            }
        }
    }

    pub fn get_bridge_ip(&mut self) {
        let sender = self.channel.0.clone();
        async_std::task::spawn(async move {
            match get_bridge_ip().await {
                Ok(ip) => {
                    let _ = sender.send(Signal::Ip(Some(ip)));
                }
                Err(e) => {
                    _ = sender.send(Signal::Error(format!("Error: {}", e)));
                }
            };
        });
    }

    pub fn register_user(&mut self, ip: IpAddr) {
        let sender = self.channel.0.clone();
        async_std::task::spawn(async move {
            match register_user(ip).await {
                Ok(user) => {
                    store_user(user.clone());
                    let bridge = Bridge::new(ip, user);
                    let _ = sender.send(Signal::Bridge(Some(bridge)));
                }
                Err(e) => {
                    _ = sender.send(Signal::Error(format!("Error: {}", e)));
                }
            }
        });
    }

    fn get_color_lights(&mut self) {
        if let Some(bridge) = self.bridge.clone() {
            let sender = self.channel.0.clone();
            async_std::task::spawn(async move {
                if let Ok(lights) = get_color_lights(bridge).await {
                    let _ = sender.send(Signal::Lights(lights));
                }
            });
        }
    }

    pub fn update_data(&mut self) {
        {
            let mut async_data = self.async_data.write().unwrap();
            *async_data = self.data.clone();
        }

        if self.rebuild_modulators {
            self.rebuild_modulators();
            self.rebuild_modulators = false;
        }
    }

    fn rebuild_modulators(&mut self) {
        self.modulators.clear();
        if let Some(bridge) = &self.bridge {
            let mut lights = self
                .lights
                .iter()
                .filter(|(_, light)| light.on)
                .map(|light| light.1.light.id.clone())
                .collect::<Vec<_>>();

            self.modulators = match self.sync_mode {
                SyncMode::None => lights
                    .drain(..)
                    .map(|l| {
                        Modulator::new(
                            SyncMode::None,
                            vec![l],
                            bridge.clone(),
                            self.async_data.clone(),
                        )
                    })
                    .collect::<Vec<_>>(),
                _ => vec![Modulator::new(
                    self.sync_mode,
                    lights,
                    bridge.clone(),
                    self.async_data.clone(),
                )],
            }
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct Config {
    user: Option<String>,
}

fn load_user() -> Option<String> {
    match confy::load::<Config>(APP_NAME, None) {
        Ok(config) => config.user,
        _ => None,
    }
}

fn store_user(user: String) {
    let config = Config { user: Some(user) };
    _ = confy::store(APP_NAME, None, config);
}

pub async fn get_bridge_ip() -> Result<IpAddr> {
    bridge::discover_nupnp()?
        .pop()
        .ok_or_else(|| anyhow!("No hue bridge found."))
}

async fn register_user(ip: IpAddr) -> Result<String> {
    Ok(bridge::register_user(ip, "kinderdisco")?)
}

async fn get_color_lights(bridge: Bridge) -> Result<Vec<Light>> {
    Ok(bridge
        .get_all_lights()?
        .drain(..)
        .filter(|light| light.kind == "Extended color light")
        .collect())
}
