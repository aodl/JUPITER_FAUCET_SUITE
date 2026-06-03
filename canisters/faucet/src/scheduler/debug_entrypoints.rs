#![cfg(feature = "debug_api")]

use super::*;

pub async fn debug_main_tick_impl() {
    main_tick(true).await;
}
pub async fn debug_rescue_tick_impl() {
    rescue_tick().await;
}
