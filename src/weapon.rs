use std::collections::HashMap;
use std::time::{Duration};

use futures_util::stream::StreamExt;
use stubs::common::v0::{
    Orientation, Position, Vector, Velocity, Weapon
};

use stubs::mission::v0::stream_events_response::{ShotEvent, Event};

use stubs::weapon::v0::stream_weapons_response::{WeaponGone, Update};
use stubs::weapon::v0::weapon_service_server::WeaponService;
use stubs::weapon::v0::{StreamWeaponsRequest, StreamWeaponsResponse, GetTransformRequest, GetTransformResponse};

use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::Sender;
use tokio::time::MissedTickBehavior;
use tonic::{Code, Request, Status};

use crate::rpc::MissionRpc;

/// Stream unit updates.
pub async fn stream_weapons(
    opts: StreamWeaponsRequest,
    rpc: MissionRpc,
    tx: Sender<Result<StreamWeaponsResponse, Status>>,
) -> Result<(), Error> {
    // initialize the state for the current units stream instance
    let poll_rate = opts.poll_rate.unwrap_or(1000);
    let poll_rate = Duration::from_millis(poll_rate as u64);
    let mut state = State {
        weapons: HashMap::new(),
        ctx: Context {
            rpc,
            tx,
            poll_rate,
        },
    };

    // We are purely reactive on events and do not track non-event based as there isn't a function
    // to get all airborne weapons (that i know of), so we can do an initial sync of weapons in
    // flight - which is sufficient for our needs anyhow as we want the initiator which is only
    // reliably existing from the event

    // initiate an event stream used to update the state
    let mut events = state.ctx.rpc.events().await;

    // create an interval used to poll the mission for updates
    let mut interval = tokio::time::interval(poll_rate);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        // wait for either the next event or the next tick, whatever happens first
        tokio::select! {
            // listen to events that update the current state
            Some(stubs::mission::v0::StreamEventsResponse { time, event: Some(event), .. })
                = events.next() =>
            {
                handle_event(&mut state, time, event).await?;
            }

            // poll units for updates
            _ = interval.tick() => {
                update_weapons(&mut state).await?;
            }
        }
    }
}

/// The state of an active units stream.
struct State {
    weapons: HashMap<u32, WeaponState>,
    ctx: Context,
}

/// Various structs and options used to handle unit updates.
struct Context {
    rpc: MissionRpc,
    tx: Sender<Result<StreamWeaponsResponse, Status>>,
    poll_rate: Duration,
}

/// Update the given [State] based on the given [Event].
async fn handle_event(
    state: &mut State,
    time: f64,
    event: Event,
) -> Result<(), Error> {
    match event {
        // When a weapon is birthed (shot), we need to add it to the list
        // to monitor next update
        Event::Shot(ShotEvent {
            weapon: Some(weapon),
            ..
        }) => {
            state
                .ctx
                .tx
                .send(Ok(StreamWeaponsResponse {
                    time,
                    update: Some(Update::Weapon(weapon.clone())),
                }))
                .await?;

            // And add to our monitored events
            state
                .weapons
                .insert(weapon.id.clone(), WeaponState::new(weapon));
        },
        _ => {}
    }

    Ok(())
}

/// Updates all units inside of the provided [State].
async fn update_weapons(state: &mut State) -> Result<(), Error> {
    let mut weapons = std::mem::take(&mut state.weapons);
    // Update all weapons in parallel (will queue a request for each unit, but the execution will
    // still be throttled by the throughputLimit setting).
    futures_util::future::try_join_all(
        weapons 
            .values_mut()
            .map(|weapon_state| update_weapon(&state.ctx, weapon_state)),
    )
    .await?;

    // remove state for all units that are gone
    weapons.retain(|_, v| !v.is_gone);
    state.weapons = weapons;

    Ok(())
}

async fn update_weapon(ctx: &Context, weapon_state: &mut WeaponState) -> Result<(), Error> {

    // weapons are always checked because they tend not to be stationary like ground units :) 

    match weapon_state.update(ctx).await {
        Ok(changed) => {
            if changed {
                ctx.tx
                    .send(Ok(StreamWeaponsResponse {
                        time: weapon_state.update_time,
                        update: Some(Update::Weapon(weapon_state.weapon.clone())),
                    }))
                    .await?;
            }

            Ok(())
        }
        // if the unit was not found, flag it as gone, and continue with the next unit for now
        Err(err) if err.code() == Code::NotFound => {
            ctx.tx
                .send(Ok(StreamWeaponsResponse {
                    // The time provided here is just the last time an update was received for the
                    // unit. It is not exactly the time the unit got destroyed. Since this not-found
                    // handling is just a safeguard if a `Dead` event was missed / not fired by DCS,
                    // it should be ok that it is not the exact time of death.
                    time: weapon_state.update_time,
                    update: Some(Update::Gone(WeaponGone {
                        id: weapon_state.weapon.id,
                    })),
                }))
                .await?;

            weapon_state.is_gone = true;

            Ok(())
        }
        Err(err) => Err(err.into()),
    }
}


