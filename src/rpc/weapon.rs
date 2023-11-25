use std::pin::Pin;

use futures_util::Stream;
use stubs::weapon;
use stubs::weapon::v0::weapon_service_server::WeaponService;

use tonic::{Request, Response, Status};

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::MissionRpc;
use crate::shutdown::AbortableStream;

#[tonic::async_trait]
impl WeaponService for MissionRpc {
    type StreamWeaponsStream = Pin<
        Box<
            dyn Stream<Item = Result<weapon::v0::StreamWeaponsResponse, tonic::Status>>
                + Send
                + Sync
                + 'static,
        >,
    >;

    async fn stream_weapons(
        &self,
        request: Request<weapon::v0::StreamWeaponsRequest>,
    ) -> Result<Response<Self::StreamWeaponsStream>, Status> {
        let rpc = self.clone();
        let (tx, rx) = mpsc::channel(128);
        tokio::spawn(async move {
            if let Err(crate::weapon::Error::Status(err)) =
                crate::weapon::stream_weapons(request.into_inner(), rpc, tx.clone()).await
            {
                // ignore error, as we don't care at this point whether the channel is closed or not
                let _ = tx.send(Err(err)).await;
            }
        });

        let stream = AbortableStream::new(self.shutdown_signal.signal(), ReceiverStream::new(rx));
        Ok(Response::new(Box::pin(stream)))
    }

    async fn get_transform(
        &self,
        request: Request<weapon::v0::GetTransformRequest>,
    ) -> Result<Response<weapon::v0::GetTransformResponse>, Status> {
        let res = self.request("getWeaponTransform", request).await?;
        Ok(Response::new(res))
    }

    async fn get_tracked_weapon_ids(
        &self,
        request: Request<weapon::v0::GetTrackedWeaponIdsRequest>,
    ) -> Result<Response<weapon::v0::GetTrackedWeaponIdsResponse>, Status> {
        let res = self.request("getTrackedWeaponIds", request).await?;
        Ok(Response::new(res))
    }

    async fn destroy(
        &self,
        request: Request<weapon::v0::DestroyRequest>,
    ) -> Result<Response<weapon::v0::DestroyResponse>, Status> {
        let res = self.request("weaponDestroy", request).await?;
        Ok(Response::new(res))
    }
}
