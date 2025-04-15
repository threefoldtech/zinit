pub mod app;
pub mod manager;
pub mod zinit;
pub use app::types::{Status, ZinitResponse, ZinitState};
pub use app::api_trait::{
    ZinitRpcApiClient, ZinitServiceApiClient, ZinitSystemApiClient, ZinitLoggingApiClient,
};
