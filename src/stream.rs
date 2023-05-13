use std::collections::HashMap;
use std::time::{Duration, Instant};

use futures_util::stream::StreamExt;
use futures_util::TryFutureExt;
use stubs::coalition::v0::coalition_service_server::CoalitionService;
use stubs::coalition::v0::GetGroupsRequest;
use stubs::coalition::v0::GetStaticObjectsRequest;
use stubs::common;
use stubs::common::v0::{Coalition, GroupCategory, Orientation, Position, Unit, Static, Vector, Velocity};
use stubs::group::v0::group_service_server::GroupService;
use stubs::group::v0::GetUnitsRequest;
use stubs::mission::v0::stream_events_response::{BirthEvent, DeadEvent, Event};
use stubs::mission::v0::stream_units_response::{UnitGone, Update};
use stubs::mission::v0::{StreamUnitsRequest, StreamUnitsResponse};
use stubs::unit::v0::unit_service_server::UnitService;
use stubs::unit::v0::{GetTransformRequest, GetTransformResponse};
use stubs::unit::v0::{GetStaticTransformRequest, GetStaticTransformResponse};
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::Sender;
use tokio::time::MissedTickBehavior;
use tonic::{Code, Request, Status};

use crate::rpc::MissionRpc;

/// Stream unit updates.
pub async fn stream_units(
    opts: StreamUnitsRequest,
    rpc: MissionRpc,
    tx: Sender<Result<StreamUnitsResponse, Status>>,
) -> Result<(), Error> {
    // initialize the state for the current units stream instance
    let poll_rate = opts.poll_rate.unwrap_or(5);
    let max_backoff = Duration::from_secs(opts.max_backoff.unwrap_or(30).max(poll_rate) as u64);
    let poll_rate = Duration::from_secs(poll_rate as u64);
    let category = GroupCategory::from_i32(opts.category).unwrap_or(GroupCategory::Unspecified);
    let include_static_objects = opts.include_static_objects.unwrap_or(false);
    log::info!("Include Statics ? {}", include_static_objects);
    let mut state = State {
        units: HashMap::new(),
        statics: HashMap::new(),
        ctx: Context {
            rpc,
            tx,
            poll_rate,
            max_backoff,
        },
    };

    // initial full-sync of all current units inside of the mission
    let groups = futures_util::future::try_join_all(
        [Coalition::Blue, Coalition::Red, Coalition::Neutral].map(|coalition| {
            state
                .ctx
                .rpc
                .get_groups(Request::new(GetGroupsRequest {
                    coalition: coalition.into(),
                    category: opts.category,
                }))
                .map_ok(|res| res.into_inner().groups)
        }),
    )
    .await?
    .into_iter()
    .flatten();

    let group_units = futures_util::future::try_join_all(groups.into_iter().map(|group| {
        state
            .ctx
            .rpc
            .get_units(Request::new(GetUnitsRequest {
                group_name: group.name,
                active: Some(true),
            }))
            .map_ok(|res| res.into_inner().units)
    }))
    .await?;

    for units in group_units {
        state.units.extend(
            units
                .into_iter()
                .map(|unit| (unit.name.clone(), UnitState::new(unit))),
        )
    }

    if include_static_objects {
        // initial full-sync of all current units inside of the mission
        let static_objects = futures_util::future::try_join_all(
            [Coalition::Blue, Coalition::Red, Coalition::Neutral].map(|coalition| {
                state
                    .ctx
                    .rpc
                    .get_static_objects(Request::new(GetStaticObjectsRequest {
                        coalition: coalition.into(),
                    }))
                    .map_ok(|res| res.into_inner().statics)
            }),
        )
        .await?
        .into_iter()
        .flatten();

        state.statics.extend(
            static_objects
                .into_iter()
                .map(|static_object| (static_object.name.clone(), StaticState::new(static_object))),
        )
    }

    // send out all initial units
    for unit_state in state.units.values() {
        state
            .ctx
            .tx
            .send(Ok(StreamUnitsResponse {
                time: unit_state.update_time,
                update: Some(Update::Unit(unit_state.unit.clone())),
            }))
            .await?;
    }

    // send out all the initial static objects
    for static_state in state.statics.values() {
        state
            .ctx
            .tx
            .send(Ok(StreamUnitsResponse {
                time: static_state.update_time,
                update: Some(Update::Static(static_state.static_object.clone())),
            }))
            .await?;
    }

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
                handle_event(&mut state, time, event, category).await?;
            }

            // poll units for updates
            _ = interval.tick() => {
                update_units(&mut state).await?;
                update_static_objects(&mut state).await?;
            }
        }
    }
}