/// The last know information about a unit and various other information to track whether it is
/// worth checking the unit for updates or not.
struct WeaponState {
    weapon: Weapon,
    update_time: f64,
    is_gone: bool,
}

impl WeaponState {
    fn new(weapon: Weapon) -> Self {
        Self {
            weapon,
            update_time: 0.0,
            is_gone: false,
        }
    }

    /// Check the unit for updates and return whether the unit got changed or not.
    async fn update(&mut self, ctx: &Context) -> Result<bool, Status> {
        let mut changed = false;

        let res = WeaponService::get_transform(
            &ctx.rpc,
            Request::new(GetTransformRequest {
                id: self.weapon.id.clone(),
            }),
        )
        .await?;
        let GetTransformResponse {
            time,
            position,
            orientation,
            velocity,
        } = res.into_inner();

        self.update_time = time;

        if let Some((before, after)) = self.weapon.position.as_mut().zip(position) {
            if !position_equalish(before, &after) {
                *before = after;
                changed = true;
            }
        }
        if let Some((before, after)) = self.weapon.orientation.as_mut().zip(orientation) {
            if !orientation_equalish(before, &after) {
                *before = after;
                changed = true;
            }
        }
        if let Some((before, after)) = self.weapon.velocity.as_mut().zip(velocity) {
            if !velocity_equalish(before, &after) {
                *before = after;
                changed = true;
            }
        }

        Ok(changed)
    }
}


#[derive(Debug, thiserror::Error)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    #[error(transparent)]
    Status(#[from] Status),
    #[error("the channel got closed")]
    Send(#[from] SendError<Result<StreamWeaponsResponse, Status>>),
}

/// Check whether two positions are equal, taking an epsilon into account.
fn position_equalish(l: &Position, r: &Position) -> bool {
    const LL_EPSILON: f64 = 0.000001;
    const ALT_EPSILON: f64 = 0.001;
    meters_equalish(l.u, r.u) && meters_equalish(l.v, r.v)
}

/// Check whether two orientations are equal, taking an epsilon into account.
fn orientation_equalish(l: &Orientation, r: &Orientation) -> bool {
    let Orientation {
        heading,
        yaw,
        pitch,
        roll,
        forward,
        right,
        up,
    } = l;

    if let Some((l, r)) = forward.as_ref().zip(r.forward.as_ref()) {
        if !vector_equalish(l, r) {
            return false;
        }
    }

    if let Some((l, r)) = right.as_ref().zip(r.right.as_ref()) {
        if !vector_equalish(l, r) {
            return false;
        }
    }

    if let Some((l, r)) = up.as_ref().zip(r.up.as_ref()) {
        if !vector_equalish(l, r) {
            return false;
        }
    }

    if !degrees_equalish(*heading, r.heading)
        || !degrees_equalish(*yaw, r.yaw)
        || !degrees_equalish(*pitch, r.pitch)
        || !degrees_equalish(*roll, r.roll)
    {
        return false;
    }

    true
}

/// Check whether two velocities are equal, taking an epsilon into account.
fn velocity_equalish(l: &Velocity, r: &Velocity) -> bool {
    let Velocity {
        heading,
        speed,
        velocity,
    } = l;

    if let Some((l, r)) = velocity.as_ref().zip(r.velocity.as_ref()) {
        if !vector_equalish(l, r) {
            return false;
        }
    }

    if !degrees_equalish(*heading, r.heading) {
        return false;
    }

    if !speed_equalish(*speed, r.speed) {
        return false;
    }

    true
}

/// Check whether two vectors are equal, taking an epsilon into account.
fn vector_equalish(a: &Vector, b: &Vector) -> bool {
    const EPSILON: f64 = 0.000001;
    (a.x - b.x).abs() < EPSILON && (a.y - b.y).abs() < EPSILON && (a.z - b.z).abs() < EPSILON
}

/// Check whether two distances in meter are equal, taking an epsilon into account.
fn meters_equalish(a: f64, b: f64) -> bool {
    const EPSILON: f64 = 0.001;
    (a - b).abs() < EPSILON
}

/// Check whether two angles in degrees are equal, taking an epsilon into account.
fn degrees_equalish(a: f64, b: f64) -> bool {
    const EPSILON: f64 = 0.01;
    (a - b).abs() < EPSILON
}

/// Check whether two speeds are equal, taking an epsilon into account.
fn speed_equalish(a: f64, b: f64) -> bool {
    const EPSILON: f64 = 0.001;
    (a - b).abs() < EPSILON
}
