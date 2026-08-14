//! MUPC 核心层
//!
//! 提供服务协调等核心基础设施

pub mod service_coord;
pub mod service_coord_impl;

pub use service_coord::{ServiceCoordinator, ServiceStatus};
pub use service_coord_impl::ServiceCoordinatorImpl;