/// The state of an active units stream.
// TODO: re-use one state for all concurrent unit streams?
struct State {
    units: HashMap<String, UnitState>,
    statics: HashMap<String, StaticState>,
    ctx: Context,
}

/// Various structs and options used to handle unit updates.
struct Context {
    rpc: MissionRpc,
    tx: Sender<Result<StreamUnitsResponse, Status>>,
    poll_rate: Duration,
    max_backoff: Duration,
}

/// Update the given [State] based on the given [Event].
async fn handle_event(
    state: &mut State,
    time: f64,
    event: Event,
    category: GroupCategory,
) -> Result<(), Error> {
    match event {
        Event::Birth(BirthEvent {
            initiator:
                Some(common::v0::Initiator {
                    initiator: Some(common::v0::initiator::Initiator::Unit(unit)),
                }),
            ..
        }) => {
            // if we are monitoring all the categories, let's watch it.
            // otherwise, we need to be selective on the units we are monitoring
            let unit_category = unit
                .group
                .as_ref()
                .and_then(|group| GroupCategory::from_i32(group.category))
                .unwrap_or(GroupCategory::Unspecified);
            if category == unit_category || category == GroupCategory::Unspecified {
                state
                    .ctx
                    .tx
                    .send(Ok(StreamUnitsResponse {
                        time,
                        update: Some(Update::Unit(unit.clone())),
                    }))
                    .await?;
                state.units.insert(unit.name.clone(), UnitState::new(unit));
            }
        }

        // The dead event is known to not fire reliably in certain cases. This is fine here, because
        // those cases are covered by removing those units when they are not found anymore during
        // attempted updates.
        Event::Dead(DeadEvent {
            initiator:
                Some(common::v0::Initiator {
                    initiator: Some(common::v0::initiator::Initiator::Unit(Unit { name, .. })),
                }),
        }) => {
            if let Some(unit_state) = state.units.remove(&name) {
                state
                    .ctx
                    .tx
                    .send(Ok(StreamUnitsResponse {
                        time,
                        update: Some(Update::Gone(UnitGone {
                            id: unit_state.unit.id,
                            name: unit_state.unit.name.clone(),
                        })),
                    }))
                    .await?;
            }
        }

        // Static Object Birth and Death Events
        Event::Birth(BirthEvent {
            initiator:
                Some(common::v0::Initiator {
                    initiator: Some(common::v0::initiator::Initiator::Static(static_object)),
                }),
            ..
        }) => {
            state
                .ctx
                .tx
                .send(Ok(StreamUnitsResponse {
                    time,
                    update: Some(Update::Static(static_object.clone())),
                }))
                .await?;
            state.statics.insert(static_object.name.clone(), StaticState::new(static_object));
        }

        Event::Dead(DeadEvent {
            initiator:
                Some(common::v0::Initiator {
                    initiator: Some(common::v0::initiator::Initiator::Static(Static { name, .. })),
                }),
        }) => {
            if let Some(static_state) = state.statics.remove(&name) {
                state
                    .ctx
                    .tx
                    .send(Ok(StreamUnitsResponse {
                        time,
                        update: Some(Update::Gone(UnitGone {
                            id: static_state.static_object.id,
                            name: static_state.static_object.name.clone(),
                        })),
                    }))
                    .await?;
            }
        }


        _ => {}
    }

    Ok(())
}

/// Updates all units inside of the provided [State].
async fn update_units(state: &mut State) -> Result<(), Error> {
    let mut units = std::mem::take(&mut state.units);
    // Update all units in parallel (will queue a request for each unit, but the execution will
    // still be throttled by the throughputLimit setting).
    futures_util::future::try_join_all(
        units
            .values_mut()
            .map(|unit_state| update_unit(&state.ctx, unit_state)),
    )
    .await?;

    // remove state for all units that are gone
    units.retain(|_, v| !v.is_gone);
    state.units = units;

    Ok(())
}

