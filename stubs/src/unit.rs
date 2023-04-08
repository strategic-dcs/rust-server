pub mod v0 {
    use crate::common::v0::{RawTransform, Transform};

    tonic::include_proto!("dcs.unit.v0");

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GetTransformResponseIntermediate {
        time: f64,
        raw_transform: Option<RawTransform>,
    }

    impl From<GetTransformResponseIntermediate> for GetTransformResponse {
        fn from(i: GetTransformResponseIntermediate) -> Self {
            let GetTransformResponseIntermediate {
                time,
                raw_transform,
            } = i;
            let transform = Transform::from(raw_transform.unwrap_or_default());
            GetTransformResponse {
                time,
                position: Some(transform.position),
                orientation: Some(transform.orientation),
                velocity: Some(transform.velocity),
                player_name: Some(transform.player_name),
            }
        }
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GetStaticTransformResponseIntermediate {
        time: f64,
        raw_transform: Option<RawTransform>,
    }

    impl From<GetStaticTransformResponseIntermediate> for GetStaticTransformResponse {
        fn from(i: GetStaticTransformResponseIntermediate) -> Self {
            let GetStaticTransformResponseIntermediate {
                time,
                raw_transform,
            } = i;
            let transform = Transform::from(raw_transform.unwrap_or_default());
            GetStaticTransformResponse {
                time,
                position: Some(transform.position),
                orientation: Some(transform.orientation),
                velocity: Some(transform.velocity),
            }
        }
    }
}
