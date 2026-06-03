pub mod handlers;

use std::sync::Arc;

use crate::auth::anthropic_oauth::AnthropicOAuth;
use crate::router::Router;

#[derive(Clone)]
pub struct AppState {
    pub router: Arc<Router>,
    pub anthropic_oauth: Arc<AnthropicOAuth>,
}