/// Updates all the static objects inside the provided [State].
async fn update_static_objects(state: &mut State) -> Result<(), Error> {
    let mut statics = std::mem::take(&mut state.statics);
    // Update all units in parallel (will queue a request for each unit, but the execution will
    // still be throttled by the throughputLimit setting).
    futures_util::future::try_join_all(
        statics
            .values_mut()
            .map(|static_state| update_static_object(&state.ctx, static_state)),
    )
    .await?;

    // remove state for all units that are gone
    statics.retain(|_, v| !v.is_gone);
    state.statics = statics;

    Ok(())
}

async fn update_unit(ctx: &Context, unit_state: &mut UnitState) -> Result<(), Error> {
    if !unit_state.should_update() {
        return Ok(());
    }

    match unit_state.update(ctx).await {
        Ok(changed) => {
            if changed {
                ctx.tx
                    .send(Ok(StreamUnitsResponse {
                        time: unit_state.update_time,
                        update: Some(Update::Unit(unit_state.unit.clone())),
                    }))
                    .await?;
                unit_state.backoff = Duration::ZERO;
                unit_state.last_changed = Instant::now();
            }

            unit_state.last_checked = Instant::now();

            Ok(())
        }
        // if the unit was not found, flag it as gone, and continue with the next unit for now
        Err(err) if err.code() == Code::NotFound => {
            ctx.tx
                .send(Ok(StreamUnitsResponse {
                    // The time provided here is just the last time an update was received for the
                    // unit. It is not exactly the time the unit got destroyed. Since this not-found
                    // handling is just a safeguard if a `Dead` event was missed / not fired by DCS,
                    // it should be ok that it is not the exact time of death.
                    time: unit_state.update_time,
                    update: Some(Update::Gone(UnitGone {
                        id: unit_state.unit.id,
                        name: unit_state.unit.name.clone(),
                    })),
                }))
                .await?;

            unit_state.is_gone = true;

            Ok(())
        }
        Err(err) => Err(err.into()),
    }
}

async fn update_static_object(ctx: &Context, static_state: &mut StaticState) -> Result<(), Error> {
    if !static_state.should_update() {
        return Ok(());
    }

    match static_state.update(ctx).await {
        Ok(changed) => {
            if changed {
                ctx.tx
                    .send(Ok(StreamUnitsResponse {
                        time: static_state.update_time,
                        update: Some(Update::Static(static_state.static_object.clone())),
                    }))
                    .await?;
                static_state.backoff = Duration::ZERO;
                static_state.last_changed = Instant::now();
            }

            static_state.last_checked = Instant::now();

            Ok(())
        }
        // if the unit was not found, flag it as gone, and continue with the next unit for now
        Err(err) if err.code() == Code::NotFound => {
            ctx.tx
                .send(Ok(StreamUnitsResponse {
                    // The time provided here is just the last time an update was received for the
                    // unit. It is not exactly the time the unit got destroyed. Since this not-found
                    // handling is just a safeguard if a `Dead` event was missed / not fired by DCS,
                    // it should be ok that it is not the exact time of death.
                    time: static_state.update_time,
                    update: Some(Update::Gone(UnitGone {
                        id: static_state.static_object.id,
                        name: static_state.static_object.name.clone(),
                    })),
                }))
                .await?;

            static_state.is_gone = true;

            Ok(())
        }
        Err(err) => Err(err.into()),
    }
}

/// The last know information about a unit and various other information to track whether it is
/// worth checking the unit for updates or not.
struct UnitState {
    unit: Unit,
    backoff: Duration,
    /// Time of the update in seconds relative to the mission start.
    update_time: f64,
    last_checked: Instant,
    last_changed: Instant,
    is_gone: bool,
}

struct StaticState {
    static_object: Static,
    backoff: Duration,
    /// Time of the update in seconds relative to the mission start.
    update_time: f64,
    last_checked: Instant,
    last_changed: Instant,
    is_gone: bool,
}


impl UnitState {
    fn new(unit: Unit) -> Self {
        Self {
            unit,
            backoff: Duration::ZERO,
            update_time: 0.0,
            last_checked: Instant::now(),
            last_changed: Instant::now(),
            is_gone: false,
        }
    }

    /// Whether the unit should be checked for updates or not. This can be used to check stationary
    /// units less often.
    fn should_update(&self) -> bool {
        self.last_checked.elapsed() >= self.backoff
    }

