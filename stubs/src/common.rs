pub mod v0 {
    use std::ops::Neg;

    tonic::include_proto!("dcs.common.v0");

    #[derive(Default, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub(crate) struct RawTransform {
        pub position: Option<Position>,
        pub position_north: Option<Vector>,
        pub forward: Option<Vector>,
        pub right: Option<Vector>,
        pub up: Option<Vector>,
        pub velocity: Option<Vector>,
        pub player_name: Option<String>,
        pub in_air: Option<bool>,
        pub fuel: Option<f64>,
    }

    pub(crate) struct Transform {
        pub position: Position,
        pub orientation: Orientation,
        pub velocity: Velocity,
        pub player_name: String,
        pub in_air: bool,
        pub fuel: f64,
    }

    impl From<RawTransform> for Transform {
        fn from(raw: RawTransform) -> Self {
            let RawTransform {
                position,
                position_north,
                forward,
                right,
                up,
                velocity,
                player_name,
                in_air,
                fuel,
            } = raw;
            let position = position.unwrap_or_default();
            let position_north = position_north.unwrap_or_default();
            let forward = forward.unwrap_or_default();
            let right = right.unwrap_or_default();
            let up = up.unwrap_or_default();
            let velocity = velocity.unwrap_or_default();
            let player_name = player_name.unwrap_or_default();
            let in_air = in_air.unwrap_or_default();
            let fuel = fuel.unwrap_or_default();

            let projection_error =
                (position_north.z - position.u).atan2(position_north.x - position.v);
            let heading = forward.z.atan2(forward.x);

            let orientation = Orientation {
                heading: {
                    let heading = heading.to_degrees();
                    if heading < 0.0 {
                        heading + 360.0
                    } else {
                        heading
                    }
                },
                yaw: (heading - projection_error).to_degrees(),
                roll: right.y.asin().neg().to_degrees(),
                pitch: forward.y.asin().to_degrees(),
                forward: Some(forward),
                right: Some(right),
                up: Some(up),
            };

            let velocity = Velocity {
                heading: {
                    let heading = velocity.z.atan2(velocity.x).to_degrees();
                    if heading < 0.0 {
                        heading + 360.0
                    } else {
                        heading
                    }
                },
                speed: (velocity.x.powi(2) + velocity.z.powi(2)).sqrt(),
                velocity: Some(velocity),
            };

            Transform {
                position,
                orientation,
                velocity,
                player_name,
                in_air,
                fuel,
            }
        }
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UnitIntermediate {
        id: u32,
        name: String,
        callsign: String,
        coalition: i32,
        r#type: String,
        group: Option<Group>,
        number_in_group: u32,
        in_air: bool,
        fuel: f64,
        raw_transform: Option<RawTransform>,
    }

    impl From<UnitIntermediate> for Unit {
        fn from(i: UnitIntermediate) -> Self {
            let UnitIntermediate {
                id,
                name,
                callsign,
                coalition,
                r#type,
                group,
                number_in_group,
                raw_transform,
                in_air: _,
                fuel: _,
            } = i;
            let transform = Transform::from(raw_transform.unwrap_or_default());
            Unit {
                id,
                name,
                callsign,
                coalition,
                r#type,
                player_name: Some(transform.player_name),
                in_air: transform.in_air,
                fuel: transform.fuel,
                position: Some(transform.position),
                orientation: Some(transform.orientation),
                velocity: Some(transform.velocity),
                group,
                number_in_group,
            }
        }
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StaticIntermediate {
        id: u32,
        name: String,
        coalition: i32,
        r#type: String,
        raw_transform: Option<RawTransform>,
    }

    impl From<StaticIntermediate> for Static {
        fn from(i: StaticIntermediate) -> Self {
            let StaticIntermediate {
                id,
                name,
                coalition,
                r#type,
                raw_transform,
            } = i;
            let transform = Transform::from(raw_transform.unwrap_or_default());
            Static {
                id,
                name,
                coalition,
                r#type,
                position: Some(transform.position),
                orientation: Some(transform.orientation),
                velocity: Some(transform.velocity),
            }
        }
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WeaponIntermediate {
        id: u32,
        r#type: String,
        raw_transform: Option<RawTransform>,
    }

    impl From<WeaponIntermediate> for Weapon {
        fn from(i: WeaponIntermediate) -> Self {
            let WeaponIntermediate {
                id,
                r#type,
                raw_transform,
            } = i;
            let transform = Transform::from(raw_transform.unwrap_or_default());
            Weapon {
                id,
                r#type,
                position: Some(transform.position),
                orientation: Some(transform.orientation),
                velocity: Some(transform.velocity),
            }
        }
    }
}