    /// Check the unit for updates and return whether the unit got changed or not.
    async fn update(&mut self, ctx: &Context) -> Result<bool, Status> {
        let mut changed = false;

        let res = UnitService::get_transform(
            &ctx.rpc,
            Request::new(GetTransformRequest {
                name: self.unit.name.clone(),
            }),
        )
        .await?;
        let GetTransformResponse {
            time,
            position,
            orientation,
            velocity,
            player_name,
            in_air,
        } = res.into_inner();

        self.update_time = time;

        if let Some((before, after)) = self.unit.position.as_mut().zip(position) {
            if !position_equalish(before, &after) {
                *before = after;
                changed = true;
            }
        }
        if let Some((before, after)) = self.unit.orientation.as_mut().zip(orientation) {
            if !orientation_equalish(before, &after) {
                *before = after;
                changed = true;
            }
        }
        if let Some((before, after)) = self.unit.velocity.as_mut().zip(velocity) {
            if !velocity_equalish(before, &after) {
                *before = after;
                changed = true;
            }
        }
        if let Some((before, after)) = self.unit.player_name.as_mut().zip(player_name) {
            if !after.eq(&*before) {
                *before = after;
                changed = true;
            }
        }

        // Just pass through the current boolean value
        self.unit.in_air = in_air;

        // keep track of when it was last checked and changed and determine a corresponding backoff
        self.last_checked = Instant::now();
        if changed {
            self.last_changed = Instant::now();
            self.backoff = Duration::ZERO;
        } else {
            self.backoff = if self.backoff.is_zero() {
                ctx.poll_rate
            } else {
                (self.backoff * 2).min(ctx.max_backoff)
            }
        }

        Ok(changed)
    }
}

impl StaticState {
    fn new(static_object: Static) -> Self {
        Self {
            static_object,
            backoff: Duration::ZERO,
            update_time: 0.0,
            last_checked: Instant::now(),
            last_changed: Instant::now(),
            is_gone: false,
        }
    }

    /// Whether the unit should be checked for updates or not. This can be used to check stationary
    /// units less often.
    fn should_update(&self) -> bool {
        self.last_checked.elapsed() >= self.backoff
    }

    /// Check the unit for updates and return whether the unit got changed or not.
    async fn update(&mut self, ctx: &Context) -> Result<bool, Status> {
        let mut changed = false;

        let res = UnitService::get_static_transform(
            &ctx.rpc,
            Request::new(GetStaticTransformRequest {
                name: self.static_object.name.clone(),
            }),
        )
        .await?;

        let GetStaticTransformResponse {
            time,
            position,
            orientation,
            velocity,
        } = res.into_inner();

        self.update_time = time;

        if let Some((before, after)) = self.static_object.position.as_mut().zip(position) {
            if !position_equalish(before, &after) {
                *before = after;
                changed = true;
            }
        }
        if let Some((before, after)) = self.static_object.orientation.as_mut().zip(orientation) {
            if !orientation_equalish(before, &after) {
                *before = after;
                changed = true;
            }
        }
        if let Some((before, after)) = self.static_object.velocity.as_mut().zip(velocity) {
            if !velocity_equalish(before, &after) {
                *before = after;
                changed = true;
            }
        }

        // keep track of when it was last checked and changed and determine a corresponding backoff
        self.last_checked = Instant::now();
        if changed {
            self.last_changed = Instant::now();
            self.backoff = Duration::ZERO;
        } else {
            self.backoff = if self.backoff.is_zero() {
                ctx.poll_rate
            } else {
                (self.backoff * 2).min(ctx.max_backoff)
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
    Send(#[from] SendError<Result<StreamUnitsResponse, Status>>),
}

/// Check whether two positions are equal, taking an epsilon into account.
fn position_equalish(l: &Position, r: &Position) -> bool {
    const LL_EPSILON: f64 = 0.000001;
    const ALT_EPSILON: f64 = 0.001;
    (l.lat - r.lat).abs() < LL_EPSILON
        && (l.lon - r.lon).abs() < LL_EPSILON
        && (l.alt - r.alt).abs() < ALT_EPSILON
        && meters_equalish(l.u, r.u)
        && meters_equalish(l.v, r.v)
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
